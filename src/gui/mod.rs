//! The window: a GNOME-style header bar (back button, title, menu) and one
//! screen at a time.

mod download;
mod log;
mod settings;
mod setup;
mod style;

use iced::theme::Mode as ThemeMode;
use iced::widget::{
    button, column, container, image, mouse_area, opaque, row, rule, scrollable, space, stack, text,
};
use iced::{Alignment, Element, Font, Length, Subscription, Task, Theme, font, window};
use tr::tr;

use crate::config::{Settings, ThemePref};
use crate::provision::Paths;
use download::Download;
use settings::SettingsPage;
use setup::Setup;

pub fn run() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title("MimirDLP")
        .theme(App::theme)
        .subscription(App::subscription)
        .window(window::Settings {
            size: iced::Size::new(1000.0, 720.0),
            // Below this the Settings rows and the queue cards overlap.
            min_size: Some(iced::Size::new(720.0, 520.0)),
            // Rendered from branding/logo.svg (rsvg-convert -w 256 -h 256).
            icon: window::icon::from_file_data(include_bytes!("../../branding/icon.png"), None)
                .ok(),
            // Closing is handled in `update`, so typed settings are saved
            // and a running download is stopped first.
            exit_on_close_request: false,
            platform_specific: platform_window(),
            ..window::Settings::default()
        })
        .run()
}

/// On Wayland a program can't hand its window an icon: the desktop finds it
/// through the window's application id, in a `.desktop` file of that name
/// (`crate::desktop`, the opt-in menu entry). X11 takes the icon from
/// `window::Settings::icon`.
#[cfg(target_os = "linux")]
fn platform_window() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific {
        application_id: crate::desktop::APP_ID.to_string(),
        ..Default::default()
    }
}

#[cfg(not(target_os = "linux"))]
fn platform_window() -> window::settings::PlatformSpecific {
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
    CheckYtDlp,
    /// A web address from the About page, opened in the browser.
    OpenLink(&'static str),
    CloseRequested,
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
        /// Unknown until the startup query resolves; treated as dark, this
        /// app's original and only appearance before this existed.
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
        // With everything already in bin/, Download is where the app ends
        // up, so it opens there: showing Setup for the half second the
        // first inspection takes was a visible flash on every launch.
        let tools_present = crate::provision::all_present(&paths);
        // A menu entry the user added follows the folder if it was moved.
        // Release builds only: a development build must not repoint it.
        if cfg!(target_os = "linux")
            && !cfg!(debug_assertions)
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
                Message::CloseRequested,
            ) => {
                // Whatever was typed is kept, and a running download is
                // stopped rather than left behind with no window.
                let _ = settings.save();
                download.shutdown();
                iced::exit()
            }
            (App::Broken(_), Message::CloseRequested) => iced::exit(),
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
            (App::Running { download, .. }, Message::OpenLink(url)) => {
                if let Err(e) = download::open(url) {
                    download.log((
                        crate::log::Level::Warn,
                        tr!("Could not open {url}: {e}", url, e),
                    ));
                }
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
                // The same rules the header bar's buttons follow.
                let allowed = match target {
                    Page::Download => (setup.ready() && !setup.busy()) || *confirming_tools,
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
                Message::CheckYtDlp,
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
        // The same rule `Message::Navigate` follows.
        let can_download = (setup.ready() && !setup.busy()) || confirming_tools;
        let can_setup = !download.uses_tools();

        let content = match page {
            Page::Setup => setup.view().map(Message::Setup),
            Page::Download => {
                let screen = download.view().map(Message::Download);
                match setup
                    .ytdlp_age()
                    .filter(|age| *age > crate::provision::YTDLP_STALE_DAYS)
                {
                    Some(age) => column![stale_notice(age, can_setup), screen].into(),
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

        let window =
            column![header_bar(page, can_download, menu_open), content].height(Length::Fill);
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
            container(opaque(main_menu(can_setup, settings.settings())))
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
        let close = window::close_requests().map(|_| Message::CloseRequested);
        match self {
            App::Running { snapshot_due, .. } => {
                let theme = iced::system::theme_changes().map(Message::SystemTheme);
                if *snapshot_due {
                    Subscription::batch([
                        close,
                        theme,
                        window::frames().map(|_| Message::SnapshotFrame),
                    ])
                } else {
                    Subscription::batch([close, theme])
                }
            }
            App::Broken(_) => close,
        }
    }
}

const HEADER_HEIGHT: f32 = 52.0;

/// GNOME's header bar: a back button off the main screen, the title in the
/// middle, and the menu that leads everywhere else.
fn header_bar<'a>(page: Page, can_go_back: bool, menu_open: bool) -> Element<'a, Message> {
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
        Page::Download => "MimirDLP".to_string(),
        Page::Setup => tr!("Setup"),
        Page::Settings => tr!("Settings"),
        Page::About => tr!("About"),
    };
    let side = |content: Element<'a, Message>, align: Alignment| {
        container(content).width(Length::Fixed(80.0)).align_x(align)
    };
    let bar = row![
        side(back, Alignment::Start),
        container(text(title).size(15).font(Font {
            weight: font::Weight::Bold,
            ..Font::DEFAULT
        }))
        .center_x(Length::Fill),
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
fn main_menu<'a>(can_setup: bool, settings: &Settings) -> Element<'a, Message> {
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
            .style(style::nav(theme == target))
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
            rule::horizontal(1).style(|t: &Theme| rule::Style {
                color: style::separator(t),
                ..rule::default(t)
            }),
            language_picker(&settings.language),
            rule::horizontal(1).style(|t: &Theme| rule::Style {
                color: style::separator(t),
                ..rule::default(t)
            }),
            item(tr!("Settings"), Some(Message::Navigate(Page::Settings))),
            item(
                tr!("Setup and updates"),
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

const REPOSITORY: &str = "https://github.com/indep-arg/MimirDLP";
const REPORT: &str = "https://github.com/indep-arg/MimirDLP/issues/new/choose";
const TRANSLATE: &str = "https://hosted.weblate.org/engage/mimirdlp/";

/// What a bug report asks for (this version and yt-dlp's), and where to
/// report, translate or read more. Kept to the essentials, like GNOME's
/// About windows.
fn about<'a>(ytdlp: Option<&str>) -> Element<'a, Message> {
    let link = |label: String, url: &'static str| {
        button(text(label))
            .padding([8, 16])
            .style(style::pill_secondary)
            .on_press(Message::OpenLink(url))
    };
    let muted = |label: String| {
        text(label).style(|t: &Theme| text::Style {
            color: Some(style::muted(t)),
        })
    };
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
        image(image::Handle::from_bytes(
            include_bytes!("../../branding/icon.png").as_slice()
        ))
        .width(Length::Fixed(96.0))
        .height(Length::Fixed(96.0)),
        text("MimirDLP").size(26).font(Font {
            weight: font::Weight::Bold,
            ..Font::DEFAULT
        }),
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
/// so an old one is the usual reason a download suddenly fails.
fn stale_notice<'a>(age: i64, can_update: bool) -> Element<'a, Message> {
    container(
        row![
            text(tr!(
                "yt-dlp is {n} day old. Sites change often; if a download fails, update it first."
                    | "yt-dlp is {n} days old. Sites change often; if a download fails, update it first."
                    % age
            ))
            .size(13)
            .style(text::warning)
            .width(Length::Fill),
            button(text(tr!("Check for updates")).size(13))
                .padding([4, 14])
                .style(style::pill_secondary)
                .on_press_maybe(can_update.then_some(Message::CheckYtDlp)),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding([10, 24])
    .into()
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

/// Debug builds only: `YTP_DEV_PAGE=setup|download|settings|about` opens directly
/// on that page, so a screen's layout can be checked without installing
/// anything or navigating there by hand. Takes priority over `YTP_DEV_DOWNLOAD`'s
/// own page choice when both are set.
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
    Task::perform(wait_on_a_thread(after), |_| Message::SnapshotDue)
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
    Task::perform(wait_on_a_thread(after), |_| {
        Message::Download(download::Message::Stop)
    })
}

/// Waits without blocking the executor: a `thread::sleep` inside a future
/// stalls every task batched with it, the worker threads included.
fn wait_on_a_thread(millis: u64) -> impl std::future::Future<Output = ()> {
    let (done, wait) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(millis));
        let _ = done.send(());
    });
    async move {
        let _ = wait.await;
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
