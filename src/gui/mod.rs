//! The window: a GNOME-style header bar (back button, title, menu) and one
//! screen at a time.

mod download;
mod log;
mod open;
mod settings;
mod setup;
mod style;

use iced::theme::Mode as ThemeMode;
use iced::widget::{
    button, column, container, image, mouse_area, opaque, row, rule, scrollable, space, stack, text,
};
use iced::{Alignment, Element, Length, Subscription, Task, Theme, window};
use tr::tr;

use crate::config::{Settings, ThemePref, WindowState};
use crate::provision::Paths;
use download::Download;
use settings::SettingsPage;
use setup::Setup;

/// Below this the Settings rows and the queue cards overlap.
const MIN_SIZE: iced::Size = iced::Size::new(720.0, 520.0);

pub fn run() -> iced::Result {
    // The window opens as it was closed. Only Windows gets its place back
    // (see `remember_window`); on Linux the desktop chooses it.
    let saved = Paths::for_current_exe()
        .ok()
        .and_then(|paths| WindowState::load(&paths));
    let (width, height) = saved.map_or(WindowState::DEFAULT_SIZE, |w| (w.width, w.height));
    let size = iced::Size::new(width.max(MIN_SIZE.width), height.max(MIN_SIZE.height));
    let position = match saved.and_then(|w| w.position) {
        Some((x, y)) => window::Position::Specific(iced::Point::new(x, y)),
        None => window::Position::Default,
    };
    iced::application(App::boot, App::update, App::view)
        .title("MimirDLP")
        .theme(App::theme)
        .subscription(App::subscription)
        .window(window::Settings {
            size,
            position,
            maximized: saved.is_some_and(|w| w.maximized),
            min_size: Some(MIN_SIZE),
            // Rendered from branding/logo.svg (rsvg-convert -w 256 -h 256).
            icon: window::icon::from_file_data(include_bytes!("../../branding/icon.png"), None)
                .ok(),
            // Closing is handled in `update`, so typed settings are saved
            // and a running download is stopped first.
            exit_on_close_request: false,
            platform_specific: platform_settings(),
            ..window::Settings::default()
        })
        .run()
}

/// On Wayland a program can't hand its window an icon: the desktop finds it
/// through the window's application id, in a `.desktop` file of that name
/// (`crate::desktop`, the opt-in menu entry). X11 takes the icon from
/// `window::Settings::icon`.
#[cfg(target_os = "linux")]
fn platform_settings() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific {
        application_id: crate::desktop::APP_ID.to_string(),
        ..Default::default()
    }
}

#[cfg(not(target_os = "linux"))]
fn platform_settings() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific::default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Download,
    Setup,
    Settings,
    About,
}

#[derive(Debug, Clone)]
enum Message {
    Navigate(Page),
    /// The header bar's menu button, and a click anywhere outside the menu.
    ToggleMenu,
    CloseMenu,
    Setup(setup::Message),
    Download(download::Message),
    Settings(settings::Message),
    /// The OS's current light/dark mode, at startup and whenever it changes.
    SystemTheme(ThemeMode),
    ThemePref(ThemePref),
    /// A code from `i18n::LANGUAGES`, or empty for the system's language.
    Language(&'static str),
    /// From the "yt-dlp is old" notice: go to Setup and check for updates.
    CheckForUpdates,
    /// A web address from the About page, opened in the browser.
    OpenLink(&'static str),
    LinkOpened(&'static str, Result<(), String>),
    CloseRequested(window::Id),
    /// The window got the focus: a link copied meanwhile may be offered.
    WindowFocused,
    /// Debug builds only, see [`dev_snapshot`].
    SnapshotDue,
    SnapshotFrame,
    Snapshot(window::Screenshot),
}

enum App {
    /// The folder next to the executable couldn't be determined.
    Broken(String),
    Running {
        page: Page,
        setup: Box<Setup>,
        download: Box<Download>,
        settings: Box<SettingsPage>,
        menu_open: bool,
        /// The app opens on Setup and moves on to Download by itself once
        /// everything is installed, unless the user went somewhere first.
        leave_setup_when_ready: bool,
        /// The app opened on Download because every tool's file is there,
        /// before the first inspection said whether they run. A broken one
        /// sends it to Setup instead, once that inspection ends.
        confirming_tools: bool,
        /// Download was pressed while that inspection still ran; it starts
        /// as soon as the tools are known to work.
        start_when_ready: bool,
        /// `None` until the startup query answers.
        system_theme: ThemeMode,
        /// Debug builds only: the state is frozen until the capture is taken.
        snapshot_due: bool,
    },
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        let paths = match Paths::for_current_exe() {
            Ok(paths) => paths,
            Err(e) => {
                crate::i18n::apply("");
                let reason = tr!("Could not find the folder this app is in: {e}", e);
                return (App::Broken(reason), Task::none());
            }
        };
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
        let saved = Settings::load(&paths);
        // Before anything is shown or logged, so all of it is translated.
        crate::i18n::apply(&saved.language);
        let settings = SettingsPage::new(paths.clone(), saved);
        let (setup, task) = Setup::new(paths.clone());
        let mut download = Download::new(paths);
        let mut tasks = vec![
            task.map(Message::Setup),
            download.restore().map(Message::Download),
            dev_snapshot(),
            iced::system::theme().map(Message::SystemTheme),
        ];
        // Links handed over for a dev run open on the Download screen,
        // queue every one of them (in order), and start right away.
        let dev_urls = download::dev_urls();
        let page = if !dev_urls.is_empty() {
            for url in dev_urls {
                tasks.push(Task::done(Message::Download(download::Message::UrlInput(
                    url,
                ))));
                tasks.push(Task::done(Message::Download(download::Message::AddUrl)));
            }
            tasks.push(Task::done(Message::Download(download::Message::Start)));
            tasks.push(dev_stop());
            Page::Download
        } else if tools_present {
            Page::Download
        } else {
            Page::Setup
        };
        let dev_page = dev_page();
        let leave_setup_when_ready = page == Page::Setup && dev_page.is_none();
        let page = dev_page.unwrap_or(page);
        let app = App::Running {
            page,
            setup: Box::new(setup),
            download: Box::new(download),
            settings: Box::new(settings),
            menu_open: dev_menu(),
            leave_setup_when_ready,
            confirming_tools: page == Page::Download,
            start_when_ready: false,
            system_theme: ThemeMode::None,
            snapshot_due: false,
        };
        (app, Task::batch(tasks))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match (self, message) {
            (_, Message::Snapshot(shot)) => save_snapshot(shot),
            (
                App::Running {
                    download, settings, ..
                },
                Message::CloseRequested(id),
            ) => {
                // Whatever was typed is kept, and a running download is
                // stopped rather than left behind with no window.
                let _ = settings.save();
                download.shutdown();
                let paths = settings.paths().clone();
                remember_window(id, paths).then(|()| iced::exit())
            }
            (App::Broken(_), Message::CloseRequested(_)) => iced::exit(),
            (App::Running { snapshot_due, .. }, Message::SnapshotDue) => {
                *snapshot_due = true;
                Task::none()
            }
            (App::Running { .. }, Message::SnapshotFrame) => window::latest().then(|id| match id {
                Some(id) => window::screenshot(id).map(Message::Snapshot),
                None => Task::none(),
            }),
            (
                App::Running {
                    snapshot_due: true, ..
                },
                _,
            ) => Task::none(),
            (App::Running { menu_open, .. }, Message::ToggleMenu) => {
                *menu_open = !*menu_open;
                Task::none()
            }
            (App::Running { menu_open, .. }, Message::CloseMenu) => {
                *menu_open = false;
                Task::none()
            }
            (App::Running { download, .. }, Message::WindowFocused) => {
                if download.wants_clipboard() {
                    iced::clipboard::read()
                        .map(|text| Message::Download(download::Message::Clipboard(text)))
                } else {
                    Task::none()
                }
            }
            (App::Broken(_), Message::WindowFocused) => Task::none(),
            (App::Running { .. }, Message::OpenLink(url)) => {
                open::open(url).map(move |result| Message::LinkOpened(url, result))
            }
            (_, Message::LinkOpened(_, Ok(()))) => Task::none(),
            (App::Running { download, .. }, Message::LinkOpened(url, Err(e))) => {
                download.log((
                    crate::log::Level::Warn,
                    tr!("Could not open {url}: {e}", url, e),
                ));
                Task::none()
            }
            (
                App::Running {
                    page,
                    setup,
                    download,
                    settings,
                    menu_open,
                    leave_setup_when_ready,
                    confirming_tools,
                    ..
                },
                Message::Navigate(target),
            ) => {
                *menu_open = false;
                *leave_setup_when_ready = false;
                let allowed = match target {
                    Page::Download => can_download(setup, *confirming_tools),
                    Page::Setup => !download.uses_tools(),
                    Page::Settings | Page::About => true,
                };
                if allowed {
                    // Typed values are saved on leaving the page.
                    if *page == Page::Settings
                        && let Some(note) = settings.save()
                    {
                        download.log(note);
                    }
                    *page = target;
                }
                Task::none()
            }
            (
                App::Running {
                    page,
                    setup,
                    download,
                    leave_setup_when_ready,
                    ..
                },
                Message::CheckForUpdates,
            ) => {
                if download.uses_tools() || setup.busy() {
                    return Task::none();
                }
                *leave_setup_when_ready = false;
                *page = Page::Setup;
                Task::done(Message::Setup(setup::Message::CheckUpdates))
            }
            (
                App::Running {
                    page,
                    setup,
                    leave_setup_when_ready,
                    confirming_tools,
                    start_when_ready,
                    ..
                },
                Message::Setup(message),
            ) => {
                let task = setup.update(message).map(Message::Setup);
                if setup.busy() {
                    return task;
                }
                if *confirming_tools {
                    *confirming_tools = false;
                    if !setup.ready() && *page == Page::Download {
                        // A file is there but doesn't run: Setup repairs
                        // it by itself, then comes back here.
                        *page = Page::Setup;
                        *leave_setup_when_ready = true;
                    }
                }
                if *leave_setup_when_ready && setup.ready() {
                    *leave_setup_when_ready = false;
                    if *page == Page::Setup {
                        *page = Page::Download;
                    }
                }
                if *start_when_ready {
                    *start_when_ready = false;
                    if setup.ready() {
                        return Task::batch([
                            task,
                            Task::done(Message::Download(download::Message::Start)),
                        ]);
                    }
                }
                task
            }
            (
                App::Running {
                    download,
                    settings,
                    setup,
                    start_when_ready,
                    ..
                },
                Message::Download(message),
            ) => {
                // The tools must be known to work before yt-dlp is started.
                // Pressed during the first inspection, the start waits for it.
                if matches!(message, download::Message::Start) && !setup.ready() {
                    *start_when_ready = setup.busy();
                    return Task::none();
                }
                // A download starts with what the Settings page says now,
                // typed values included, so write them first.
                if matches!(message, download::Message::Start)
                    && let Some(note) = settings.save()
                {
                    download.log(note);
                }
                let finished = matches!(
                    message,
                    download::Message::Report(crate::runner::Event::Finished(_))
                );
                let task = download.update(message, settings.settings());
                // A finished run may have added to the history file.
                if finished {
                    settings.refresh_history();
                }
                task.map(Message::Download)
            }
            (
                App::Running {
                    download, settings, ..
                },
                Message::Settings(message),
            ) => {
                if matches!(message, settings::Message::ChooseFolder) {
                    return settings.choose_folder().map(Message::Settings);
                }
                if let Some(note) = settings.update(message) {
                    download.log(note);
                }
                Task::none()
            }
            (App::Running { system_theme, .. }, Message::SystemTheme(mode)) => {
                *system_theme = mode;
                Task::none()
            }
            (
                App::Running {
                    download, settings, ..
                },
                Message::ThemePref(pref),
            ) => {
                // The menu stays open, so the other choices can be tried.
                if let Some(note) = settings.set_theme(pref) {
                    download.log(note);
                }
                Task::none()
            }
            (
                App::Running {
                    download, settings, ..
                },
                Message::Language(code),
            ) => {
                if let Some(note) = settings.set_language(code) {
                    download.log(note);
                }
                Task::none()
            }
            (App::Broken(_), _) => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let (page, setup, download, settings, menu_open, confirming_tools) = match self {
            App::Broken(reason) => {
                return container(text(reason).style(text::danger))
                    .padding(24)
                    .into();
            }
            App::Running {
                page,
                setup,
                download,
                settings,
                menu_open,
                confirming_tools,
                ..
            } => (
                *page,
                setup,
                download,
                settings,
                *menu_open,
                *confirming_tools,
            ),
        };
        let can_download = can_download(setup, confirming_tools);
        let can_setup = !download.uses_tools();

        let content = match page {
            Page::Setup => setup.view().map(Message::Setup),
            Page::Download => {
                let screen = download.view().map(Message::Download);
                match setup.ytdlp_age() {
                    Some(age) => {
                        column![stale_notice(age, can_setup, setup.bundled()), screen].into()
                    }
                    None => screen,
                }
            }
            Page::Settings => scrollable(
                container(
                    container(settings.view().map(Message::Settings))
                        .padding(24)
                        .max_width(760),
                )
                .center_x(Length::Fill),
            )
            .height(Length::Fill)
            .into(),
            Page::About => about(setup.ytdlp_version()),
        };

        let window = column![
            header_bar(page, can_download, menu_open, setup.bundled()),
            content
        ]
        .height(Length::Fill);
        if !menu_open {
            return window.into();
        }
        stack![
            window,
            // A click anywhere outside the menu closes it, and reaches
            // nothing underneath.
            opaque(
                mouse_area(space().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::CloseMenu)
            ),
            container(opaque(main_menu(
                can_setup,
                setup.bundled(),
                settings.settings()
            )))
            .padding([HEADER_HEIGHT, 8.0])
            .align_right(Length::Fill),
        ]
        .into()
    }

    fn theme(&self) -> Theme {
        match self {
            App::Running {
                settings,
                system_theme,
                ..
            } => style::theme(settings.settings().theme, *system_theme),
            App::Broken(_) => style::theme(ThemePref::System, ThemeMode::None),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let close = window::close_requests().map(Message::CloseRequested);
        match self {
            App::Running { snapshot_due, .. } => {
                let theme = iced::system::theme_changes().map(Message::SystemTheme);
                let focus = iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Window(window::Event::Focused) => Some(Message::WindowFocused),
                    _ => None,
                });
                if *snapshot_due {
                    Subscription::batch([
                        close,
                        theme,
                        focus,
                        window::frames().map(|_| Message::SnapshotFrame),
                    ])
                } else {
                    Subscription::batch([close, theme, focus])
                }
            }
            App::Broken(_) => close,
        }
    }
}

/// Saves the window's size and place for the next launch. If that fails
/// the window still closes.
fn remember_window(id: window::Id, paths: Paths) -> Task<()> {
    window::size(id)
        .then(move |size| window::position(id).map(move |position| (size, position)))
        .then(move |(size, position)| {
            window::is_maximized(id).map(move |maximized| (size, position, maximized))
        })
        .then(move |(size, position, maximized)| {
            window::is_minimized(id)
                .map(move |minimized| (size, position, maximized, minimized == Some(true)))
        })
        .map(move |(size, position, maximized, minimized)| {
            // Only Windows reads and sets the place the same way. On X11,
            // winit reports where the inside of the window is but puts the
            // frame there, so the window crept down by the height of its
            // title bar on every launch (seen on KDE). Wayland tells nothing.
            let position = position.filter(|_| cfg!(windows));
            let state = WindowState::when_closed(
                WindowState::load(&paths),
                (size.width, size.height),
                position.map(|p| (p.x, p.y)),
                maximized,
                minimized,
            );
            if let Some(state) = state {
                let _ = state.save(&paths);
            }
        })
}

/// Download can be shown once the tools work, or while the first
/// inspection confirms the ones that are there.
fn can_download(setup: &Setup, confirming_tools: bool) -> bool {
    (setup.ready() && !setup.busy()) || confirming_tools
}

const HEADER_HEIGHT: f32 = 52.0;

/// GNOME's header bar: a back button off the main screen, the title in the
/// middle, and the menu that leads everywhere else.
/// With the tools bundled, Setup only lists them, so it is "Components".
fn header_bar<'a>(
    page: Page,
    can_go_back: bool,
    menu_open: bool,
    bundled: bool,
) -> Element<'a, Message> {
    let back: Element<'a, Message> = if page == Page::Download {
        space().into()
    } else {
        button(text("\u{2039}").size(22).line_height(1.0))
            .padding([4, 14])
            .style(style::flat(false))
            .on_press_maybe(can_go_back.then_some(Message::Navigate(Page::Download)))
            .into()
    };
    let title = match page {
        // The window's own title bar already says it.
        Page::Download => String::new(),
        Page::Setup if bundled => tr!("Components"),
        Page::Setup => tr!("Setup"),
        Page::Settings => tr!("Settings"),
        Page::About => tr!("About"),
    };
    let side = |content: Element<'a, Message>, align: Alignment| {
        container(content).width(Length::Fixed(80.0)).align_x(align)
    };
    let bar = row![
        side(back, Alignment::Start),
        container(text(title).size(15).font(style::BOLD)).center_x(Length::Fill),
        side(menu_button(menu_open), Alignment::End),
    ]
    .padding([0, 8])
    .height(Length::Fixed(HEADER_HEIGHT))
    .align_y(Alignment::Center);
    container(bar)
        .width(Length::Fill)
        .style(style::header_bar)
        .into()
}

/// Three bars drawn as boxes rather than a glyph, since not every font has
/// one for it.
fn menu_button<'a>(open: bool) -> Element<'a, Message> {
    let bar = || {
        container(space())
            .width(Length::Fixed(16.0))
            .height(Length::Fixed(2.0))
            .style(|t: &Theme| container::Style {
                background: Some(t.palette().text.into()),
                border: iced::border::rounded(1),
                ..container::Style::default()
            })
    };
    button(column![bar(), bar(), bar()].spacing(3))
        .padding([10, 12])
        .style(style::flat(open))
        .on_press(Message::ToggleMenu)
        .into()
}

/// The header bar's menu: the screens besides Download, the theme and the
/// language.
fn main_menu<'a>(can_setup: bool, bundled: bool, settings: &Settings) -> Element<'a, Message> {
    let theme = settings.theme;
    let item = |label: String, message: Option<Message>| {
        button(text(label))
            .width(Length::Fill)
            .padding([8, 12])
            .style(style::flat(false))
            .on_press_maybe(message)
    };
    let theme_choice = |label: String, target: ThemePref| {
        button(text(label).size(13).center().width(Length::Fill))
            .width(Length::Fill)
            .padding([6, 8])
            .style(style::segment(theme == target))
            .on_press(Message::ThemePref(target))
    };
    container(
        column![
            row![
                theme_choice(tr!("System"), ThemePref::System),
                theme_choice(tr!("Light"), ThemePref::Light),
                theme_choice(tr!("Dark"), ThemePref::Dark),
            ]
            .spacing(4)
            .padding([4, 4]),
            rule::horizontal(1).style(style::separator),
            language_picker(&settings.language),
            rule::horizontal(1).style(style::separator),
            item(tr!("Settings"), Some(Message::Navigate(Page::Settings))),
            item(
                if bundled {
                    tr!("Components")
                } else {
                    tr!("Setup and updates")
                },
                can_setup.then_some(Message::Navigate(Page::Setup))
            ),
            item(tr!("About MimirDLP"), Some(Message::Navigate(Page::About))),
        ]
        .spacing(4),
    )
    .padding(6)
    .width(Length::Fixed(280.0))
    .style(style::popover)
    .into()
}

/// Made once: a handle made in `view` is a new image on every redraw, and
/// the logo blinked while it was loaded again.
static LOGO: std::sync::LazyLock<image::Handle> = std::sync::LazyLock::new(|| {
    image::Handle::from_bytes(include_bytes!("../../branding/icon.png").as_slice())
});

const REPOSITORY: &str = "https://github.com/indep-arg/MimirDLP";
const REPORT: &str = "https://github.com/indep-arg/MimirDLP/issues/new/choose";
const TRANSLATE: &str = "https://hosted.weblate.org/engage/mimirdlp/";

/// The versions a bug report asks for, and where to report or translate.
fn about<'a>(ytdlp: Option<&str>) -> Element<'a, Message> {
    let link = |label: String, url: &'static str| {
        button(text(label))
            .padding([8, 16])
            .style(style::pill_secondary)
            .on_press(Message::OpenLink(url))
    };
    let muted = |label: String| text(label).style(style::muted_text);
    let mut info = column![
        text(tr!(
            "Version {version}",
            version = env!("CARGO_PKG_VERSION")
        ))
        .size(15),
    ]
    .spacing(4)
    .align_x(Alignment::Center);
    if let Some(version) = ytdlp {
        info = info.push(muted(tr!("Using yt-dlp {version}", version)));
    }
    let content = column![
        image(LOGO.clone())
            .width(Length::Fixed(96.0))
            .height(Length::Fixed(96.0)),
        text("MimirDLP").size(26).font(style::BOLD),
        info,
        text(tr!(
            "Download videos and music from YouTube and many other sites."
        ))
        .center(),
        column![
            link(tr!("Website"), REPOSITORY),
            link(tr!("Report a problem"), REPORT),
            link(tr!("Help translate"), TRANSLATE),
        ]
        .spacing(8)
        .align_x(Alignment::Center),
        muted(tr!("Free software under the MIT license.")),
    ]
    .spacing(16)
    .padding(24)
    .max_width(560)
    .align_x(Alignment::Center);
    scrollable(container(content).center_x(Length::Fill))
        .height(Length::Fill)
        .into()
}

/// One entry of the language picker: the system's language, or one of the
/// translations by its own name (so it can be found by someone who can't
/// read the current one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LanguageChoice(&'static str);

impl std::fmt::Display for LanguageChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match crate::i18n::LANGUAGES.iter().find(|l| l.code == self.0) {
            Some(language) => f.write_str(language.name),
            None => f.write_str(&tr!("System language")),
        }
    }
}

fn language_picker<'a>(current: &str) -> Element<'a, Message> {
    let mut choices = vec![LanguageChoice("")];
    choices.extend(
        crate::i18n::LANGUAGES
            .iter()
            .map(|l| LanguageChoice(l.code)),
    );
    let selected = choices.iter().copied().find(|c| c.0 == current);
    container(
        dropdown(choices, selected, |choice: LanguageChoice| {
            Message::Language(choice.0)
        })
        .width(Length::Fill),
    )
    .padding([0, 4])
    .into()
}

/// Sites change their pages all the time and yt-dlp follows within days,
/// so an old one is the usual reason a download suddenly fails. A bundled
/// one comes with the package, so the way out is updating MimirDLP itself,
/// and there is no button for that here.
fn stale_notice<'a>(age: i64, can_update: bool, bundled: bool) -> Element<'a, Message> {
    let warning = if bundled {
        tr!(
            "yt-dlp is {n} day old. Sites change often; if a download fails, update MimirDLP from your software center or with flatpak update."
                | "yt-dlp is {n} days old. Sites change often; if a download fails, update MimirDLP from your software center or with flatpak update."
                    % age
        )
    } else {
        tr!(
            "yt-dlp is {n} day old. Sites change often; if a download fails, update it first."
                | "yt-dlp is {n} days old. Sites change often; if a download fails, update it first."
                    % age
        )
    };
    let notice = row![
        text(warning)
            .size(13)
            .style(text::warning)
            .width(Length::Fill)
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    let notice = if bundled {
        notice
    } else {
        notice.push(
            button(text(tr!("Check for updates")).size(13))
                .padding([4, 14])
                .style(style::pill_secondary)
                .on_press_maybe(can_update.then_some(Message::CheckForUpdates)),
        )
    };
    container(notice).padding([10, 24]).into()
}

/// A titled card, the building block of every screen.
fn card<'a, Message: 'a>(title: String, body: Element<'a, Message>) -> Element<'a, Message> {
    column![
        text(title).size(15),
        container(body)
            .padding(16)
            .width(Length::Fill)
            .style(style::card)
    ]
    .spacing(8)
    .into()
}

/// iced's `pick_list` in Adwaita's dropdown style. Every dropdown is made
/// with this, so they all look alike.
fn dropdown<'a, T, L, V, Message>(
    options: L,
    selected: Option<V>,
    on_selected: impl Fn(T) -> Message + 'a,
) -> iced::widget::PickList<'a, T, L, V, Message>
where
    T: ToString + PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
    Message: Clone,
{
    iced::widget::pick_list(options, selected, on_selected)
        .padding([6, 12])
        .style(style::dropdown)
        .menu_style(style::dropdown_menu)
}

/// iced's `toggler` in Adwaita's switch style, likewise for every switch.
fn switch<'a, Message: 'a>(is_on: bool) -> iced::widget::Toggler<'a, Message> {
    iced::widget::toggler(is_on).style(style::switch)
}

/// iced's `text_input` in Adwaita's entry style, likewise for every field.
fn entry<'a, Message: Clone + 'a>(
    placeholder: &str,
    value: &str,
) -> iced::widget::TextInput<'a, Message> {
    iced::widget::text_input(placeholder, value)
        .padding([7, 12])
        .style(style::entry)
}

/// A labelled control, with the control pushed to the right. Shared by the
/// Settings page's sections.
fn choice<'a, Message: 'a>(label: String, control: Element<'a, Message>) -> Element<'a, Message> {
    row![text(label).width(Length::Fill), control]
        .align_y(Alignment::Center)
        .into()
}

/// Debug builds only: `YTP_DEV_PAGE=setup|download|settings|about` opens on
/// that page, even with `YTP_DEV_DOWNLOAD` set.
fn dev_page() -> Option<Page> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("YTP_DEV_PAGE").ok()?.as_str() {
        "setup" => Some(Page::Setup),
        "download" => Some(Page::Download),
        "settings" => Some(Page::Settings),
        "about" => Some(Page::About),
        _ => None,
    }
}

/// Debug builds only: `YTP_DEV_MENU=1` opens with the header bar's menu
/// open, so it can be snapshotted.
fn dev_menu() -> bool {
    cfg!(debug_assertions) && std::env::var_os("YTP_DEV_MENU").is_some()
}

/// Debug builds only: `YTP_DEV_SNAPSHOT=out.rgba` captures the window after
/// `YTP_DEV_SNAPSHOT_AFTER` milliseconds (default 1500), writes the raw
/// RGBA pixels plus `out.rgba.size`, and quits. Used to check the UI.
///
/// A screenshot re-renders the last drawn frame, whose text belongs to the
/// widgets of that frame. Capturing right after an update, before the next
/// redraw, therefore loses text. So the state is frozen first, and the
/// capture waits for the next drawn frame.
fn dev_snapshot() -> Task<Message> {
    if !cfg!(debug_assertions) || std::env::var_os("YTP_DEV_SNAPSHOT").is_none() {
        return Task::none();
    }
    let after = std::env::var("YTP_DEV_SNAPSHOT_AFTER")
        .ok()
        .and_then(|ms| ms.parse().ok())
        .unwrap_or(1500);
    Task::perform(delay(after), |_| Message::SnapshotDue)
}

/// Debug builds only: `YTP_DEV_STOP_AFTER=<ms>` presses Stop on its own,
/// to check that cancelling a real download works.
fn dev_stop() -> Task<Message> {
    let after = match std::env::var("YTP_DEV_STOP_AFTER")
        .ok()
        .map(|ms| ms.parse())
    {
        Some(Ok(ms)) if cfg!(debug_assertions) => ms,
        _ => return Task::none(),
    };
    Task::perform(delay(after), |_| Message::Download(download::Message::Stop))
}

/// Runs blocking work (a lookup, a dialog, a sleep) on a thread of its own
/// and resolves with its result, `None` if the thread died. Blocking inside
/// a future instead stalls every task batched with it.
fn in_background<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> impl std::future::Future<Output = Option<T>> {
    let (done, result) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = done.send(work());
    });
    async move { result.await.ok() }
}

fn delay(millis: u64) -> impl std::future::Future<Output = Option<()>> {
    in_background(move || std::thread::sleep(std::time::Duration::from_millis(millis)))
}

/// "12.3 / 45.6 MB  (27%)" and the fraction for a progress bar, or just
/// the amount when the total isn't known.
fn amount(done: u64, total: Option<u64>) -> (String, f32) {
    let mb = |bytes: u64| bytes as f64 / 1_000_000.0;
    match total {
        Some(total) if total > 0 => {
            let fraction = (done as f64 / total as f64).min(1.0);
            (
                format!(
                    "{:.1} / {:.1} MB  ({:.0}%)",
                    mb(done),
                    mb(total),
                    fraction * 100.0
                ),
                fraction as f32,
            )
        }
        _ => (format!("{:.1} MB", mb(done)), 0.0),
    }
}

fn save_snapshot(shot: window::Screenshot) -> Task<Message> {
    if let Some(out) = std::env::var_os("YTP_DEV_SNAPSHOT") {
        let size = format!("{}x{}", shot.size.width, shot.size.height);
        let mut size_file = out.clone();
        size_file.push(".size");
        let _ = std::fs::write(&out, &shot.rgba);
        let _ = std::fs::write(size_file, size);
    }
    iced::exit()
}

#[cfg(test)]
mod tests {
    /// `run` drops a failed icon with `.ok()`, so make sure it decodes.
    #[test]
    fn the_window_icon_decodes() {
        assert!(
            iced::window::icon::from_file_data(include_bytes!("../../branding/icon.png"), None)
                .is_ok()
        );
    }
}
