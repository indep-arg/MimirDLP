//! The setup screen's state: what is installed, and the actions available
//! on it (install missing, check for updates, update outdated, reinstall
//! all). Every step is written to the activity log with the tags
//! [`crate::log`] defines, and the file being downloaded reports its
//! progress.

use tr::tr;

use super::log::Log;
use super::task::Task;
use crate::provision::{self, Channel, Component, Event, Level, Paths, Status, UpdateCheck};

/// What the worker thread sends back while it runs.
#[derive(Debug, Clone)]
pub enum Report {
    Provision(Event),
    Status(Component, Status),
    Update(Component, UpdateCheck),
    /// The app's folder can't be written to, so installing would only fail.
    Unwritable,
    Done,
}

#[derive(Debug, Clone)]
pub enum Message {
    Report(Report),
    InstallMissing,
    CheckUpdates,
    UpdateOutdated,
    AskReinstall,
    ConfirmReinstall,
    CancelReinstall,
    Channel(Channel),
    /// The diagnostics were copied (the window does the copying, with
    /// [`Setup::diagnostics`]).
    CopyDiagnostics,
    /// The first launch's welcome: show or hide the full Setup screen.
    ToggleDetails,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Inspecting,
    Installing(Scope),
    CheckingUpdates,
}

/// Which components an install covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Missing,
    Outdated,
    All,
    /// yt-dlp again, from the channel just chosen.
    Channel(Channel),
}

#[derive(Default)]
pub struct Row {
    /// None until the first inspection finishes.
    pub status: Option<Status>,
    /// None until checked ("unchecked" in the label shown for it).
    pub update: Option<UpdateCheck>,
}

pub struct Transfer {
    pub file: String,
    pub received: u64,
    pub total: Option<u64>,
}

pub struct Setup {
    paths: Paths,
    rows: [Row; 3],
    /// Where yt-dlp comes from, as remembered in `bin/`.
    channel: Channel,
    job: Option<Job>,
    confirm_reinstall: bool,
    log: Log,
    transfer: Option<Transfer>,
    /// Debug builds only: an action to run once the first inspection ends.
    pending: Option<Message>,
    /// A first launch shows a welcome with one progress bar instead of
    /// this screen, until everything is installed.
    welcome: bool,
    /// The full screen was asked for from the welcome.
    details: bool,
    /// Install what is missing as soon as the first inspection ends, so a
    /// first launch needs no button press. Off when the folder can't be
    /// written to, when the tools came with the package, and in debug
    /// builds with `OSPREY_DEV_ACTION=none`.
    auto_install: bool,
}

impl Setup {
    pub fn new(paths: Paths) -> (Self, Task<Message>) {
        let setup = Setup {
            paths: paths.clone(),
            rows: Default::default(),
            channel: provision::ytdlp_channel(&paths),
            welcome: first_launch(&paths),
            details: false,
            job: Some(Job::Inspecting),
            confirm_reinstall: false,
            log: Log::default(),
            transfer: None,
            pending: dev_action().filter(|_| !paths.bundled_tools()),
            auto_install: !dev_no_auto_install() && !paths.bundled_tools(),
        };
        let task = in_thread(move |send| {
            if let Err(e) = provision::app_dir_writable(&paths) {
                send(Report::Unwritable);
                let mut log =
                    |text: String| send(Report::Provision(Event::Log(Level::Error, text)));
                log(tr!(
                    "This folder can't be written to: {folder} ({e}).",
                    folder = paths.app_dir.display(),
                    e
                ));
                log(tr!(
                    "Osprey keeps its tools, settings and history next to itself. \
                     Move its folder somewhere you can write to (not Program Files), \
                     such as your user folder or a USB stick."
                ));
            }
            provision::clean_leftovers(&paths);
            for component in Component::ALL {
                send(Report::Status(
                    component,
                    provision::inspect(component, &paths),
                ));
            }
        });
        (setup, task)
    }

    /// Everything is installed and runs, so downloading can work.
    pub fn ready(&self) -> bool {
        self.rows
            .iter()
            .all(|row| matches!(row.status, Some(Status::Installed { .. })))
    }

    /// Something is missing or broken, so "Install missing components" has
    /// work to do.
    pub fn needs_install(&self) -> bool {
        self.rows
            .iter()
            .any(|row| matches!(row.status, Some(Status::Missing | Status::Broken)))
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn job(&self) -> Option<Job> {
        self.job
    }

    /// Each component with what is known about it, in [`Component::ALL`]'s
    /// order.
    pub fn rows(&self) -> impl Iterator<Item = (Component, &Row)> {
        Component::ALL.into_iter().zip(self.rows.iter())
    }

    pub fn channel(&self) -> Channel {
        self.channel
    }

    /// "Reinstall all" was pressed and waits to be confirmed.
    pub fn confirming_reinstall(&self) -> bool {
        self.confirm_reinstall
    }

    pub fn transfer(&self) -> Option<&Transfer> {
        self.transfer.as_ref()
    }

    pub fn activity(&self) -> &Log {
        &self.log
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The tools came with the package (the Flatpak): this screen only
    /// shows them, and they are updated with Osprey.
    pub fn bundled(&self) -> bool {
        self.paths.bundled_tools()
    }

    pub fn ytdlp_version(&self) -> Option<&str> {
        match &self.rows[index(Component::YtDlp)].status {
            Some(Status::Installed { version }) => Some(version),
            _ => None,
        }
    }

    /// How many days old the installed yt-dlp is, when it is old enough to
    /// say so ([`provision::ytdlp_stale_days`]): not once a check found
    /// nothing newer.
    pub fn ytdlp_age(&self) -> Option<i64> {
        let row = &self.rows[index(Component::YtDlp)];
        if row.update == Some(UpdateCheck::Current) {
            return None;
        }
        self.stale_age(Component::YtDlp)
    }

    /// yt-dlp's age in days once it is stale, whatever the last check
    /// said: what its row shows. A yt-dlp weeks old is the usual reason a
    /// site stops working.
    pub fn stale_age(&self, component: Component) -> Option<i64> {
        match &self.rows[index(component)].status {
            Some(Status::Installed { version }) if component == Component::YtDlp => {
                provision::ytdlp_age_days(version, provision::today())
                    .filter(|age| *age > provision::ytdlp_stale_days(&self.paths))
            }
            _ => None,
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Only changes what is shown, so it works while busy too.
        if matches!(message, Message::ToggleDetails) {
            self.details = !self.details;
            return Task::none();
        }
        // Changes nothing, so it works in the Flatpak and while busy too.
        if matches!(message, Message::CopyDiagnostics) {
            self.log.push((
                Level::Ok,
                tr!("Diagnostic information copied. Paste it into your bug report."),
            ));
            return Task::none();
        }
        // Nothing is installed or updated from here when the package
        // brought the tools: only reports (the inspection) get through.
        if (self.busy() || self.bundled()) && !matches!(message, Message::Report(_)) {
            return Task::none();
        }
        match message {
            Message::Report(report) => return self.report(report),
            Message::InstallMissing => {
                let targets =
                    self.select(|row| matches!(row.status, Some(Status::Missing | Status::Broken)));
                return self.install(targets, Scope::Missing);
            }
            Message::CheckUpdates => {
                let targets = self.select(|row| {
                    matches!(row.status, Some(Status::Installed { .. }))
                        && matches!(row.update, None | Some(UpdateCheck::Failed(_)))
                });
                self.start(Job::CheckingUpdates);
                let paths = self.paths.clone();
                return in_thread(move |send| {
                    send(Report::Provision(Event::Log(
                        Level::Info,
                        tr!("Checking for updates... (requires internet)"),
                    )));
                    for component in targets {
                        let result = provision::check_update(component, &paths);
                        if let UpdateCheck::Failed(reason) = &result {
                            send(Report::Provision(Event::Log(
                                Level::Warn,
                                tr!(
                                    "Could not check {component} for updates: {reason}",
                                    component = component.label(),
                                    reason
                                ),
                            )));
                        }
                        send(Report::Update(component, result));
                    }
                });
            }
            Message::UpdateOutdated => {
                let targets = self.select(|row| {
                    matches!(row.status, Some(Status::Installed { .. }))
                        && row.update == Some(UpdateCheck::Outdated)
                });
                return self.install(targets, Scope::Outdated);
            }
            Message::AskReinstall => self.confirm_reinstall = true,
            Message::CancelReinstall => self.confirm_reinstall = false,
            Message::ConfirmReinstall => {
                self.confirm_reinstall = false;
                return self.install(Component::ALL.to_vec(), Scope::All);
            }
            Message::Channel(channel) => return self.switch_channel(channel),
            Message::CopyDiagnostics | Message::ToggleDetails => {}
        }
        Task::none()
    }

    fn report(&mut self, report: Report) -> Task<Message> {
        match report {
            Report::Provision(Event::Log(level, text)) => self.log.push((level, text)),
            Report::Provision(Event::Transfer {
                file,
                received,
                total,
            }) => {
                self.transfer = Some(Transfer {
                    file,
                    received,
                    total,
                })
            }
            Report::Provision(Event::TransferFinished) => self.transfer = None,
            Report::Status(component, status) => {
                let row = &mut self.rows[index(component)];
                row.status = Some(status);
                row.update = None;
            }
            Report::Update(component, check) => self.rows[index(component)].update = Some(check),
            Report::Unwritable => self.auto_install = false,
            Report::Done => {
                let first_inspection = self.job == Some(Job::Inspecting);
                self.job = None;
                self.transfer = None;
                // Once everything runs there is nothing left to welcome to;
                // a later visit shows the full screen.
                if self.ready() {
                    self.welcome = false;
                }
                if !first_inspection {
                    return Task::none();
                }
                if let Some(message) = self.pending.take() {
                    return Task::done(message);
                }
                if self.auto_install && self.needs_install() {
                    self.log.push((
                        Level::Info,
                        tr!("Some components are missing or broken, installing them now"),
                    ));
                    return Task::done(Message::InstallMissing);
                }
            }
        }
        Task::none()
    }

    fn start(&mut self, job: Job) {
        if !self.log.is_empty() {
            // A blank line between runs keeps them visually separate.
            self.log.push((Level::Info, String::new()));
        }
        self.job = Some(job);
    }

    /// Remembers the channel and, if yt-dlp is there, replaces it with
    /// that channel's build right away. A missing one is installed from it
    /// by "Install missing components".
    fn switch_channel(&mut self, channel: Channel) -> Task<Message> {
        if self.choose_channel(channel) {
            return self.install(vec![Component::YtDlp], Scope::Channel(channel));
        }
        Task::none()
    }

    /// Saves a new channel; true when the yt-dlp in `bin/` is now from the
    /// other one and has to be replaced.
    fn choose_channel(&mut self, channel: Channel) -> bool {
        if channel == self.channel {
            return false;
        }
        if let Err(e) = provision::set_ytdlp_channel(&self.paths, channel) {
            self.log.push((
                Level::Error,
                tr!("Could not save the choice of yt-dlp builds: {e}", e),
            ));
            return false;
        }
        self.channel = channel;
        let row = &self.rows[index(Component::YtDlp)];
        matches!(row.status, Some(Status::Installed { .. } | Status::Broken))
    }

    fn select(&self, wanted: impl Fn(&Row) -> bool) -> Vec<Component> {
        Component::ALL
            .into_iter()
            .filter(|component| wanted(&self.rows[index(*component)]))
            .collect()
    }

    fn install(&mut self, targets: Vec<Component>, scope: Scope) -> Task<Message> {
        if targets.is_empty() {
            return Task::none();
        }
        self.start(Job::Installing(scope));
        let paths = self.paths.clone();
        in_thread(move |send| {
            let mut failed = false;
            for component in targets {
                let result =
                    provision::install(component, &paths, &mut |e| send(Report::Provision(e)));
                send(Report::Status(
                    component,
                    provision::inspect(component, &paths),
                ));
                match result {
                    Ok(()) => send(Report::Update(component, UpdateCheck::Current)),
                    Err(_) => failed = true,
                }
            }
            let mut log = |level, text: String| send(Report::Provision(Event::Log(level, text)));
            if failed {
                log(Level::Warn, tr!("Some components are missing or broken"));
                return;
            }
            log(
                Level::Success,
                match scope {
                    Scope::Missing => tr!("Installation completed"),
                    Scope::Outdated => tr!("Update completed"),
                    Scope::All => tr!("Full reinstallation completed"),
                    Scope::Channel(Channel::Nightly) => {
                        tr!("yt-dlp now comes from the nightly builds")
                    }
                    Scope::Channel(Channel::Stable) => {
                        tr!("yt-dlp now comes from the stable releases")
                    }
                },
            );
            if matches!(scope, Scope::Missing | Scope::All) {
                log(
                    Level::Info,
                    tr!(
                        "Binaries located in: {folder}",
                        folder = paths.bin_dir.display()
                    ),
                );
            }
        })
    }

    /// The first launch's welcome is showing rather than the full screen.
    pub fn welcoming(&self) -> bool {
        self.welcome && !self.details
    }

    /// This is still the first launch: the full screen, when asked for,
    /// offers the way back to the welcome.
    pub fn first_launch(&self) -> bool {
        self.welcome
    }

    /// The welcome's progress: which step of the three is running (1 to
    /// 3) and how far the whole install is, from 0 to 1. A component counts
    /// once it runs; the one downloading counts for what it received.
    pub fn welcome_progress(&self) -> (usize, f32) {
        let installed = self
            .rows
            .iter()
            .filter(|row| matches!(row.status, Some(Status::Installed { .. })))
            .count();
        let current = self.transfer.as_ref().map_or(0.0, |t| match t.total {
            Some(total) if total > 0 => (t.received as f64 / total as f64).min(1.0) as f32,
            _ => 0.0,
        });
        let total = self.rows.len();
        let step = (installed + 1).min(total);
        (step, ((installed as f32 + current) / total as f32).min(1.0))
    }

    /// What a bug report needs to know about this copy: its version, the
    /// system, how it was installed and each tool's version and state.
    /// Kept in English for whoever reads the report, and free of anything
    /// personal: no paths, links or user names.
    pub fn diagnostics(&self) -> String {
        let install = if self.bundled() {
            "Flatpak (tools from the package)"
        } else if self.paths.installed {
            "installed (files in the user's data folder)"
        } else {
            "portable (files next to the app)"
        };
        let builds = if self.bundled() {
            "from the package"
        } else {
            match self.channel {
                Channel::Nightly => "nightly",
                Channel::Stable => "stable",
            }
        };
        let tools = Component::ALL.map(|component| {
            let row = &self.rows[index(component)];
            (component.label(), tool_state(row))
        });
        render_diagnostics(&system(), install, builds, &tools)
    }
}

fn render_diagnostics(
    system: &str,
    install: &str,
    builds: &str,
    tools: &[(&str, String)],
) -> String {
    let mut out = format!(
        "Osprey {}
System: {system}
Install: {install}
yt-dlp builds: {builds}

Tools:
",
        env!("CARGO_PKG_VERSION")
    );
    for (name, state) in tools {
        out += &format!(
            "- {name}: {state}
"
        );
    }
    out
}

/// One tool's line in the diagnostics, from what the screen knows.
fn tool_state(row: &Row) -> String {
    match &row.status {
        None => "not checked yet".into(),
        Some(Status::Missing) => "not installed".into(),
        Some(Status::Broken) => "installed but not responding".into(),
        Some(Status::Installed { version }) => {
            let update = match &row.update {
                None => "updates not checked",
                Some(UpdateCheck::Current) => "up to date",
                Some(UpdateCheck::Outdated) => "update available",
                Some(UpdateCheck::Unknown) => "installed before update tracking",
                Some(UpdateCheck::Failed(_)) => "update check failed",
            };
            format!("{version} ({update})")
        }
    }
}

/// The system and processor, and on Linux the distribution's name (which
/// says nothing about the user).
fn system() -> String {
    let base = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    #[cfg(target_os = "linux")]
    {
        // Inside the Flatpak /etc is the runtime's: the host's is here.
        let release = ["/run/host/os-release", "/etc/os-release"]
            .iter()
            .find_map(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| pretty_name(&text));
        if let Some(name) = release {
            return format!("{base}, {name}");
        }
    }
    base
}

/// `PRETTY_NAME` from an os-release file, without its quotes.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn pretty_name(os_release: &str) -> Option<String> {
    os_release
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|name| !name.is_empty())
}

/// No tool in the folder yet, and none expected from a package: the
/// first launch, which gets the welcome.
fn first_launch(paths: &Paths) -> bool {
    !paths.bundled_tools() && provision::none_present(paths)
}

fn index(component: Component) -> usize {
    Component::ALL
        .iter()
        .position(|c| *c == component)
        .expect("every component is in ALL")
}

/// Runs `job` on its own thread and turns what it sends into messages:
/// downloading, hashing and extracting never block the window.
fn in_thread(job: impl FnOnce(&mut dyn FnMut(Report)) + Send + 'static) -> Task<Message> {
    Task::stream(move |send| {
        job(&mut |report| send(Message::Report(report)));
        send(Message::Report(Report::Done));
    })
}

/// Debug builds only: `OSPREY_DEV_ACTION=none` keeps a first launch from
/// installing anything, to look at an empty folder's screens.
fn dev_no_auto_install() -> bool {
    cfg!(debug_assertions) && std::env::var("OSPREY_DEV_ACTION").is_ok_and(|a| a == "none")
}

/// Debug builds only: `OSPREY_DEV_ACTION=install|check|update|ask-reinstall|reinstall`
/// runs that action after the first inspection, instead of a click.
fn dev_action() -> Option<Message> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("OSPREY_DEV_ACTION").ok()?.as_str() {
        "install" => Some(Message::InstallMissing),
        "check" => Some(Message::CheckUpdates),
        "update" => Some(Message::UpdateOutdated),
        "ask-reinstall" => Some(Message::AskReinstall),
        "reinstall" => Some(Message::ConfirmReinstall),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_ytdlp(version: &str, update: Option<UpdateCheck>) -> Setup {
        let mut rows: [Row; 3] = Default::default();
        rows[index(Component::YtDlp)] = Row {
            status: Some(Status::Installed {
                version: version.into(),
            }),
            update,
        };
        Setup {
            paths: Paths::new("/app".into()),
            rows,
            channel: Channel::Nightly,
            welcome: false,
            details: false,
            job: None,
            confirm_reinstall: false,
            log: Log::default(),
            transfer: None,
            pending: None,
            auto_install: true,
        }
    }

    /// Whether finishing the current job made Setup start installing on its
    /// own (it says so in the log first).
    fn installs_by_itself(mut setup: Setup) -> bool {
        let _ = setup.report(Report::Done);
        setup
            .log
            .iter()
            .any(|line| line.text.starts_with("Some components are missing"))
    }

    /// A folder with no tool at all is a first launch: the welcome shows
    /// until everything runs, and "Show details" swaps it for the full
    /// screen and back. A folder with tools never gets it.
    #[test]
    fn a_first_launch_is_welcomed_until_everything_runs() {
        let dir = crate::testing::tempdir();
        let paths = Paths::new(dir.clone());
        assert!(first_launch(&paths));
        assert!(!first_launch(&Paths::bundled(dir.clone(), vec![])));
        std::fs::create_dir_all(&paths.bin_dir).unwrap();
        std::fs::write(paths.tool("yt-dlp"), "").unwrap();
        assert!(!first_launch(&paths), "a folder with a tool isn't new");
        std::fs::remove_dir_all(dir).unwrap();

        let mut setup = with_ytdlp("2026.10.30", None);
        setup.welcome = true;
        setup.rows[index(Component::Ffmpeg)].status = Some(Status::Missing);
        assert!(setup.welcoming());
        let _ = setup.update(Message::ToggleDetails);
        assert!(!setup.welcoming());
        let _ = setup.update(Message::ToggleDetails);
        assert!(setup.welcoming());

        let _ = setup.report(Report::Done);
        assert!(setup.welcoming(), "FFmpeg is still missing");
        let _ = setup.report(Report::Status(
            Component::Ffmpeg,
            Status::Installed {
                version: "1".into(),
            },
        ));
        let _ = setup.report(Report::Status(
            Component::Deno,
            Status::Installed {
                version: "1".into(),
            },
        ));
        let _ = setup.report(Report::Done);
        assert!(!setup.welcoming(), "nothing left to welcome to");
    }

    /// One bar for the whole install: each installed component is a
    /// third, the downloading one adds what it received.
    #[test]
    fn the_welcome_counts_steps_and_progress() {
        let mut setup = with_ytdlp("2026.10.30", None);
        setup.rows[index(Component::Ffmpeg)].status = Some(Status::Missing);
        setup.rows[index(Component::Deno)].status = Some(Status::Missing);
        assert_eq!(setup.welcome_progress(), (2, 1.0 / 3.0));
        setup.transfer = Some(Transfer {
            file: "ffmpeg.tar.xz".into(),
            received: 50,
            total: Some(100),
        });
        assert_eq!(setup.welcome_progress(), (2, 0.5));
        for component in Component::ALL {
            setup.rows[index(component)].status = Some(Status::Installed {
                version: "1".into(),
            });
        }
        setup.transfer = None;
        assert_eq!(setup.welcome_progress(), (3, 1.0));
    }

    /// The report says what a bug report needs and nothing personal: no
    /// folder, no user name.
    #[test]
    fn the_diagnostics_describe_the_copy_without_personal_details() {
        let dir = crate::testing::tempdir();
        let mut setup = with_ytdlp("2026.10.30.123456", Some(UpdateCheck::Current));
        setup.paths = Paths::new(dir.clone());
        setup.rows[index(Component::Ffmpeg)].status = Some(Status::Broken);
        let report = setup.diagnostics();
        assert!(
            report.starts_with(&format!("Osprey {}\n", env!("CARGO_PKG_VERSION"))),
            "{report}"
        );
        for expected in [
            "Install: portable",
            "yt-dlp builds: nightly",
            "- yt-dlp: 2026.10.30.123456 (up to date)",
            "- FFmpeg: installed but not responding",
            "- Deno: not checked yet",
        ] {
            assert!(report.contains(expected), "{expected} in:\n{report}");
        }
        let dir_text = dir.display().to_string();
        assert!(!report.contains(&dir_text), "{report}");
        if let Ok(user) = std::env::var("USER") {
            assert!(!report.contains(&format!("/{user}")), "{report}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_distribution_name_is_read_from_os_release() {
        let text = "NAME=\"Fedora Linux\"\nPRETTY_NAME=\"Fedora Linux 45 (Workstation Edition)\"\nID=fedora\n";
        assert_eq!(
            pretty_name(text).as_deref(),
            Some("Fedora Linux 45 (Workstation Edition)")
        );
        assert_eq!(pretty_name("ID=arch\n"), None);
        assert_eq!(pretty_name("PRETTY_NAME=\"\"\n"), None);
    }

    /// Copying works in the Flatpak and during a job, where every other
    /// action is ignored.
    #[test]
    fn diagnostics_can_be_copied_while_busy() {
        let mut setup = with_ytdlp("2026.10.30", None);
        setup.job = Some(Job::CheckingUpdates);
        let _ = setup.update(Message::CopyDiagnostics);
        assert!(
            setup
                .log
                .iter()
                .any(|l| l.text.starts_with("Diagnostic information copied")),
            "{:?}",
            setup.log.iter().collect::<Vec<_>>()
        );
    }

    /// Choosing the other channel is remembered in `bin/` and replaces an
    /// installed yt-dlp; choosing the same one does nothing, and a missing
    /// yt-dlp is left for "Install missing components".
    #[test]
    fn choosing_a_channel_is_saved_and_replaces_yt_dlp() {
        let dir = crate::testing::tempdir();
        let mut setup = with_ytdlp("2026.10.30.123456", None);
        setup.paths = Paths::new(dir.clone());

        assert!(!setup.choose_channel(Channel::Nightly), "already nightly");
        assert!(setup.choose_channel(Channel::Stable));
        assert_eq!(provision::ytdlp_channel(&setup.paths), Channel::Stable);
        assert!(!setup.choose_channel(Channel::Stable));

        setup.rows[index(Component::YtDlp)].status = Some(Status::Missing);
        assert!(
            !setup.choose_channel(Channel::Nightly),
            "nothing to replace"
        );
        assert_eq!(provision::ytdlp_channel(&setup.paths), Channel::Nightly);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_first_launch_installs_what_is_missing_by_itself() {
        let mut missing = with_ytdlp("2026.09.16", None);
        missing.rows[index(Component::Ffmpeg)].status = Some(Status::Missing);
        missing.job = Some(Job::Inspecting);
        assert!(installs_by_itself(missing));

        let mut broken = with_ytdlp("2026.09.16", None);
        broken.rows[index(Component::Deno)].status = Some(Status::Broken);
        broken.job = Some(Job::Inspecting);
        assert!(installs_by_itself(broken));
    }

    #[test]
    fn nothing_is_installed_by_itself_when_it_would_fail_or_was_not_a_first_look() {
        let missing = || {
            let mut setup = with_ytdlp("2026.09.16", None);
            setup.rows[index(Component::Ffmpeg)].status = Some(Status::Missing);
            setup.job = Some(Job::Inspecting);
            setup
        };

        let mut unwritable = missing();
        let _ = unwritable.report(Report::Unwritable);
        assert!(!installs_by_itself(unwritable));

        // A failed install ends with something still missing; trying again
        // on its own would loop.
        let mut after_install = missing();
        after_install.job = Some(Job::Installing(Scope::Missing));
        assert!(!installs_by_itself(after_install));

        let mut complete = with_ytdlp("2026.09.16", None);
        for component in Component::ALL {
            complete.rows[index(component)].status = Some(Status::Installed {
                version: "1".into(),
            });
        }
        complete.job = Some(Job::Inspecting);
        assert!(!installs_by_itself(complete));
    }

    #[test]
    fn bundled_tools_are_only_shown_never_installed_or_updated() {
        let mut setup = with_ytdlp("2026.09.16", None);
        setup.paths = Paths::bundled("/data".into(), vec!["/app/bin".into()]);
        setup.auto_install = false;
        setup.rows[index(Component::Ffmpeg)].status = Some(Status::Missing);
        for message in [
            Message::InstallMissing,
            Message::CheckUpdates,
            Message::UpdateOutdated,
            Message::ConfirmReinstall,
        ] {
            let _ = setup.update(message);
            assert!(!setup.busy(), "nothing may start");
        }
        let _ = setup.update(Message::AskReinstall);
        assert!(!setup.confirm_reinstall);

        let (fresh, _) = Setup::new(Paths::bundled("/data".into(), vec![]));
        assert!(!fresh.auto_install && fresh.pending.is_none());
    }

    #[test]
    fn a_bundled_yt_dlp_counts_as_old_later() {
        let days_ago = |n: i64| {
            let (y, m, d) = civil_from_days(provision::today() - n);
            format!("{y}.{m:02}.{d:02}")
        };
        let mut portable = with_ytdlp(&days_ago(30), None);
        assert_eq!(portable.ytdlp_age(), Some(30));
        portable.paths = Paths::bundled("/data".into(), vec![]);
        assert_eq!(portable.ytdlp_age(), None, "a stable release a month old");
        let bundled_old = Setup {
            paths: Paths::bundled("/data".into(), vec![]),
            ..with_ytdlp(&days_ago(100), None)
        };
        assert_eq!(bundled_old.ytdlp_age(), Some(100));
    }

    /// The calendar date of a day number (Hinnant's `civil_from_days`).
    fn civil_from_days(days: i64) -> (i64, i64, i64) {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        (yoe + era * 400 + i64::from(m <= 2), m, d)
    }

    #[test]
    fn an_old_yt_dlp_is_reported_until_a_check_finds_nothing_newer() {
        let old = with_ytdlp("2020.01.01", None);
        assert!(old.ytdlp_age().is_some_and(|age| age > 365));
        let checked = with_ytdlp("2020.01.01", Some(UpdateCheck::Current));
        assert_eq!(checked.ytdlp_age(), None);
        let unknown = with_ytdlp("not-a-date", None);
        assert_eq!(unknown.ytdlp_age(), None);
    }
}
