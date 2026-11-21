//! The app without its window: the three screens' state, what each action
//! does to it, and the picture of it the window draws ([`view`]).
//!
//! The window sends [`Message`]s and reads [`App::snapshot`]. Nothing here
//! blocks: slow work comes back as a [`Task`] for the caller to run.

pub mod log;
pub mod queue;
pub mod settings;
pub mod setup;
pub mod task;
pub mod view;

use tr::tr;

use crate::config::{self, Settings};
use crate::provision::Paths;
pub use queue::Queue;
pub use settings::SettingsPage;
pub use setup::Setup;
pub use task::Task;

#[derive(Debug, Clone)]
pub enum Message {
    Setup(setup::Message),
    Queue(queue::Message),
    /// From the "yt-dlp is old" notice: check the tools for updates.
    CheckForUpdates,
    /// The version of a newer Osprey release, if the daily look found one.
    AppUpdate(Option<String>),
    /// The new-version notice was closed for this session.
    DismissAppUpdate,
}

pub struct App {
    pub setup: Setup,
    pub queue: Queue,
    pub settings: SettingsPage,
    /// Every tool's file was there when the app opened, so it opened on
    /// the Download screen.
    tools_present: bool,
    /// The first inspection hasn't said yet whether those tools run.
    confirming_tools: bool,
    /// Download was pressed while that inspection still ran; it starts as
    /// soon as the tools are known to work.
    start_when_ready: bool,
    /// A newer Osprey that the notice offers, until it is closed.
    app_update: Option<String>,
}

impl App {
    /// The app as it opens in `paths`, with the settings saved there, and
    /// the work that start-up leaves running: the first look at the tools,
    /// the lookups of the links restored from the last session, and the
    /// daily look for a newer release.
    pub fn boot(paths: Paths, saved: Settings) -> (App, Task<Message>) {
        // With every tool in bin/, open on Download rather than showing Setup
        // for the half second the first inspection takes.
        let tools_present = crate::provision::all_present(&paths);
        // A menu entry the user added follows the folder if it was moved.
        // Release builds only: a development build must not repoint it. An
        // installed copy's entry belongs to its package.
        if cfg!(target_os = "linux")
            && !cfg!(debug_assertions)
            && !paths.installed
            && let (Some(data), Ok(exe)) = (crate::desktop::data_home(), std::env::current_exe())
        {
            let _ = crate::desktop::refresh(&data, &exe);
        }
        // Before anything is shown or logged, so all of it is translated.
        crate::i18n::apply(&saved.language);
        // Once a day at most, off the main thread; never in the Flatpak.
        let look_for_update = saved.app_updates && !paths.bundled_tools();
        let update_paths = paths.clone();
        let settings = SettingsPage::new(paths.clone(), saved);
        let (setup, task) = Setup::new(paths.clone());
        let mut queue = Queue::new(paths);
        let mut tasks = vec![
            task.map(Message::Setup),
            queue.restore().map(Message::Queue),
        ];
        if look_for_update {
            tasks.push(Task::perform(
                move || crate::app_update::check(&update_paths),
                |latest| Message::AppUpdate(latest.flatten()),
            ));
        }
        // Links handed over for a dev run are queued, in order, and start
        // right away.
        let dev_urls = queue::dev_urls();
        if !dev_urls.is_empty() {
            for url in dev_urls {
                tasks.push(Task::done(Message::Queue(queue::Message::UrlInput(url))));
                tasks.push(Task::done(Message::Queue(queue::Message::AddUrl)));
            }
            tasks.push(Task::done(Message::Queue(queue::Message::Start)));
        }
        let app = App {
            setup,
            queue,
            settings,
            tools_present,
            confirming_tools: tools_present,
            start_when_ready: false,
            app_update: None,
        };
        (app, Task::batch(tasks))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AppUpdate(latest) => {
                self.app_update = latest;
                Task::none()
            }
            Message::DismissAppUpdate => {
                self.app_update = None;
                Task::none()
            }
            Message::CheckForUpdates => {
                if self.queue.uses_tools() || self.setup.busy() {
                    return Task::none();
                }
                Task::done(Message::Setup(setup::Message::CheckUpdates))
            }
            Message::Setup(message) => {
                // yt-dlp is running for a download or a lookup: what is in
                // bin/ must not be replaced underneath it (Windows refuses
                // to, and a half-replaced tool breaks the download).
                // Installing what is missing replaces nothing that could
                // be running, and must not wait: the links restored from
                // the last session are looked up as soon as the app opens.
                let replaces_tools = matches!(
                    message,
                    setup::Message::UpdateOutdated
                        | setup::Message::ConfirmReinstall
                        | setup::Message::Channel(_)
                );
                if replaces_tools && self.queue.uses_tools() {
                    self.queue.log((
                        crate::log::Level::Warn,
                        tr!("The tools can't be changed while a download is running."),
                    ));
                    return Task::none();
                }
                let task = self.setup.update(message).map(Message::Setup);
                if self.setup.busy() {
                    return task;
                }
                self.confirming_tools = false;
                if self.start_when_ready {
                    self.start_when_ready = false;
                    if self.setup.ready() {
                        return Task::batch([
                            task,
                            Task::done(Message::Queue(queue::Message::Start)),
                        ]);
                    }
                }
                task
            }
            Message::Queue(message) => {
                // The tools must be known to work before yt-dlp is started.
                // Pressed during the first inspection, the start waits for it.
                if matches!(message, queue::Message::Start) && !self.setup.ready() {
                    self.start_when_ready = self.setup.busy();
                    return Task::none();
                }
                let finished = matches!(
                    message,
                    queue::Message::Report(crate::runner::Event::Finished(_))
                );
                let task = self.queue.update(message, self.settings.settings());
                // A finished run may have added to the history file.
                if finished {
                    self.settings.refresh_history();
                }
                task.map(Message::Queue)
            }
        }
    }

    /// The window is closing: a running download is stopped rather than
    /// left behind with no window, and what is still to do is saved.
    pub fn shutdown(&mut self) {
        let _ = self.settings.save();
        self.queue.shutdown();
    }

    /// Download can be shown once the tools work, or while the first
    /// inspection confirms the ones that are there.
    pub fn can_download(&self) -> bool {
        (self.setup.ready() && !self.setup.busy()) || self.confirming_tools
    }

    /// Changes a setting and puts what the activity log should say about
    /// it, if anything, into that log.
    pub fn set_setting(&mut self, key: &str, value: &str) {
        if let Some(note) = self.settings.set(key, value) {
            self.queue.log(note);
        }
    }

    pub fn set_desktop_entry(&mut self, wanted: bool) {
        if let Some(note) = self.settings.set_desktop_entry(wanted) {
            self.queue.log(note);
        }
    }

    pub fn clear_history(&mut self) {
        let note = self.settings.clear_history();
        self.queue.log(note);
    }

    pub fn link_failed(&mut self, url: &str, error: &str) {
        self.queue.log((
            crate::log::Level::Warn,
            tr!("Could not open {url}: {e}", url, e = error),
        ));
    }
}

pub const WEBSITE: &str = "https://github.com/indep-arg/Osprey";
pub const REPORT: &str = "https://github.com/indep-arg/Osprey/issues/new/choose";

/// What to say when the folder the app lives in can't be determined, in
/// the system's language: there are no settings to read a choice from.
pub fn folder_not_found(error: &str) -> String {
    crate::i18n::apply("");
    tr!("Could not find the folder this app is in: {e}", e = error)
}

pub fn folder_dialog_title() -> String {
    tr!("Where to save downloads")
}

/// The names this app's files had before it was called Osprey. A portable
/// folder updated in place keeps its settings, queue and window: each file
/// is renamed once, and only when the new one isn't there yet.
pub fn adopt_earlier_files(paths: &Paths) {
    const RENAMED: &[(&str, &str)] = &[
        ("mimirdlp.config", config::FILE_NAME),
        ("mimirdlp.queue", config::QUEUE_NAME),
        ("mimirdlp.window", config::WINDOW_NAME),
    ];
    for (old, new) in RENAMED {
        let (old, new) = (paths.app_dir.join(old), paths.app_dir.join(new));
        if old.is_file() && !new.exists() {
            let _ = std::fs::rename(old, new);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::tempdir;

    /// In English whatever this machine's language is: the translation
    /// in use is shared by every test of the crate.
    fn app(dir: &std::path::Path) -> App {
        let english = Settings {
            language: "en".into(),
            ..Settings::default()
        };
        App::boot(Paths::new(dir.to_path_buf()), english).0
    }

    /// Download pressed before the tools are known to work waits for the
    /// first inspection instead of starting yt-dlp blind, and is dropped
    /// when that inspection finds them missing.
    #[test]
    fn a_start_waits_until_the_tools_are_known_to_work() {
        let dir = tempdir();
        let mut app = app(&dir);
        assert!(app.setup.busy(), "the first inspection is running");
        let _ = app.update(Message::Queue(queue::Message::UrlInput(
            "https://example.com/a".into(),
        )));
        let _ = app.update(Message::Queue(queue::Message::AddUrl));
        let _ = app.update(Message::Queue(queue::Message::Start));
        assert!(!app.queue.running());
        assert!(app.start_when_ready);
        // The inspection ends with nothing installed.
        let _ = app.update(Message::Setup(setup::Message::Report(setup::Report::Done)));
        assert!(!app.start_when_ready && !app.queue.running());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Looking a link up runs yt-dlp, so the tools stay as they are until
    /// it is done.
    #[test]
    fn the_tools_are_not_replaced_while_yt_dlp_is_in_use() {
        let dir = tempdir();
        let mut app = app(&dir);
        let _ = app.update(Message::Setup(setup::Message::Report(setup::Report::Done)));
        let _ = app.update(Message::Queue(queue::Message::UrlInput(
            "https://example.com/a".into(),
        )));
        let _ = app.update(Message::Queue(queue::Message::AddUrl));
        assert!(app.queue.uses_tools(), "its lookup hasn't answered");
        let _ = app.update(Message::Setup(setup::Message::ConfirmReinstall));
        assert!(!app.setup.busy());
        assert!(
            app.queue
                .activity()
                .latest(crate::log::Level::Warn)
                .is_some()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A first launch with links already waiting (restored, or added
    /// while the tools were looked at) still installs what is missing.
    #[test]
    fn missing_tools_are_installed_even_while_links_are_looked_up() {
        let dir = tempdir();
        let mut app = app(&dir);
        let _ = app.update(Message::Queue(queue::Message::UrlInput(
            "https://example.com/a".into(),
        )));
        let _ = app.update(Message::Queue(queue::Message::AddUrl));
        assert!(app.queue.uses_tools());
        for component in crate::provision::Component::ALL {
            let _ = app.update(Message::Setup(setup::Message::Report(
                setup::Report::Status(component, crate::provision::Status::Missing),
            )));
        }
        let _ = app.update(Message::Setup(setup::Message::Report(setup::Report::Done)));
        let _ = app.update(Message::Setup(setup::Message::InstallMissing));
        assert_eq!(
            app.setup.job(),
            Some(setup::Job::Installing(setup::Scope::Missing))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn files_from_before_the_rename_are_adopted_once() {
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        std::fs::write(dir.join("mimirdlp.config"), "verbose=yes\n").unwrap();
        std::fs::write(dir.join("mimirdlp.queue"), "old queue").unwrap();
        std::fs::write(dir.join(config::QUEUE_NAME), "new queue").unwrap();
        adopt_earlier_files(&paths);
        assert!(Settings::load(&paths).verbose);
        assert!(!dir.join("mimirdlp.config").exists());
        // A file already there under the new name is never replaced.
        assert_eq!(
            std::fs::read_to_string(dir.join(config::QUEUE_NAME)).unwrap(),
            "new queue"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
