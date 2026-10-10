//! The setup screen: what is installed, and the actions available on it
//! (install missing, check for updates, update outdated, reinstall all).
//! Every step is written to the activity log with the tags [`crate::log`]
//! defines, and the file being downloaded gets a progress bar.

use std::thread;

use iced::futures::channel::mpsc;
use iced::widget::{Column, button, column, container, progress_bar, row, space, text};
use iced::{Alignment, Element, Font, Length, Task, Theme};
use tr::tr;

use super::{amount, card, log as log_panel, style};
use crate::provision::{self, Component, Event, Level, Paths, Status, UpdateCheck};

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Job {
    Inspecting,
    Installing(Scope),
    CheckingUpdates,
}

/// Which components an install covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Missing,
    Outdated,
    All,
}

#[derive(Default)]
struct Row {
    /// None until the first inspection finishes.
    status: Option<Status>,
    /// None until checked ("unchecked" in the label shown for it).
    update: Option<UpdateCheck>,
}

struct Transfer {
    file: String,
    received: u64,
    total: Option<u64>,
}

pub struct Setup {
    paths: Paths,
    rows: [Row; 3],
    job: Option<Job>,
    confirm_reinstall: bool,
    log: Vec<(Level, String)>,
    transfer: Option<Transfer>,
    /// Debug builds only: an action to run once the first inspection ends.
    pending: Option<Message>,
    /// Install what is missing as soon as the first inspection ends, so a
    /// first launch needs no button press. Off when the folder can't be
    /// written to, and in debug builds with `YTP_DEV_ACTION=none`.
    auto_install: bool,
}

impl Setup {
    pub fn new(paths: Paths) -> (Self, Task<Message>) {
        let setup = Setup {
            paths: paths.clone(),
            rows: Default::default(),
            job: Some(Job::Inspecting),
            confirm_reinstall: false,
            log: Vec::new(),
            transfer: None,
            pending: dev_action(),
            auto_install: !dev_no_auto_install(),
        };
        let task = in_thread(move |send| {
            if let Err(e) = provision::app_dir_writable(&paths) {
                send(Report::Unwritable);
                let log = |text: String| send(Report::Provision(Event::Log(Level::Error, text)));
                log(tr!(
                    "This folder can't be written to: {folder} ({e}).",
                    folder = paths.app_dir.display(),
                    e
                ));
                log(tr!(
                    "MimirDLP keeps its tools, settings and history next to itself. \
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
    fn needs_install(&self) -> bool {
        self.rows
            .iter()
            .any(|row| matches!(row.status, Some(Status::Missing | Status::Broken)))
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// The installed yt-dlp's version, once it answered.
    pub fn ytdlp_version(&self) -> Option<&str> {
        match &self.rows[index(Component::YtDlp)].status {
            Some(Status::Installed { version }) => Some(version),
            _ => None,
        }
    }

    /// How many days old the installed yt-dlp is, when that can be told
    /// and is worth saying: not once a check found nothing newer.
    pub fn ytdlp_age(&self) -> Option<i64> {
        let row = &self.rows[index(Component::YtDlp)];
        if row.update == Some(UpdateCheck::Current) {
            return None;
        }
        match &row.status {
            Some(Status::Installed { version }) => {
                provision::ytdlp_age_days(version, provision::today())
            }
            _ => None,
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if self.busy() && !matches!(message, Message::Report(_)) {
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
                    let log =
                        |level, text: String| send(Report::Provision(Event::Log(level, text)));
                    log(
                        Level::Info,
                        tr!("Checking for updates... (requires internet)"),
                    );
                    for component in targets {
                        let result = provision::check_update(component, &paths);
                        if let UpdateCheck::Failed(reason) = &result {
                            log(
                                Level::Warn,
                                tr!(
                                    "Could not check {component} for updates: {reason}",
                                    component = component.label(),
                                    reason
                                ),
                            );
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
            let log = |level, text: String| send(Report::Provision(Event::Log(level, text)));
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
                },
            );
            if scope != Scope::Outdated {
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

    pub fn view(&self) -> Element<'_, Message> {
        let where_kept = if self.paths.installed {
            tr!(
                "yt-dlp, FFmpeg and Deno are kept in {folder}, and updated from here.",
                folder = self.paths.bin_dir.display()
            )
        } else {
            tr!(
                "yt-dlp, FFmpeg and Deno are kept in the bin folder next to this app. \
                 Nothing is installed on your system."
            )
        };
        let header = text(where_kept).style(style::muted_text);

        let components = Column::with_children(
            Component::ALL
                .into_iter()
                .map(|component| component_row(component, &self.rows[index(component)])),
        )
        .spacing(12);

        let mut page = column![
            header,
            card(tr!("Components"), components.into()),
            self.actions()
        ]
        .spacing(20);
        if let Some(transfer) = &self.transfer {
            page = page.push(transfer_view(transfer));
        }
        page = page.push(
            column![
                text(tr!("Activity")).size(15),
                log_panel::panel(
                    &self.log,
                    tr!("Nothing yet. Each download and check will be listed here."),
                    Length::Fill,
                ),
            ]
            .spacing(8)
            .height(Length::Fill),
        );

        container(page.padding(24).max_width(760))
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into()
    }

    /// The contextual actions available, given what is installed right now.
    fn actions(&self) -> Element<'_, Message> {
        if self.confirm_reinstall {
            return row![
                text(tr!("This will re-download and reinstall ALL components."))
                    .style(text::warning)
                    .width(Length::Fill),
                button(text(tr!("Cancel")))
                    .padding([8, 20])
                    .style(style::pill_secondary)
                    .on_press(Message::CancelReinstall),
                button(text(tr!("Reinstall")))
                    .padding([8, 20])
                    .style(style::pill_danger)
                    .on_press(Message::ConfirmReinstall),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into();
        }

        let any = |f: &dyn Fn(&Row) -> bool| self.rows.iter().any(f);
        let installed = |row: &Row| matches!(row.status, Some(Status::Installed { .. }));
        let has_missing = self.needs_install();
        let has_unchecked =
            any(&|row| installed(row) && matches!(row.update, None | Some(UpdateCheck::Failed(_))));
        let has_updates = any(&|row| installed(row) && row.update == Some(UpdateCheck::Outdated));
        let has_installed = any(&|row| row.status.as_ref().is_some_and(|s| *s != Status::Missing));

        let idle = !self.busy();
        let action = |label: String, message: Message, primary: bool| {
            button(text(label))
                .padding([8, 20])
                .style(if primary {
                    style::pill
                } else {
                    style::pill_secondary
                })
                .on_press_maybe(idle.then_some(message))
        };

        let mut actions = row![].spacing(10).align_y(Alignment::Center);
        if has_missing {
            actions = actions.push(action(
                tr!("Install missing components"),
                Message::InstallMissing,
                true,
            ));
        }
        if has_updates {
            actions = actions.push(action(
                tr!("Update outdated components"),
                Message::UpdateOutdated,
                true,
            ));
        }
        if has_unchecked {
            actions = actions.push(action(
                tr!("Check for updates"),
                Message::CheckUpdates,
                !has_missing && !has_updates,
            ));
        }
        if has_installed {
            actions = actions.push(action(tr!("Reinstall all"), Message::AskReinstall, false));
        }
        if let Some(job) = self.job {
            let busy = match job {
                Job::Inspecting => tr!("Looking at what is installed..."),
                Job::Installing(_) => tr!("Working..."),
                Job::CheckingUpdates => tr!("Checking for updates..."),
            };
            actions = actions.push(text(busy).style(style::muted_text));
        }
        actions.into()
    }
}

fn index(component: Component) -> usize {
    Component::ALL
        .iter()
        .position(|c| *c == component)
        .expect("every component is in ALL")
}

/// Runs `job` on its own thread and turns what it sends into messages:
/// downloading, hashing and extracting never block the window.
fn in_thread(job: impl FnOnce(&dyn Fn(Report)) + Send + 'static) -> Task<Message> {
    let (sender, receiver) = mpsc::unbounded();
    thread::spawn(move || {
        let send = |report| {
            let _ = sender.unbounded_send(report);
        };
        job(&send);
        send(Report::Done);
    });
    Task::run(receiver, Message::Report)
}

/// One line of the component list.
fn component_row(component: Component, row: &Row) -> Element<'static, Message> {
    #[derive(Clone, Copy)]
    enum Tone {
        Good,
        Warn,
        Bad,
        Neutral,
    }
    // A yt-dlp weeks old is the usual reason a site stops working, so its
    // age is shown once it is stale.
    let stale_age = match &row.status {
        Some(Status::Installed { version }) if component == Component::YtDlp => {
            provision::ytdlp_age_days(version, provision::today())
                .filter(|age| *age > provision::YTDLP_STALE_DAYS)
        }
        _ => None,
    };
    let (tone, detail) = match (&row.status, &row.update, stale_age) {
        (None, ..) => (Tone::Neutral, tr!("Checking...")),
        (Some(Status::Missing), ..) => (Tone::Bad, tr!("Not installed")),
        (Some(Status::Broken), ..) => (Tone::Bad, tr!("Installed but not responding")),
        (Some(Status::Installed { version }), None, Some(age)) => (
            Tone::Warn,
            tr!(
                "Version {version} ({n} day old, check for updates)"
                    | "Version {version} ({n} days old, check for updates)" % age,
                version
            ),
        ),
        (Some(Status::Installed { version }), update, _) => match update {
            None => (Tone::Good, tr!("Version {version} (installed)", version)),
            Some(UpdateCheck::Current) => {
                (Tone::Good, tr!("Version {version} (up to date)", version))
            }
            Some(UpdateCheck::Outdated) => (
                Tone::Warn,
                tr!("Version {version} (update available)", version),
            ),
            Some(UpdateCheck::Unknown) => (
                Tone::Warn,
                tr!(
                    "Version {version} (installed before update tracking, reinstall to enable it)",
                    version
                ),
            ),
            Some(UpdateCheck::Failed(_)) => (
                Tone::Warn,
                tr!("Version {version} (update check failed)", version),
            ),
        },
    };
    let mark = match tone {
        Tone::Good => "✓",
        Tone::Warn => "⚠",
        Tone::Bad => "✗",
        Tone::Neutral => "…",
    };
    let color = move |theme: &Theme| {
        let palette = theme.palette();
        text::Style {
            color: Some(match tone {
                Tone::Good => palette.success,
                Tone::Warn => palette.warning,
                Tone::Bad => palette.danger,
                Tone::Neutral => style::muted(theme),
            }),
        }
    };
    row![
        text(mark).width(22).style(color),
        text(component.label()).width(90),
        text(detail).style(move |t: &Theme| match tone {
            Tone::Good | Tone::Neutral => style::muted_text(t),
            _ => color(t),
        }),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn transfer_view(transfer: &Transfer) -> Element<'_, Message> {
    let (amount, fraction) = amount(transfer.received, transfer.total);
    column![
        row![
            text(&transfer.file).font(Font::MONOSPACE).size(13),
            space::horizontal(),
            text(amount).size(13).style(style::muted_text),
        ]
        .align_y(Alignment::Center),
        progress_bar(0.0..=1.0, fraction)
            .girth(8)
            .style(style::progress),
    ]
    .spacing(8)
    .into()
}

/// Debug builds only: `YTP_DEV_ACTION=none` keeps a first launch from
/// installing anything, to look at an empty folder's screens.
fn dev_no_auto_install() -> bool {
    cfg!(debug_assertions) && std::env::var("YTP_DEV_ACTION").is_ok_and(|a| a == "none")
}

/// Debug builds only: `YTP_DEV_ACTION=install|check|update|ask-reinstall|reinstall`
/// runs that action after the first inspection, instead of a click.
fn dev_action() -> Option<Message> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("YTP_DEV_ACTION").ok()?.as_str() {
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
            job: None,
            confirm_reinstall: false,
            log: Vec::new(),
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
            .any(|(_, line)| line.starts_with("Some components are missing"))
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
    fn an_old_yt_dlp_is_reported_until_a_check_finds_nothing_newer() {
        let old = with_ytdlp("2020.01.01", None);
        assert!(old.ytdlp_age().is_some_and(|age| age > 365));
        let checked = with_ytdlp("2020.01.01", Some(UpdateCheck::Current));
        assert_eq!(checked.ytdlp_age(), None);
        let unknown = with_ytdlp("not-a-date", None);
        assert_eq!(unknown.ytdlp_age(), None);
    }
}
