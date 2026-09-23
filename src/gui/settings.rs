//! The Settings page and the settings themselves.
//!
//! `SettingsPage` owns [`Settings`], saves it, and keeps the history
//! file's live state (count, entry list, the two step clear). Its values
//! apply to links queued from then on: the Download screen copies them
//! into each item when it is added, so an item keeps what it was queued
//! with. `config::Settings`, `Settings::to_options` and `Settings::problems`
//! hold the model and its rules; this module only edits and shows it.

use iced::futures::channel::oneshot;
use iced::widget::{
    Column, button, column, container, pick_list, radio, row, scrollable, text, text_input, toggler,
};
use iced::{Alignment, Element, Font, Length, Task, Theme};

use super::{card, choice, style};
use crate::config::{
    Audio, Browser, Cats, Container, Convert, Kind, PlaylistChoice, Quality, Settings, Sponsor,
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
    Cats(Cats),
    Playlist(PlaylistChoice),
    Template(Template),
    Browser(Browser),
    ClearHistory,
    ConfirmClearHistory,
    CancelClearHistory,
    ToggleHistoryList,
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
    CustomCats,
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
            Field::CustomCats => &mut s.custom_cats,
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
}

impl SettingsPage {
    pub fn new(paths: Paths, settings: Settings) -> Self {
        let mut page = SettingsPage {
            paths,
            settings,
            history: None,
            history_list: None,
            clearing_history: false,
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

    /// Writes the settings, text fields included. `None` when it worked.
    pub fn save(&mut self) -> Option<Note> {
        self.settings
            .save(&self.paths)
            .err()
            .map(|e| (Level::Warn, format!("Could not save the settings: {e}")))
    }

    /// Re-reads the history file, which a finished download may have
    /// added to, and the open entry list with it.
    pub fn refresh_history(&mut self) {
        self.history = self
            .settings
            .archive
            .then(|| self.settings.history_entries(&self.paths))
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
            Message::Cats(value) => s.cats = value,
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
                let entries = self.settings.history_entries(&self.paths).unwrap_or(0);
                let path = self.settings.archive_path(&self.paths);
                let note = match std::fs::remove_file(&path) {
                    Ok(()) => (Level::Ok, format!("History cleared ({entries} entries).")),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        (Level::Info, "There is no history file yet.".into())
                    }
                    Err(e) => (Level::Warn, format!("Could not clear the history: {e}")),
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
        let (tx, rx) = oneshot::channel();
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title("Where to save downloads");
            if start.is_dir() {
                dialog = dialog.set_directory(&start);
            }
            let _ = tx.send(dialog.pick_folder());
        });
        Task::perform(
            async move { rx.await.ok().flatten() },
            Message::FolderChosen,
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
        )
    }
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
) -> Element<'a, Message> {
    column![
        text("Settings").size(26),
        text("These apply to every link you queue from now on. Links already in the queue keep what they were queued with.").style(|t: &Theme| {
            text::Style {
                color: Some(style::muted(t)),
            }
        }),
        card("What to download", format_section(settings)),
        card("Extras", extras_section(settings)),
        card(
            "Automation and naming",
            automation_section(settings, history)
        ),
        card("Signing in", sign_in_section(settings)),
        card("Advanced", advanced_section(settings)),
        card("Where to save", destination_section(settings, paths)),
    ]
    .spacing(20)
    .into()
}

fn format_section(settings: &Settings) -> Element<'_, Message> {
    let mut body = column![
        radio(
            "Video and audio",
            Kind::VideoAudio,
            Some(settings.kind),
            Message::Kind
        ),
        radio(
            "Audio only",
            Kind::AudioOnly,
            Some(settings.kind),
            Message::Kind
        ),
    ]
    .spacing(12);

    match settings.kind {
        Kind::VideoAudio => {
            body = body.push(choice(
                "Format",
                pick_list(
                    VideoFormat::ALL,
                    Some(settings.video_format),
                    Message::VideoFormat,
                )
                .into(),
            ));
            if settings.video_format == VideoFormat::Custom {
                body = body.push(choice(
                    "Format string",
                    text_input("bestvideo+bestaudio", &settings.custom_format)
                        .on_input(|v| Message::Text(Field::CustomFormat, v))
                        .padding(8)
                        .width(300)
                        .into(),
                ));
            }
            body = body
                .push(choice(
                    "Quality",
                    pick_list(Quality::ALL, Some(settings.quality), Message::Quality).into(),
                ))
                .push(choice(
                    "Container",
                    pick_list(Container::ALL, Some(settings.container), Message::Container).into(),
                ))
                .push(choice(
                    "Convert the container",
                    pick_list(Convert::ALL, Some(settings.convert), Message::Convert).into(),
                ));
            if settings.convert != Convert::Keep {
                body = body.push(choice(
                    "Convert to",
                    text_input("mp4", &settings.convert_target)
                        .on_input(|v| Message::Text(Field::ConvertTarget, v))
                        .padding(8)
                        .width(160)
                        .into(),
                ));
            }
        }
        Kind::AudioOnly => {
            body = body.push(
                toggler(settings.audio_convert)
                    .label("Convert it to another format")
                    .on_toggle(|v| Message::Toggle(Flag::AudioConvert, v)),
            );
            if settings.audio_convert {
                body = body.push(choice(
                    "Audio format",
                    pick_list(Audio::ALL, Some(settings.audio), Message::Audio).into(),
                ));
                if !settings.audio.is_lossless() {
                    body = body.push(choice(
                        "Audio quality (0 best, 10 smallest)",
                        pick_list(
                            AUDIO_QUALITIES,
                            Some(settings.audio_quality),
                            Message::AudioQuality,
                        )
                        .into(),
                    ));
                }
            } else {
                body = body.push(
                    text("The audio is kept exactly as the site serves it.")
                        .size(13)
                        .style(|t: &Theme| text::Style {
                            color: Some(style::muted(t)),
                        }),
                );
            }
        }
    }
    body.into()
}

fn extras_section(settings: &Settings) -> Element<'_, Message> {
    let mut body = column![
        toggler(settings.thumbnail)
            .label("Embed the thumbnail")
            .on_toggle(|v| Message::Toggle(Flag::Thumbnail, v)),
    ]
    .spacing(12);
    if settings.thumbnail {
        body = body.push(choice(
            "Thumbnail format",
            pick_list(
                Thumb::ALL,
                Some(settings.thumbnail_format),
                Message::ThumbFormat,
            )
            .into(),
        ));
    }
    body = body
        .push(
            toggler(settings.metadata)
                .label("Embed title, artist and date")
                .on_toggle(|v| Message::Toggle(Flag::Metadata, v)),
        )
        .push(
            toggler(settings.chapters)
                .label("Embed chapters")
                .on_toggle(|v| Message::Toggle(Flag::Chapters, v)),
        )
        .push(choice(
            "Subtitles",
            pick_list(Subs::ALL, Some(settings.subs), Message::Subs).into(),
        ));

    if settings.subs != Subs::Off {
        body = body
            .push(choice(
                "Languages",
                text_input("all but live chat", &settings.sub_langs)
                    .on_input(|v| Message::Text(Field::SubLangs, v))
                    .padding(8)
                    .width(240)
                    .into(),
            ))
            .push(
                toggler(settings.auto_subs)
                    .label("Include auto-generated subtitles")
                    .on_toggle(|v| Message::Toggle(Flag::AutoSubs, v)),
            );
    }
    body = body.push(choice(
        "SponsorBlock",
        pick_list(Sponsor::ALL, Some(settings.sponsor), Message::Sponsor).into(),
    ));
    if settings.sponsor != Sponsor::Off {
        body = body.push(choice(
            "Segments",
            pick_list(Cats::ALL, Some(settings.cats), Message::Cats).into(),
        ));
        if settings.cats == Cats::Custom {
            body = body.push(choice(
                "Categories",
                text_input("sponsor,intro,outro", &settings.custom_cats)
                    .on_input(|v| Message::Text(Field::CustomCats, v))
                    .padding(8)
                    .width(240)
                    .into(),
            ));
        }
    }
    if settings.sponsor == Sponsor::Remove {
        body = body.push(
            toggler(settings.keyframes)
                .label("Cut exactly at keyframes (re-encodes, much slower)")
                .on_toggle(|v| Message::Toggle(Flag::Keyframes, v)),
        );
    }
    body.into()
}

fn automation_section<'a>(settings: &'a Settings, history: History<'a>) -> Element<'a, Message> {
    let mut body = column![choice(
        "Playlists",
        pick_list(
            PlaylistChoice::ALL,
            Some(settings.playlist),
            Message::Playlist
        )
        .into(),
    )]
    .spacing(12);

    if settings.playlist == PlaylistChoice::Whole {
        body = body
            .push(choice(
                "Items (empty for all)",
                text_input("1-5,8", &settings.playlist_items)
                    .on_input(|v| Message::Text(Field::PlaylistItems, v))
                    .padding(8)
                    .width(160)
                    .into(),
            ))
            .push(
                toggler(settings.playlist_reverse)
                    .label("Start from the end of the playlist")
                    .on_toggle(|v| Message::Toggle(Flag::PlaylistReverse, v)),
            )
            .push(
                toggler(settings.organize)
                    .label("One folder per playlist, numbered")
                    .on_toggle(|v| Message::Toggle(Flag::Organize, v)),
            );
    }

    body = body.push(
        toggler(settings.archive)
            .label("Keep a history and skip what was downloaded before")
            .on_toggle(|v| Message::Toggle(Flag::Archive, v)),
    );
    if settings.archive {
        body = body
            .push(choice(
                "History file",
                text_input("download_archive.txt", &settings.archive_file)
                    .on_input(|v| Message::Text(Field::ArchiveFile, v))
                    .padding(8)
                    .width(240)
                    .into(),
            ))
            .push(
                toggler(settings.archive_stop)
                    .label("Stop as soon as a video is already in the history")
                    .on_toggle(|v| Message::Toggle(Flag::ArchiveStop, v)),
            )
            .push(history_row(history));
    }
    body = body
        .push(choice(
            "Stop after this many files",
            text_input("no limit", &settings.max_downloads)
                .on_input(|v| Message::Text(Field::MaxDownloads, v))
                .padding(8)
                .width(160)
                .into(),
        ))
        .push(choice(
            "File names",
            pick_list(Template::ALL, Some(settings.template), Message::Template).into(),
        ));
    if settings.template == Template::Custom {
        body = body.push(choice(
            "Template",
            text_input("%(title)s [%(id)s].%(ext)s", &settings.custom_template)
                .on_input(|v| Message::Text(Field::CustomTemplate, v))
                .padding(8)
                .width(300)
                .into(),
        ));
    }
    body = body.push(
        toggler(settings.restrict_filenames)
            .label("Plain ASCII file names")
            .on_toggle(|v| Message::Toggle(Flag::Restrict, v)),
    );
    body.into()
}

/// What the history holds, and a two step way to throw it away.
fn history_row(history: History<'_>) -> Element<'_, Message> {
    if history.confirming_clear {
        return row![
            text("This deletes the history file for good.")
                .style(text::warning)
                .width(Length::Fill),
            button(text("Cancel").size(13))
                .padding([4, 14])
                .style(style::pill_secondary)
                .on_press(Message::CancelClearHistory),
            button(text("Clear").size(13))
                .padding([4, 14])
                .style(style::pill_danger)
                .on_press(Message::ConfirmClearHistory),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into();
    }
    let summary = match history.count {
        Some(1) => "1 video in the history".to_string(),
        Some(n) => format!("{n} videos in the history"),
        None => "The history file doesn't exist yet.".to_string(),
    };
    let row = row![
        text(summary)
            .size(13)
            .style(|t: &Theme| text::Style {
                color: Some(style::muted(t))
            })
            .width(Length::Fill),
        button(
            text(if history.entries.is_some() {
                "Hide entries"
            } else {
                "View entries"
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
        button(text("Clear history").size(13))
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
        text("The history is empty.")
            .size(13)
            .style(|t: &Theme| text::Style {
                color: Some(style::muted(t)),
            })
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
    let mut body = column![choice(
        "Use cookies from",
        pick_list(
            Browser::ALL,
            Some(settings.cookies_browser),
            Message::Browser
        )
        .into(),
    )]
    .spacing(12);
    if settings.cookies_browser != Browser::None {
        body = body
            .push(choice(
                "Profile (empty for the last used)",
                text_input("default", &settings.cookies_profile)
                    .on_input(|v| Message::Text(Field::CookiesProfile, v))
                    .padding(8)
                    .width(240)
                    .into(),
            ))
            .push(
                text(
                    "Only for videos that need you signed in. Firefox works best: \
                     Chrome, Edge and other Chromium browsers may have to be closed \
                     first, and recent versions on Windows can't be read at all.",
                )
                .size(13)
                .style(|t: &Theme| text::Style {
                    color: Some(style::muted(t)),
                }),
            );
    }
    body.into()
}

fn advanced_section(settings: &Settings) -> Element<'_, Message> {
    column![
        choice(
            "Speed limit",
            text_input("no limit", &settings.limit_rate)
                .on_input(|v| Message::Text(Field::LimitRate, v))
                .padding(8)
                .width(160)
                .into(),
        ),
        choice(
            "Parts downloaded at once",
            text_input("5", &settings.fragments)
                .on_input(|v| Message::Text(Field::Fragments, v))
                .padding(8)
                .width(160)
                .into(),
        ),
        choice(
            "Seconds between requests",
            text_input("1.5", &settings.sleep)
                .on_input(|v| Message::Text(Field::Sleep, v))
                .padding(8)
                .width(160)
                .into(),
        ),
        toggler(settings.live_from_start)
            .label("Record live streams from the start")
            .on_toggle(|v| Message::Toggle(Flag::LiveFromStart, v)),
        choice(
            "Wait for a scheduled stream (seconds)",
            text_input("60-3600", &settings.wait_for_video)
                .on_input(|v| Message::Text(Field::WaitForVideo, v))
                .padding(8)
                .width(160)
                .into(),
        ),
        toggler(settings.preserve_mtime)
            .label("Keep the original upload date on the file")
            .on_toggle(|v| Message::Toggle(Flag::Mtime, v)),
        toggler(settings.ignore_errors)
            .label("Keep going when one video of a playlist fails")
            .on_toggle(|v| Message::Toggle(Flag::IgnoreErrors, v)),
        toggler(settings.verbose)
            .label("Verbose log (for troubleshooting)")
            .on_toggle(|v| Message::Toggle(Flag::Verbose, v)),
    ]
    .spacing(12)
    .into()
}

fn destination_section<'a>(settings: &'a Settings, paths: &'a Paths) -> Element<'a, Message> {
    let default = paths.app_dir.join("downloads");
    column![
        row![
            text_input(&default.to_string_lossy(), &settings.output_dir)
                .on_input(|v| Message::Text(Field::OutputDir, v))
                .padding(10),
            button(text("Choose..."))
                .padding([10, 18])
                .style(style::pill_secondary)
                .on_press(Message::ChooseFolder),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        text("Leave it empty to use the downloads folder next to this app.")
            .size(13)
            .style(|t: &Theme| text::Style {
                color: Some(style::muted(t))
            }),
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
        assert!(!history.exists());
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
            Field::CustomCats,
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
