//! The Settings page and the settings themselves.
//!
//! `SettingsPage` owns [`Settings`], saves it, and keeps the history
//! file's live state (count, entry list, the two step clear). Its values
//! apply to links queued from then on: the Download screen copies them
//! into each item when it is added, so an item keeps what it was queued
//! with. `config::Settings`, `Settings::to_options` and `Settings::problems`
//! hold the model and its rules; this module only edits and shows it.

use iced::widget::{Column, button, column, container, radio, row, rule, scrollable, text};
use iced::{Alignment, Element, Font, Length, Task};
use tr::tr;

use super::{card, choice, dropdown, entry, in_background, style, switch};
use crate::config::{
    Audio, Browser, Container, Convert, Kind, PlaylistChoice, Quality, Segments, Settings, Sponsor,
    Subs, Template, ThemePref, Thumb, VideoFormat,
};
use crate::log::Level;
use crate::provision::Paths;

/// A line for the activity log, which the Download screen owns.
pub type Note = (Level, String);

#[derive(Debug, Clone)]
pub enum Message {
    Text(Field, String),
    Toggle(Flag, bool),
    Kind(Kind),
    VideoFormat(VideoFormat),
    Quality(Quality),
    Audio(Audio),
    AudioQuality(u8),
    Container(Container),
    Convert(Convert),
    ThumbFormat(Thumb),
    Subs(Subs),
    Sponsor(Sponsor),
    Segments(Segments),
    Playlist(PlaylistChoice),
    Template(Template),
    Browser(Browser),
    ClearHistory,
    ConfirmClearHistory,
    CancelClearHistory,
    ToggleHistoryList,
    /// Shows or hides the sections most people never need.
    ToggleAdvanced,
    /// Linux: adds or removes the entry in the desktop's application menu.
    DesktopEntry(bool),
    /// Opens the system's folder dialog; handled by [`SettingsPage::choose_folder`].
    ChooseFolder,
    /// What the dialog returned; `None` when it was cancelled.
    FolderChosen(Option<std::path::PathBuf>),
}

/// The text fields. Typed values are saved when leaving the page, when a
/// download starts and when the window closes, not on every keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    OutputDir,
    CustomFormat,
    ConvertTarget,
    SubLangs,
    CustomSegments,
    PlaylistItems,
    ArchiveFile,
    MaxDownloads,
    CustomTemplate,
    LimitRate,
    Fragments,
    Sleep,
    WaitForVideo,
    CookiesProfile,
}

impl Field {
    fn slot(self, s: &mut Settings) -> &mut String {
        match self {
            Field::OutputDir => &mut s.output_dir,
            Field::CustomFormat => &mut s.custom_format,
            Field::ConvertTarget => &mut s.convert_target,
            Field::SubLangs => &mut s.sub_langs,
            Field::CustomSegments => &mut s.custom_segments,
            Field::PlaylistItems => &mut s.playlist_items,
            Field::ArchiveFile => &mut s.archive_file,
            Field::MaxDownloads => &mut s.max_downloads,
            Field::CustomTemplate => &mut s.custom_template,
            Field::LimitRate => &mut s.limit_rate,
            Field::Fragments => &mut s.fragments,
            Field::Sleep => &mut s.sleep,
            Field::WaitForVideo => &mut s.wait_for_video,
            Field::CookiesProfile => &mut s.cookies_profile,
        }
    }
}

/// The on/off settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    AudioConvert,
    Thumbnail,
    Metadata,
    Chapters,
    AutoSubs,
    Keyframes,
    PlaylistReverse,
    Organize,
    Archive,
    ArchiveStop,
    LiveFromStart,
    Verbose,
    Restrict,
    Mtime,
    IgnoreErrors,
}

impl Flag {
    fn slot(self, s: &mut Settings) -> &mut bool {
        match self {
            Flag::AudioConvert => &mut s.audio_convert,
            Flag::Thumbnail => &mut s.thumbnail,
            Flag::Metadata => &mut s.metadata,
            Flag::Chapters => &mut s.chapters,
            Flag::AutoSubs => &mut s.auto_subs,
            Flag::Keyframes => &mut s.keyframes,
            Flag::PlaylistReverse => &mut s.playlist_reverse,
            Flag::Organize => &mut s.organize,
            Flag::Archive => &mut s.archive,
            Flag::ArchiveStop => &mut s.archive_stop,
            Flag::LiveFromStart => &mut s.live_from_start,
            Flag::Verbose => &mut s.verbose,
            Flag::Restrict => &mut s.restrict_filenames,
            Flag::Mtime => &mut s.preserve_mtime,
            Flag::IgnoreErrors => &mut s.ignore_errors,
        }
    }
}

pub struct SettingsPage {
    paths: Paths,
    settings: Settings,
    /// How many videos the history holds, when it has been looked at.
    history: Option<usize>,
    /// The history's own entries, shown one by one while this is `Some`.
    history_list: Option<Vec<String>>,
    /// Waiting for the history file to be cleared on purpose.
    clearing_history: bool,
    /// The less common sections are folded away until asked for. Not
    /// saved: every launch starts with the short page.
    show_advanced: bool,
    /// Linux: the application menu has an entry for MimirDLP (see
    /// `crate::desktop`). Read from disk, not saved in the settings.
    desktop_entry: bool,
}

impl SettingsPage {
    pub fn new(paths: Paths, settings: Settings) -> Self {
        let mut page = SettingsPage {
            paths,
            settings,
            history: None,
            history_list: None,
            clearing_history: false,
            show_advanced: false,
            desktop_entry: crate::desktop::data_home()
                .is_some_and(|data| crate::desktop::installed(&data)),
        };
        page.refresh_history();
        page
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// An app-wide preference kept in the same file, set from the top bar.
    pub fn set_theme(&mut self, pref: ThemePref) -> Option<Note> {
        self.settings.theme = pref;
        self.save()
    }

    fn set_desktop_entry(&mut self, wanted: bool) -> Option<Note> {
        let Some(data) = crate::desktop::data_home() else {
            return Some((Level::Warn, tr!("Could not find the home folder.")));
        };
        let result = if wanted {
            std::env::current_exe().and_then(|exe| crate::desktop::install(&data, &exe))
        } else {
            crate::desktop::remove(&data)
        };
        self.desktop_entry = crate::desktop::installed(&data);
        match result {
            Ok(()) if wanted => Some((Level::Ok, tr!("MimirDLP is now in the applications menu."))),
            Ok(()) => Some((
                Level::Info,
                tr!("MimirDLP was removed from the applications menu."),
            )),
            Err(e) => Some((
                Level::Warn,
                tr!("Could not change the applications menu: {e}", e),
            )),
        }
    }

    /// Shows the interface in another language at once, and keeps it.
    pub fn set_language(&mut self, code: &str) -> Option<Note> {
        self.settings.language = code.to_string();
        crate::i18n::apply(code);
        self.save()
    }

    /// Writes the settings, text fields included. `None` when it worked.
    pub fn save(&mut self) -> Option<Note> {
        self.settings
            .save(&self.paths)
            .err()
            .map(|e| (Level::Warn, tr!("Could not save the settings: {e}", e)))
    }

    /// Re-reads the history file, which a finished download may have
    /// added to, and the open entry list with it.
    pub fn refresh_history(&mut self) {
        self.history = self
            .settings
            .archive
            .then(|| self.settings.history_count(&self.paths))
            .flatten();
        if self.history_list.is_some() {
            self.history_list = self.settings.history_lines(&self.paths);
        }
    }

    /// Applies one change. Choices are saved at once; typed text waits
    /// (see [`Field`]). Returns what the activity log should say, if
    /// anything.
    pub fn update(&mut self, message: Message) -> Option<Note> {
        let s = &mut self.settings;
        match message {
            Message::Text(field, value) => {
                *field.slot(s) = value;
                // The history file's name is one of these: keep its count
                // honest as it is typed.
                if field == Field::ArchiveFile {
                    self.refresh_history();
                }
                return None;
            }
            Message::Toggle(flag, value) => *flag.slot(s) = value,
            Message::Kind(value) => s.kind = value,
            Message::VideoFormat(value) => s.video_format = value,
            Message::Quality(value) => s.quality = value,
            Message::Audio(value) => s.audio = value,
            Message::AudioQuality(value) => s.audio_quality = value,
            Message::Container(value) => s.container = value,
            Message::Convert(value) => s.convert = value,
            Message::ThumbFormat(value) => s.thumbnail_format = value,
            Message::Subs(value) => s.subs = value,
            Message::Sponsor(value) => s.sponsor = value,
            Message::Segments(value) => s.segments = value,
            Message::Playlist(value) => s.playlist = value,
            Message::Template(value) => s.template = value,
            Message::Browser(value) => s.cookies_browser = value,
            Message::ClearHistory => {
                self.clearing_history = true;
                return None;
            }
            Message::CancelClearHistory => {
                self.clearing_history = false;
                return None;
            }
            Message::ConfirmClearHistory => {
                self.clearing_history = false;
                let entries = self.settings.history_count(&self.paths).unwrap_or(0);
                let path = self.settings.archive_path(&self.paths);
                // Emptied rather than removed, so a hard linked copy kept
                // elsewhere is cleared too.
                let cleared = if path.exists() {
                    std::fs::write(&path, "")
                } else {
                    Err(std::io::ErrorKind::NotFound.into())
                };
                let note = match cleared {
                    Ok(()) => (
                        Level::Ok,
                        tr!("History cleared ({n} entry)."
                            | "History cleared ({n} entries)." % entries),
                    ),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        (Level::Info, tr!("There is no history file yet."))
                    }
                    Err(e) => (Level::Warn, tr!("Could not clear the history: {e}", e)),
                };
                self.history = None;
                self.history_list = None;
                return Some(note);
            }
            Message::ChooseFolder => return None,
            Message::FolderChosen(None) => return None,
            Message::FolderChosen(Some(folder)) => {
                s.output_dir = folder.to_string_lossy().into_owned();
            }
            Message::ToggleAdvanced => {
                self.show_advanced = !self.show_advanced;
                return None;
            }
            Message::DesktopEntry(wanted) => return self.set_desktop_entry(wanted),
            Message::ToggleHistoryList => {
                self.history_list = if self.history_list.is_some() {
                    None
                } else {
                    self.settings.history_lines(&self.paths)
                };
                return None;
            }
        }
        // Cheap, and it keeps the history count honest after the toggle
        // changes.
        self.refresh_history();
        self.save()
    }

    /// Opens the desktop's folder dialog on a thread of its own (it blocks
    /// until closed) and answers with [`Message::FolderChosen`]. It starts
    /// in the current output folder when that exists.
    pub fn choose_folder(&self) -> Task<Message> {
        let start = self.settings.output_dir(&self.paths);
        let title = tr!("Where to save downloads");
        Task::perform(
            in_background(move || {
                let mut dialog = rfd::FileDialog::new().set_title(title);
                if start.is_dir() {
                    dialog = dialog.set_directory(&start);
                }
                dialog.pick_folder()
            }),
            |folder| Message::FolderChosen(folder.flatten()),
        )
    }

    pub fn view(&self) -> Element<'_, Message> {
        view(
            &self.settings,
            &self.paths,
            History {
                count: self.history,
                entries: self.history_list.as_deref(),
                confirming_clear: self.clearing_history,
            },
            self.show_advanced,
            self.desktop_entry,
        )
    }
}

fn desktop_section<'a>(installed: bool) -> Element<'a, Message> {
    column![
        switch(installed)
            .label(tr!("Show MimirDLP in the applications menu"))
            .on_toggle(Message::DesktopEntry),
        text(tr!(
            "Adds a launcher and the icon to your user folder, so MimirDLP can be started from the menu and shows its icon in the taskbar. Turning it off removes them."
        ))
        .size(13)
        .style(style::muted_text),
    ]
    .spacing(8)
    .into()
}

/// The row that folds the less common sections in and out, like an
/// Adwaita expander row.
fn advanced_toggle<'a>(open: bool, sign_in: bool) -> Element<'a, Message> {
    let (label, hint) = if open {
        (tr!("Hide advanced options"), "\u{2212}")
    } else {
        (tr!("Advanced options"), "+")
    };
    let contents = if sign_in {
        tr!("Signing in, playlists, history, file names, speed limits")
    } else {
        tr!("Playlists, history, file names, speed limits")
    };
    button(
        row![
            column![
                text(label),
                text(contents).size(13).style(style::muted_text),
            ]
            .spacing(2)
            .width(Length::Fill),
            text(hint).size(20),
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([12, 16])
    .style(style::expander)
    .on_press(Message::ToggleAdvanced)
    .into()
}

/// A card's rows with a line between each two, like an Adwaita boxed list.
/// Built like a `column!`: `rows![...]`, then `.push`.
struct Rows<'a>(Vec<Element<'a, Message>>);

macro_rules! rows {
    ($($row:expr),* $(,)?) => {
        Rows(Vec::new())$(.push($row))*
    };
}

impl<'a> Rows<'a> {
    fn push(mut self, row: impl Into<Element<'a, Message>>) -> Self {
        self.0.push(row.into());
        self
    }
}

impl<'a> From<Rows<'a>> for Element<'a, Message> {
    fn from(rows: Rows<'a>) -> Self {
        let mut list = Column::new().spacing(10);
        for (i, row) in rows.0.into_iter().enumerate() {
            if i > 0 {
                list = list.push(rule::horizontal(1).style(style::separator));
            }
            list = list.push(row);
        }
        list.into()
    }
}

/// A typed value, saved later (see [`Field`]).
fn text_field<'a>(
    placeholder: &str,
    value: &'a str,
    field: Field,
    width: u32,
) -> Element<'a, Message> {
    entry(placeholder, value)
        .on_input(move |v| Message::Text(field, v))
        .padding(8)
        .width(width)
        .into()
}

/// yt-dlp's `--audio-quality` scale.
const AUDIO_QUALITIES: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

/// Context `history_row` needs that isn't part of `Settings` itself: the
/// live state of the history file.
struct History<'a> {
    /// How many videos the history holds, when it has been looked at.
    count: Option<usize>,
    /// The history's own entries, shown one by one while this is `Some`.
    entries: Option<&'a [String]>,
    confirming_clear: bool,
}

fn view<'a>(
    settings: &'a Settings,
    paths: &'a Paths,
    history: History<'a>,
    show_advanced: bool,
    desktop_entry: bool,
) -> Element<'a, Message> {
    let page = column![
        text(tr!("These apply to every link you queue from now on. Links already in the queue keep what they were queued with.")).style(style::muted_text),
        card(tr!("What to download"), format_section(settings)),
        card(tr!("Where to save"), destination_section(settings, paths)),
        card(tr!("Extras"), extras_section(settings)),
    ]
    .spacing(20);
    // Only Linux desktops build their menus from .desktop files, and an
    // installed copy came with its own.
    let page = if cfg!(target_os = "linux") && !paths.installed {
        page.push(card(tr!("Desktop"), desktop_section(desktop_entry)))
    } else {
        page
    };
    // Browser cookies are left out of the Flatpak: its sandbox can't read
    // the browsers' folders (Settings::cookies ignores them there too).
    let sign_in = !paths.bundled_tools();
    let page = page.push(advanced_toggle(show_advanced, sign_in));
    if !show_advanced {
        return page.into();
    }
    let page = if sign_in {
        page.push(card(tr!("Signing in"), sign_in_section(settings)))
    } else {
        page
    };
    page.push(card(
        tr!("Automation and naming"),
        automation_section(settings, history),
    ))
    .push(card(tr!("Network and other"), network_section(settings)))
    .into()
}

fn format_section(settings: &Settings) -> Element<'_, Message> {
    let mut body = rows![
        radio(
            tr!("Video and audio"),
            Kind::VideoAudio,
            Some(settings.kind),
            Message::Kind
        ),
        radio(
            tr!("Audio only"),
            Kind::AudioOnly,
            Some(settings.kind),
            Message::Kind
        ),
    ];

    match settings.kind {
        Kind::VideoAudio => {
            body = body.push(choice(
                tr!("Format"),
                dropdown(
                    VideoFormat::ALL,
                    Some(settings.video_format),
                    Message::VideoFormat,
                )
                .into(),
            ));
            if settings.video_format == VideoFormat::Custom {
                body = body.push(choice(
                    tr!("Format string"),
                    text_field(
                        "bestvideo+bestaudio",
                        &settings.custom_format,
                        Field::CustomFormat,
                        300,
                    ),
                ));
            }
            body = body
                .push(choice(
                    tr!("Quality"),
                    dropdown(Quality::ALL, Some(settings.quality), Message::Quality).into(),
                ))
                .push(choice(
                    tr!("Container"),
                    dropdown(Container::ALL, Some(settings.container), Message::Container).into(),
                ))
                .push(choice(
                    tr!("Convert the container"),
                    dropdown(Convert::ALL, Some(settings.convert), Message::Convert).into(),
                ));
            if settings.convert != Convert::Keep {
                body = body.push(choice(
                    tr!("Convert to"),
                    text_field("mp4", &settings.convert_target, Field::ConvertTarget, 160),
                ));
            }
        }
        Kind::AudioOnly => {
            body = body.push(
                switch(settings.audio_convert)
                    .label(tr!("Convert it to another format"))
                    .on_toggle(|v| Message::Toggle(Flag::AudioConvert, v)),
            );
            if settings.audio_convert {
                body = body.push(choice(
                    tr!("Audio format"),
                    dropdown(Audio::ALL, Some(settings.audio), Message::Audio).into(),
                ));
                if !settings.audio.is_lossless() {
                    body = body.push(choice(
                        tr!("Audio quality (0 best, 10 smallest)"),
                        dropdown(
                            AUDIO_QUALITIES,
                            Some(settings.audio_quality),
                            Message::AudioQuality,
                        )
                        .into(),
                    ));
                }
            } else {
                body = body.push(
                    text(tr!("The audio is kept exactly as the site serves it."))
                        .size(13)
                        .style(style::muted_text),
                );
            }
        }
    }
    body.into()
}

fn extras_section(settings: &Settings) -> Element<'_, Message> {
    let mut body = rows![
        switch(settings.thumbnail)
            .label(tr!("Embed the thumbnail"))
            .on_toggle(|v| Message::Toggle(Flag::Thumbnail, v)),
    ];
    if settings.thumbnail {
        body = body.push(choice(
            tr!("Thumbnail format"),
            dropdown(
                Thumb::ALL,
                Some(settings.thumbnail_format),
                Message::ThumbFormat,
            )
            .into(),
        ));
    }
    body = body
        .push(
            switch(settings.metadata)
                .label(tr!("Embed title, artist and date"))
                .on_toggle(|v| Message::Toggle(Flag::Metadata, v)),
        )
        .push(
            switch(settings.chapters)
                .label(tr!("Embed chapters"))
                .on_toggle(|v| Message::Toggle(Flag::Chapters, v)),
        )
        .push(choice(
            tr!("Subtitles"),
            dropdown(Subs::ALL, Some(settings.subs), Message::Subs).into(),
        ));

    if settings.subs != Subs::Off {
        body = body
            .push(choice(
                tr!("Languages"),
                text_field(
                    &tr!("all but live chat"),
                    &settings.sub_langs,
                    Field::SubLangs,
                    240,
                ),
            ))
            .push(
                switch(settings.auto_subs)
                    .label(tr!("Include auto-generated subtitles"))
                    .on_toggle(|v| Message::Toggle(Flag::AutoSubs, v)),
            );
    }
    body = body.push(choice(
        // A name, the same in every language.
        "SponsorBlock".to_string(),
        dropdown(Sponsor::ALL, Some(settings.sponsor), Message::Sponsor).into(),
    ));
    if settings.sponsor != Sponsor::Off {
        body = body.push(choice(
            tr!("Segments"),
            dropdown(Segments::ALL, Some(settings.segments), Message::Segments).into(),
        ));
        if settings.segments == Segments::Custom {
            body = body.push(choice(
                tr!("Categories"),
                text_field(
                    "sponsor,intro,outro",
                    &settings.custom_segments,
                    Field::CustomSegments,
                    240,
                ),
            ));
        }
    }
    if settings.sponsor == Sponsor::Remove {
        body = body.push(
            switch(settings.keyframes)
                .label(tr!("Cut exactly at keyframes (re-encodes, much slower)"))
                .on_toggle(|v| Message::Toggle(Flag::Keyframes, v)),
        );
    }
    body.into()
}

fn automation_section<'a>(settings: &'a Settings, history: History<'a>) -> Element<'a, Message> {
    let mut body = rows![choice(
        tr!("Playlists"),
        dropdown(
            PlaylistChoice::ALL,
            Some(settings.playlist),
            Message::Playlist
        )
        .into(),
    )];

    if settings.playlist == PlaylistChoice::Whole {
        body = body
            .push(choice(
                tr!("Items (empty for all)"),
                text_field("1-5,8", &settings.playlist_items, Field::PlaylistItems, 160),
            ))
            .push(
                switch(settings.playlist_reverse)
                    .label(tr!("Start from the end of the playlist"))
                    .on_toggle(|v| Message::Toggle(Flag::PlaylistReverse, v)),
            )
            .push(
                switch(settings.organize)
                    .label(tr!("One folder per playlist, numbered"))
                    .on_toggle(|v| Message::Toggle(Flag::Organize, v)),
            );
    }

    body = body.push(
        switch(settings.archive)
            .label(tr!("Keep a history and skip what was downloaded before"))
            .on_toggle(|v| Message::Toggle(Flag::Archive, v)),
    );
    if settings.archive {
        body = body
            .push(choice(
                tr!("History file"),
                text_field(
                    "download_archive.txt",
                    &settings.archive_file,
                    Field::ArchiveFile,
                    240,
                ),
            ))
            .push(
                switch(settings.archive_stop)
                    .label(tr!("Stop as soon as a video is already in the history"))
                    .on_toggle(|v| Message::Toggle(Flag::ArchiveStop, v)),
            )
            .push(history_row(history));
    }
    body = body
        .push(choice(
            tr!("Stop after this many files"),
            text_field(
                &tr!("no limit"),
                &settings.max_downloads,
                Field::MaxDownloads,
                160,
            ),
        ))
        .push(choice(
            tr!("File names"),
            dropdown(Template::ALL, Some(settings.template), Message::Template).into(),
        ));
    if settings.template == Template::Custom {
        body = body.push(choice(
            tr!("Template"),
            text_field(
                "%(title)s [%(id)s].%(ext)s",
                &settings.custom_template,
                Field::CustomTemplate,
                300,
            ),
        ));
    }
    body = body.push(
        switch(settings.restrict_filenames)
            .label(tr!("Plain ASCII file names"))
            .on_toggle(|v| Message::Toggle(Flag::Restrict, v)),
    );
    body.into()
}

/// What the history holds, and a two step way to throw it away.
fn history_row(history: History<'_>) -> Element<'_, Message> {
    if history.confirming_clear {
        return row![
            text(tr!("This deletes the history file for good."))
                .style(text::warning)
                .width(Length::Fill),
            button(text(tr!("Cancel")).size(13))
                .padding([4, 14])
                .style(style::pill_secondary)
                .on_press(Message::CancelClearHistory),
            button(text(tr!("Clear")).size(13))
                .padding([4, 14])
                .style(style::pill_danger)
                .on_press(Message::ConfirmClearHistory),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into();
    }
    let summary = match history.count {
        Some(count) => tr!("{n} video in the history" | "{n} videos in the history" % count),
        None => tr!("The history file doesn't exist yet."),
    };
    let row = row![
        text(summary)
            .size(13)
            .style(style::muted_text)
            .width(Length::Fill),
        button(
            text(if history.entries.is_some() {
                tr!("Hide entries")
            } else {
                tr!("View entries")
            })
            .size(13)
        )
        .padding([4, 14])
        .style(style::pill_secondary)
        .on_press_maybe(
            history
                .count
                .is_some()
                .then_some(Message::ToggleHistoryList)
        ),
        button(text(tr!("Clear history")).size(13))
            .padding([4, 14])
            .style(style::pill_secondary)
            .on_press_maybe(history.count.is_some().then_some(Message::ClearHistory)),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    match history.entries {
        None => row.into(),
        Some(lines) => column![row, history_list_view(lines)].spacing(10).into(),
    }
}

/// The history's own entries, one per line as yt-dlp wrote them. Scrollable
/// and height capped, so a long history doesn't push the rest of the card
/// off the screen.
fn history_list_view(lines: &[String]) -> Element<'_, Message> {
    let body: Element<'_, Message> = if lines.is_empty() {
        text(tr!("The history is empty."))
            .size(13)
            .style(style::muted_text)
            .into()
    } else {
        Column::with_children(
            lines
                .iter()
                .map(|line| text(line).font(Font::MONOSPACE).size(13).into()),
        )
        .spacing(4)
        .into()
    };
    container(scrollable(container(body).padding(10).width(Length::Fill)).height(140))
        .style(style::card)
        .into()
}

/// Cookies from a browser the user is signed in with, for videos that need
/// an account (age restricted, members only, private).
fn sign_in_section(settings: &Settings) -> Element<'_, Message> {
    let mut body = rows![choice(
        tr!("Use cookies from"),
        dropdown(
            Browser::ALL,
            Some(settings.cookies_browser),
            Message::Browser
        )
        .into(),
    )];
    if settings.cookies_browser != Browser::None {
        body = body
            .push(choice(
                tr!("Profile (empty for the last used)"),
                text_field(
                    &tr!("default"),
                    &settings.cookies_profile,
                    Field::CookiesProfile,
                    240,
                ),
            ))
            .push(
                text(tr!(
                    "Only for videos that need you signed in. Firefox works best: \
                     Chrome, Edge and other Chromium browsers may have to be closed \
                     first, and recent versions on Windows can't be read at all."
                ))
                .size(13)
                .style(style::muted_text),
            );
    }
    body.into()
}

fn network_section(settings: &Settings) -> Element<'_, Message> {
    rows![
        choice(
            tr!("Speed limit"),
            text_field(
                &tr!("no limit"),
                &settings.limit_rate,
                Field::LimitRate,
                160
            ),
        ),
        choice(
            tr!("Parts downloaded at once"),
            text_field("5", &settings.fragments, Field::Fragments, 160),
        ),
        choice(
            tr!("Seconds between requests"),
            text_field("1.5", &settings.sleep, Field::Sleep, 160),
        ),
        switch(settings.live_from_start)
            .label(tr!("Record live streams from the start"))
            .on_toggle(|v| Message::Toggle(Flag::LiveFromStart, v)),
        choice(
            tr!("Wait for a scheduled stream (seconds)"),
            text_field(
                "60-3600",
                &settings.wait_for_video,
                Field::WaitForVideo,
                160
            ),
        ),
        switch(settings.preserve_mtime)
            .label(tr!("Keep the original upload date on the file"))
            .on_toggle(|v| Message::Toggle(Flag::Mtime, v)),
        switch(settings.ignore_errors)
            .label(tr!("Keep going when one video of a playlist fails"))
            .on_toggle(|v| Message::Toggle(Flag::IgnoreErrors, v)),
        switch(settings.verbose)
            .label(tr!("Verbose log (for troubleshooting)"))
            .on_toggle(|v| Message::Toggle(Flag::Verbose, v)),
    ]
    .into()
}

fn destination_section<'a>(settings: &'a Settings, paths: &'a Paths) -> Element<'a, Message> {
    let default = crate::config::default_output_dir(paths);
    let hint = if paths.installed {
        tr!("Leave it empty to use your Downloads folder.")
    } else {
        tr!("Leave it empty to use the downloads folder next to this app.")
    };
    column![
        row![
            entry(&default.to_string_lossy(), &settings.output_dir)
                .on_input(|v| Message::Text(Field::OutputDir, v))
                .padding(10),
            button(text(tr!("Choose...")))
                .padding([10, 18])
                .style(style::pill_secondary)
                .on_press(Message::ChooseFolder),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        text(hint).size(13).style(style::muted_text),
    ]
    .spacing(8)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::tempdir;

    fn page_with_history(dir: &std::path::Path) -> (SettingsPage, std::path::PathBuf) {
        let paths = Paths::new(dir.to_path_buf());
        let settings = Settings {
            archive: true,
            ..Settings::default()
        };
        let history = settings.archive_path(&paths);
        std::fs::write(&history, "youtube a\nyoutube b\n").unwrap();
        (SettingsPage::new(paths, settings), history)
    }

    #[test]
    fn clearing_the_history_asks_first_and_says_how_much_went() {
        let dir = tempdir();
        let (mut page, history) = page_with_history(&dir);
        let _ = page.update(Message::Toggle(Flag::Archive, true));
        assert_eq!(page.history, Some(2), "the count comes from the file");
        assert_eq!(page.update(Message::ClearHistory), None);
        assert!(history.exists(), "nothing goes until it is confirmed");
        let _ = page.update(Message::CancelClearHistory);
        assert!(history.exists());
        let _ = page.update(Message::ClearHistory);
        let note = page.update(Message::ConfirmClearHistory).unwrap();
        assert_eq!(std::fs::read_to_string(&history).unwrap(), "", "emptied");
        assert!(note.1.contains("(2 entries)"), "{note:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_history_entries_can_be_shown_one_by_one_and_hidden_again() {
        let dir = tempdir();
        let (mut page, _) = page_with_history(&dir);
        assert_eq!(page.history_list, None, "closed until asked to open");
        let _ = page.update(Message::ToggleHistoryList);
        assert_eq!(
            page.history_list,
            Some(vec!["youtube a".to_string(), "youtube b".to_string()])
        );
        let _ = page.update(Message::ToggleHistoryList);
        assert_eq!(page.history_list, None, "toggles back off");
        // Clearing the history closes an open list rather than leaving it
        // pointing at a file that no longer exists.
        let _ = page.update(Message::ToggleHistoryList);
        let _ = page.update(Message::ClearHistory);
        let _ = page.update(Message::ConfirmClearHistory);
        assert_eq!(page.history_list, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Choices are written at once; typed text only when saved (leaving
    /// the page, starting a download, closing the window).
    #[test]
    fn choices_are_saved_at_once_and_typed_text_on_save() {
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        let mut page = SettingsPage::new(paths.clone(), Settings::default());
        let _ = page.update(Message::Text(Field::OutputDir, "/videos".into()));
        assert_eq!(Settings::load(&paths).output_dir, "");
        let _ = page.update(Message::Toggle(Flag::Verbose, true));
        let on_disk = Settings::load(&paths);
        assert!(on_disk.verbose);
        // A choice writes everything, typed text included.
        assert_eq!(on_disk.output_dir, "/videos");
        let _ = page.update(Message::Text(Field::Sleep, "3".into()));
        assert_eq!(page.save(), None);
        assert_eq!(Settings::load(&paths).sleep, "3");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_chosen_folder_becomes_the_output_folder_and_is_saved() {
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        let mut page = SettingsPage::new(paths.clone(), Settings::default());
        let _ = page.update(Message::FolderChosen(None));
        assert_eq!(
            page.settings().output_dir,
            "",
            "a cancelled dialog changes nothing"
        );
        let _ = page.update(Message::FolderChosen(Some("/videos/new".into())));
        assert_eq!(page.settings().output_dir, "/videos/new");
        assert_eq!(Settings::load(&paths).output_dir, "/videos/new");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Every field and flag reaches its own setting and no other.
    #[test]
    fn every_field_and_flag_edits_its_own_setting() {
        let fields = [
            Field::OutputDir,
            Field::CustomFormat,
            Field::ConvertTarget,
            Field::SubLangs,
            Field::CustomSegments,
            Field::PlaylistItems,
            Field::ArchiveFile,
            Field::MaxDownloads,
            Field::CustomTemplate,
            Field::LimitRate,
            Field::Fragments,
            Field::Sleep,
            Field::WaitForVideo,
            Field::CookiesProfile,
        ];
        for field in fields {
            let mut s = Settings::default();
            *field.slot(&mut s) = "marker".into();
            let serialized = format!("{s:?}");
            assert_eq!(serialized.matches("\"marker\"").count(), 1, "{field:?}");
        }
        let flags = [
            Flag::AudioConvert,
            Flag::Thumbnail,
            Flag::Metadata,
            Flag::Chapters,
            Flag::AutoSubs,
            Flag::Keyframes,
            Flag::PlaylistReverse,
            Flag::Organize,
            Flag::Archive,
            Flag::ArchiveStop,
            Flag::LiveFromStart,
            Flag::Verbose,
            Flag::Restrict,
            Flag::Mtime,
            Flag::IgnoreErrors,
        ];
        let base = Settings::default();
        for flag in flags {
            let mut s = base.clone();
            let slot = flag.slot(&mut s);
            *slot = !*slot;
            assert_ne!(s, base, "{flag:?} changed nothing");
            // Flipping it back restores the default: it touched one field.
            let slot = flag.slot(&mut s);
            *slot = !*slot;
            assert_eq!(s, base, "{flag:?}");
        }
        // Two flags can't share a field.
        for (i, a) in flags.iter().enumerate() {
            for b in &flags[i + 1..] {
                let mut s = base.clone();
                *a.slot(&mut s) = !*a.slot(&mut base.clone());
                let before = *b.slot(&mut base.clone());
                assert_eq!(*b.slot(&mut s), before, "{a:?} also changed {b:?}");
            }
        }
    }
}
