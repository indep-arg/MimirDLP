//! The Download screen: a queue of links, each previewed with real metadata
//! before it downloads.
//!
//! Adding a link queues it immediately and kicks off a background lookup
//! (`crate::metadata::fetch`, then a thumbnail fetch) that fills the card in
//! once it resolves; a lookup failure never blocks queuing, it just leaves a
//! plainer card. Each item takes a copy of the Settings page's `Settings`
//! when it is added (kind and quality then stay editable on its card while
//! it waits); turning one item's
//! configuration into yt-dlp arguments is [`Settings::to_options`] plus
//! [`crate::engine::build_args`], and running it is [`crate::runner`]. Items
//! download one at a time, in order; every step yt-dlp reports lands in the
//! tagged activity log shared with the Setup screen.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::thread;

use iced::futures::channel::{mpsc, oneshot};
use iced::widget::{
    Column, button, column, container, image, pick_list, progress_bar, row, rule, scrollable,
    space, text, text_input,
};
use iced::{Alignment, Element, Length, Task, Theme};

use super::{log as log_panel, style};
use crate::config::{self, Kind, Quality, Settings, valid_url};
use crate::engine::{ThumbnailPlan, thumbnail_plan};
use crate::log::Level;
use crate::metadata::{self, Metadata};
use crate::provision::Paths;
use crate::runner::{self, Cancel, Event, Finish, Progress};

/// How many activity lines are kept. Enough to see a whole run, capped so
/// an hours long recording can't grow without end.
const MAX_LOG: usize = 2000;

#[derive(Debug, Clone)]
pub enum Message {
    UrlInput(String),
    AddUrl,
    RemoveItem(u64),
    RetryItem(u64),
    ClearFinished,
    ItemKind(u64, Kind),
    ItemQuality(u64, Quality),
    MetadataFetched(u64, Result<Metadata, String>),
    ThumbnailFetched(u64, Result<Vec<u8>, String>),
    Start,
    Stop,
    OpenFolder,
    OpenItemFolder(String),
    Report(Event),
}

#[derive(Debug, Clone, PartialEq)]
enum MetaState {
    Fetching,
    Ready(Metadata),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
enum ItemStatus {
    Queued,
    Downloading(Progress),
    Completed { path: Option<String> },
    Failed(String),
}

struct QueueItem {
    id: u64,
    url: String,
    /// The Settings page as it was when this link was queued, which is
    /// what that page promises. Kind and quality are then this item's own
    /// quick picks on its card.
    settings: Settings,
    meta: MetaState,
    thumbnail: Option<image::Handle>,
    status: ItemStatus,
}

pub struct Download {
    paths: Paths,
    url_input: String,
    items: Vec<QueueItem>,
    next_id: u64,
    log: Vec<(Level, String)>,
    /// One entry per parallel download of the *current* item, keyed by
    /// yt-dlp's slot number (a live stream fetches video and audio at once).
    progress: BTreeMap<u32, Progress>,
    /// Files yt-dlp reported finished for the current item.
    files: Vec<String>,
    /// The item currently downloading, if any.
    current: Option<u64>,
    /// Present while a download runs, and what stops it.
    job: Option<Arc<Cancel>>,
    /// Shown next to the button when the run can't start.
    problem: Option<String>,
    /// The last line as it arrived, and how often it has repeated, so a
    /// line yt-dlp keeps printing is collapsed instead of filling the log.
    last: Option<(Level, String)>,
    repeats: u32,
    /// Metadata lookups still running. Each one runs yt-dlp, which Setup
    /// must not replace underneath it (Windows refuses to).
    lookups: usize,
}

impl Download {
    pub fn new(paths: Paths) -> Self {
        Download {
            paths,
            url_input: String::new(),
            items: Vec::new(),
            next_id: 0,
            log: Vec::new(),
            progress: BTreeMap::new(),
            files: Vec::new(),
            current: None,
            job: None,
            problem: None,
            last: None,
            repeats: 0,
            lookups: 0,
        }
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// yt-dlp is running for a download or a lookup, so the tools in
    /// `bin/` must be left alone.
    pub fn uses_tools(&self) -> bool {
        self.running() || self.lookups > 0
    }

    /// The window is closing: stop a running download together with
    /// everything yt-dlp started, instead of leaving it running with nobody
    /// to see it. Its `.part` file stays, so the next run resumes it.
    pub fn shutdown(&mut self) {
        if let Some(cancel) = &self.job {
            cancel.cancel();
        }
        self.persist();
    }

    /// Adds a line to the activity log on behalf of another screen.
    pub fn log(&mut self, note: (Level, String)) {
        self.note(note);
    }

    /// `settings` is the Settings page's current state: what a link added
    /// now is queued with, and where "Open folder" looks.
    pub fn update(&mut self, message: Message, settings: &Settings) -> Task<Message> {
        // What changes the queue itself is saved, so closing the app (or
        // it crashing) loses nothing still to do.
        let changes_queue = matches!(
            message,
            Message::AddUrl
                | Message::RemoveItem(_)
                | Message::RetryItem(_)
                | Message::ClearFinished
                | Message::ItemKind(..)
                | Message::ItemQuality(..)
                | Message::Report(Event::Finished(_))
        );
        let task = self.apply(message, settings);
        if changes_queue {
            self.persist();
        }
        task
    }

    fn apply(&mut self, message: Message, settings: &Settings) -> Task<Message> {
        match message {
            Message::UrlInput(value) => self.url_input = value,
            Message::AddUrl => return self.add_url(settings),
            Message::RemoveItem(id) => {
                self.items
                    .retain(|i| i.id != id || matches!(i.status, ItemStatus::Downloading(_)));
            }
            Message::RetryItem(id) => {
                if let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                    && matches!(item.status, ItemStatus::Failed(_))
                {
                    item.status = ItemStatus::Queued;
                }
            }
            Message::ClearFinished => {
                self.items.retain(|i| {
                    !matches!(
                        i.status,
                        ItemStatus::Completed { .. } | ItemStatus::Failed(_)
                    )
                });
            }
            // An item that ran (or is running) keeps what it ran with.
            Message::ItemKind(id, value) => {
                if let Some(item) = self.queued_item(id) {
                    item.settings.kind = value;
                }
            }
            Message::ItemQuality(id, value) => {
                if let Some(item) = self.queued_item(id) {
                    item.settings.quality = value;
                }
            }
            Message::MetadataFetched(id, result) => {
                self.lookups = self.lookups.saturating_sub(1);
                let mut thumbnail_url = None;
                if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
                    item.meta = match result {
                        Ok(meta) => {
                            thumbnail_url = meta.thumbnail_url.clone();
                            MetaState::Ready(meta)
                        }
                        Err(e) => MetaState::Failed(e),
                    };
                }
                if let Some(url) = thumbnail_url {
                    return Task::perform(fetch_thumbnail(url), move |result| {
                        Message::ThumbnailFetched(id, result)
                    });
                }
            }
            Message::ThumbnailFetched(id, result) => {
                if let Ok(bytes) = result
                    && let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                {
                    item.thumbnail = Some(image::Handle::from_bytes(bytes));
                }
            }
            Message::Start => return self.start(),
            Message::Stop => {
                if let Some(cancel) = self.job.clone() {
                    self.note((Level::Warn, "Stopping the download...".into()));
                    cancel.cancel();
                }
                return Task::none();
            }
            Message::OpenFolder => {
                let dir = settings.output_dir(&self.paths);
                if !dir.is_dir() {
                    self.note((Level::Info, "The output folder doesn't exist yet.".into()));
                } else if let Err(e) = open_folder(&dir) {
                    self.note((Level::Warn, format!("Could not open the folder: {e}")));
                }
            }
            Message::OpenItemFolder(path) => {
                if let Err(e) = reveal_file(Path::new(&path)) {
                    self.note((Level::Warn, format!("Could not open the folder: {e}")));
                }
            }
            Message::Report(event) => return self.report(event),
        }
        Task::none()
    }

    /// Adds one line to the activity log, keeping only the last
    /// [`MAX_LOG`] of them. A live recording can run for hours, and an
    /// unbounded log would grow (and slow the window down) with it.
    fn note(&mut self, entry: (Level, String)) {
        // A live recording repeats the same line over and over (yt-dlp
        // sleeps between requests and says so every time), so repeats are
        // counted on one line instead of pushing the rest out of view.
        if self.last.as_ref() == Some(&entry)
            && let Some(last) = self.log.last_mut()
        {
            self.repeats += 1;
            *last = (entry.0, format!("{}  (x{})", entry.1, self.repeats + 1));
            return;
        }
        self.last = Some(entry.clone());
        self.repeats = 0;
        self.log.push(entry);
        if self.log.len() > MAX_LOG {
            self.log.drain(..self.log.len() - MAX_LOG);
        }
    }

    fn queued_item(&mut self, id: u64) -> Option<&mut QueueItem> {
        self.items
            .iter_mut()
            .find(|i| i.id == id && i.status == ItemStatus::Queued)
    }

    /// Validates and queues what was pasted (one link, or several
    /// separated by spaces or line breaks), then kicks off a metadata
    /// lookup for each. A lookup failure (handled in `MetadataFetched`)
    /// never removes the item; it just leaves a plainer card.
    fn add_url(&mut self, settings: &Settings) -> Task<Message> {
        let input = std::mem::take(&mut self.url_input);
        let mut rejected = Vec::new();
        let mut duplicates = 0;
        let mut tasks = Vec::new();
        for url in input.split_whitespace() {
            if !valid_url(url) {
                rejected.push(url);
                continue;
            }
            if self.items.iter().any(|i| i.url == url) {
                duplicates += 1;
                continue;
            }
            tasks.push(self.push_item(url.to_string(), settings.clone(), ItemStatus::Queued));
        }

        self.problem = if !rejected.is_empty() {
            // What couldn't be queued stays in the box, to be fixed.
            self.url_input = rejected.join(" ");
            Some("That doesn't look like a link (it should start with http).".into())
        } else if input.trim().is_empty() {
            Some("Paste a link first.".into())
        } else if duplicates > 0 {
            Some(if duplicates == 1 {
                "That link is already in the queue.".into()
            } else {
                format!("{duplicates} of those links are already in the queue.")
            })
        } else {
            None
        };
        Task::batch(tasks)
    }

    /// Adds an item to the end of the queue and starts its lookup.
    fn push_item(&mut self, url: String, settings: Settings, status: ItemStatus) -> Task<Message> {
        let id = self.next_id;
        self.next_id += 1;
        let lookup = metadata::Lookup {
            cookies: settings.cookies(),
            single_video: settings.playlist == crate::config::PlaylistChoice::Single,
        };
        self.items.push(QueueItem {
            id,
            url: url.clone(),
            settings,
            meta: MetaState::Fetching,
            thumbnail: None,
            status,
        });
        self.lookups += 1;
        Task::perform(
            fetch_metadata(self.paths.clone(), url, lookup),
            move |result| Message::MetadataFetched(id, result),
        )
    }

    /// Brings back the queue the app was closed with. Items that were
    /// running are queued again (their `.part` file resumes them), failed
    /// ones keep their reason, and every card is looked up again.
    pub fn restore(&mut self) -> Task<Message> {
        let saved = config::load_queue(&self.paths);
        if !saved.is_empty() {
            self.note((
                Level::Info,
                format!("Restored {} link(s) from the last session.", saved.len()),
            ));
        }
        Task::batch(saved.into_iter().map(|item| {
            let status = match item.failed {
                Some(reason) => ItemStatus::Failed(reason),
                None => ItemStatus::Queued,
            };
            self.push_item(item.url, item.settings, status)
        }))
    }

    /// Saves what is still to do: waiting, running (as waiting) and failed
    /// items. Finished ones are done with.
    fn persist(&mut self) {
        let items: Vec<config::SavedItem> = self
            .items
            .iter()
            .filter_map(|i| {
                let failed = match &i.status {
                    ItemStatus::Completed { .. } => return None,
                    ItemStatus::Failed(reason) => Some(reason.clone()),
                    ItemStatus::Queued | ItemStatus::Downloading(_) => None,
                };
                Some(config::SavedItem {
                    url: i.url.clone(),
                    settings: i.settings.clone(),
                    failed,
                })
            })
            .collect();
        if let Err(e) = config::save_queue(&self.paths, &items) {
            self.note((Level::Warn, format!("Could not save the queue: {e}")));
        }
    }

    fn start(&mut self) -> Task<Message> {
        if self.running() {
            return Task::none();
        }
        if !self.items.iter().any(|i| i.status == ItemStatus::Queued) {
            self.problem = Some("Add at least one link first.".into());
            return Task::none();
        }
        self.problem = None;
        self.run_next()
    }

    /// Starts the next `Queued` item. An item whose per-item configuration
    /// turns out to be invalid (checked against *its own* kind/quality, not
    /// just the global defaults) is marked `Failed` and skipped, so one bad
    /// item doesn't block the rest of the queue. Does nothing if a download
    /// is already running or nothing is left to start.
    fn run_next(&mut self) -> Task<Message> {
        if self.running() {
            return Task::none();
        }
        loop {
            let Some(index) = self
                .items
                .iter()
                .position(|i| i.status == ItemStatus::Queued)
            else {
                return Task::none();
            };

            let item_settings = self.items[index].settings.clone();
            if let Some(problem) = item_settings.problems().into_iter().next() {
                self.items[index].status = ItemStatus::Failed(problem.clone());
                let url = self.items[index].url.clone();
                self.note((Level::Error, format!("{url}: {problem}")));
                continue;
            }

            let id = self.items[index].id;
            let url = self.items[index].url.clone();
            let options = item_settings.to_options(&self.paths);
            let output_dir = item_settings.output_dir(&self.paths);
            let paths = self.paths.clone();
            let cancel = Arc::new(Cancel::default());
            self.job = Some(cancel.clone());
            self.current = Some(id);
            self.items[index].status = ItemStatus::Downloading(Progress::default());
            self.progress.clear();
            self.files.clear();

            if !self.log.is_empty() {
                self.note((Level::Info, String::new()));
            }
            self.note((Level::Download, url.clone()));
            self.note((
                Level::Info,
                format!("Saving into: {}", output_dir.display()),
            ));
            if let ThumbnailPlan::Skip(ext) = thumbnail_plan(&options)
                && options.embed_thumbnail
            {
                self.note((
                    Level::Info,
                    format!("No thumbnail: a .{ext} file can't hold one."),
                ));
            }

            let (sender, receiver) = mpsc::unbounded();
            let urls = vec![url];
            thread::spawn(move || {
                runner::run(
                    &paths,
                    &options,
                    Some(&output_dir),
                    &urls,
                    &cancel,
                    &mut |event| {
                        let _ = sender.unbounded_send(event);
                    },
                );
            });
            return Task::run(receiver, Message::Report);
        }
    }

    fn report(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Log(level, line) => self.note((level, line)),
            Event::Progress(progress) => {
                self.progress.insert(progress.stream, progress);
                let combined = combined(&self.progress);
                if let Some(id) = self.current
                    && let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                {
                    item.status = ItemStatus::Downloading(combined);
                }
            }
            // Not the end of the item: a playlist reports one file per
            // video, and the item completes when yt-dlp exits.
            Event::File(path) => {
                self.note((Level::Ok, format!("Saved: {path}")));
                self.files.push(path);
            }
            Event::Finished(finish) => {
                self.job = None;
                self.progress.clear();
                let id = self.current.take();
                let (level, message) = match &finish {
                    Finish::Completed => (
                        Level::Success,
                        match self.files.len() {
                            0 => "Finished, with nothing new to download".to_string(),
                            1 => "Download completed".to_string(),
                            n => format!("Download completed ({n} files)"),
                        },
                    ),
                    Finish::StoppedAsConfigured => (
                        Level::Success,
                        "Stopped as configured (the limit or the history said so)".into(),
                    ),
                    Finish::Cancelled => (
                        Level::Warn,
                        "Download cancelled. A partial file was kept, so starting again resumes it."
                            .into(),
                    ),
                    Finish::Failed(reason) => (Level::Error, reason.clone()),
                };
                self.note((level, message));
                if let Some(id) = id
                    && let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                {
                    match &finish {
                        // The .part file is kept, so the item goes back to
                        // Queued: starting again resumes it.
                        Finish::Cancelled => item.status = ItemStatus::Queued,
                        Finish::Failed(reason) => item.status = ItemStatus::Failed(reason.clone()),
                        Finish::Completed | Finish::StoppedAsConfigured => {
                            item.status = ItemStatus::Completed {
                                path: self.files.last().cloned(),
                            }
                        }
                    }
                }
                if !matches!(finish, Finish::Cancelled) {
                    return self.run_next();
                }
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let top_bar = row![
            text_input("Paste a video or playlist link...", &self.url_input)
                .on_input(Message::UrlInput)
                .on_submit(Message::AddUrl)
                .padding(10),
            button(text("Add"))
                .padding([10, 22])
                .style(style::pill)
                .on_press(Message::AddUrl),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let queue: Element<'_, Message> = if self.items.is_empty() {
            container(
                column![
                    text("Your queue is empty.").size(16),
                    text("Paste a link above to get started.")
                        .size(13)
                        .style(|t: &Theme| text::Style {
                            color: Some(style::muted(t))
                        }),
                ]
                .spacing(6)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into()
        } else {
            scrollable(
                Column::with_children(self.items.iter().map(item_card))
                    .spacing(10)
                    .width(Length::Fill),
            )
            .height(Length::Fill)
            .into()
        };

        let mut footer = column![self.actions(), self.queue_progress()].spacing(12);
        footer = footer.push(
            column![
                text("Activity").size(15),
                log_panel::panel(
                    &self.log,
                    "Nothing yet. Every step of the download will be listed here.",
                    Length::Fixed(150.0),
                ),
            ]
            .spacing(8),
        );

        column![
            container(top_bar).padding([16, 24]),
            container(queue)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 24.0,
                    bottom: 12.0,
                    left: 24.0,
                })
                .height(Length::Fill),
            rule::horizontal(1),
            container(container(footer.max_width(900)).center_x(Length::Fill)).padding([12, 24]),
        ]
        .into()
    }

    /// A thin aggregate bar across the whole queue, the way a batch of
    /// downloads reports overall progress once each item has its own.
    fn queue_progress(&self) -> Element<'_, Message> {
        let total = self.items.len();
        if total == 0 {
            return space::vertical().height(0).into();
        }
        let done = self
            .items
            .iter()
            .filter(|i| {
                matches!(
                    i.status,
                    ItemStatus::Completed { .. } | ItemStatus::Failed(_)
                )
            })
            .count();
        let summary = if self.running() {
            format!("Downloading queue: {done} of {total} done")
        } else if done == total {
            "All done.".to_string()
        } else {
            format!("Ready: {done} of {total} done")
        };
        column![
            text(summary).size(13).style(|t: &Theme| text::Style {
                color: Some(style::muted(t))
            }),
            progress_bar(0.0..=1.0, done as f32 / total as f32)
                .girth(6)
                .style(style::progress),
        ]
        .spacing(6)
        .into()
    }

    fn actions(&self) -> Element<'_, Message> {
        let button = if self.running() {
            button(text("Stop"))
                .padding([10, 28])
                .style(style::pill_danger)
                .on_press(Message::Stop)
        } else {
            button(text("Download"))
                .padding([10, 28])
                .style(style::pill)
                .on_press(Message::Start)
        };
        let open_folder = iced::widget::button(text("Open folder"))
            .padding([10, 20])
            .style(style::pill_secondary)
            .on_press(Message::OpenFolder);
        let mut actions = row![button, open_folder]
            .spacing(12)
            .align_y(Alignment::Center);
        if self.items.iter().any(|i| {
            matches!(
                i.status,
                ItemStatus::Completed { .. } | ItemStatus::Failed(_)
            )
        }) {
            actions = actions.push(
                iced::widget::button(text("Clear finished"))
                    .padding([10, 20])
                    .style(style::pill_secondary)
                    .on_press(Message::ClearFinished),
            );
        }
        if let Some(problem) = &self.problem {
            actions = actions.push(text(problem).style(text::danger));
        } else if self.running() {
            actions = actions.push(text("Working...").style(|t: &Theme| text::Style {
                color: Some(style::muted(t)),
            }));
        }
        actions.into()
    }
}

/// One card in the queue: a thumbnail (or a placeholder while it loads or
/// on a failed lookup), the title, the two quick per-item pickers, whatever
/// metadata resolved, and this item's own status/progress.
fn item_card(item: &QueueItem) -> Element<'_, Message> {
    let id = item.id;
    let thumb: Element<'_, Message> = match &item.thumbnail {
        Some(handle) => image(handle.clone())
            .width(120)
            .height(68)
            .content_fit(iced::ContentFit::Cover)
            .into(),
        None => {
            let glyph = if matches!(item.meta, MetaState::Fetching) {
                "..."
            } else {
                "?"
            };
            // `center(Length::Fill)` would override the fixed size and
            // let a long title squeeze the placeholder away.
            container(text(glyph).size(20).style(|t: &Theme| text::Style {
                color: Some(style::muted(t)),
            }))
            .width(120)
            .height(68)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(style::card)
            .into()
        }
    };

    let title = match &item.meta {
        MetaState::Ready(meta) if !meta.title.is_empty() => meta.title.clone(),
        _ => item.url.clone(),
    };

    let details: Element<'_, Message> = match &item.meta {
        MetaState::Fetching => text("Looking up this link...")
            .size(13)
            .style(|t: &Theme| text::Style {
                color: Some(style::muted(t)),
            })
            .into(),
        MetaState::Failed(e) => text(format!("Could not look this link up ({e})"))
            .size(13)
            .style(text::warning)
            .into(),
        MetaState::Ready(meta) => {
            let mut parts = Vec::new();
            match meta.playlist {
                Some(Some(1)) => parts.push("Playlist, 1 video".to_string()),
                Some(Some(n)) => parts.push(format!("Playlist, {n} videos")),
                Some(None) => parts.push("Playlist".to_string()),
                None => {}
            }
            if let Some(seconds) = meta.duration {
                parts.push(duration(seconds.max(0.0).round() as u64));
            }
            if let Some(bytes) = meta.approx_size {
                parts.push(format!("~{:.1} MB", bytes as f64 / 1_000_000.0));
            }
            if parts.is_empty() {
                space::vertical().height(0).into()
            } else {
                text(parts.join("   "))
                    .size(13)
                    .style(|t: &Theme| text::Style {
                        color: Some(style::muted(t)),
                    })
                    .into()
            }
        }
    };

    // Only a waiting item can still change; quality means nothing for audio.
    let (kind, quality) = (item.settings.kind, item.settings.quality);
    let picks: Element<'_, Message> = if item.status == ItemStatus::Queued {
        let mut picks = row![pick_list(Kind::ALL, Some(kind), move |k| {
            Message::ItemKind(id, k)
        })]
        .spacing(8);
        if kind == Kind::VideoAudio {
            picks = picks.push(pick_list(Quality::ALL, Some(quality), move |q| {
                Message::ItemQuality(id, q)
            }));
        }
        picks.into()
    } else {
        let label = match kind {
            Kind::VideoAudio => format!("{kind}, {}", quality.to_string().to_lowercase()),
            Kind::AudioOnly => kind.to_string(),
        };
        text(label)
            .size(13)
            .style(|t: &Theme| text::Style {
                color: Some(style::muted(t)),
            })
            .into()
    };

    let status_row: Element<'_, Message> = match &item.status {
        ItemStatus::Queued => text("Queued")
            .size(13)
            .style(|t: &Theme| text::Style {
                color: Some(style::muted(t)),
            })
            .into(),
        ItemStatus::Downloading(progress) => progress_view(*progress),
        ItemStatus::Completed { path } => {
            let mut row = row![text("Completed").size(13).style(text::success)]
                .spacing(10)
                .align_y(Alignment::Center);
            if let Some(path) = path {
                row = row.push(
                    button(text("Open folder").size(13))
                        .padding([4, 12])
                        .style(style::pill_secondary)
                        .on_press(Message::OpenItemFolder(path.clone())),
                );
            }
            row.into()
        }
        ItemStatus::Failed(reason) => row![
            text(format!("Failed: {reason}"))
                .size(13)
                .style(text::danger)
                .width(Length::Fill),
            button(text("Retry").size(13))
                .padding([4, 12])
                .style(style::pill_secondary)
                .on_press(Message::RetryItem(id)),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
    };

    let removable = !matches!(item.status, ItemStatus::Downloading(_));
    let remove = button(text("Remove").size(13))
        .padding([4, 12])
        .style(style::pill_secondary)
        .on_press_maybe(removable.then_some(Message::RemoveItem(id)));

    container(
        // The text column takes what is left and wraps inside it, so a
        // long error can't push Remove (or Retry) out of the card.
        row![
            thumb,
            column![text(title).size(14), picks, details, status_row]
                .spacing(6)
                .width(Length::Fill),
            remove,
        ]
        .spacing(14)
        .align_y(Alignment::Center),
    )
    .padding(12)
    .width(Length::Fill)
    .style(style::card)
    .into()
}

/// Runs `metadata::fetch` on a worker thread and resolves once it returns,
/// the same one-shot idiom `gui::wait_on_a_thread` uses for a plain delay:
/// a blocking call has no business running on the iced executor.
fn fetch_metadata(
    paths: Paths,
    url: String,
    lookup: metadata::Lookup,
) -> impl Future<Output = Result<Metadata, String>> {
    let (tx, rx) = oneshot::channel();
    thread::spawn(move || {
        let _ = tx.send(metadata::fetch(&paths, &url, &lookup));
    });
    async move {
        rx.await
            .unwrap_or_else(|_| Err("the lookup was interrupted".into()))
    }
}

fn fetch_thumbnail(url: String) -> impl Future<Output = Result<Vec<u8>, String>> {
    let (tx, rx) = oneshot::channel();
    thread::spawn(move || {
        let _ = tx.send(metadata::fetch_thumbnail(&url));
    });
    async move {
        rx.await
            .unwrap_or_else(|_| Err("the thumbnail fetch was interrupted".into()))
    }
}

/// Opens `dir` in the system's file manager. `explorer.exe` on Windows is
/// known to return a non-zero exit status even on success, so only the
/// spawn itself is checked, never the exit code.
#[cfg(windows)]
fn open_folder(dir: &Path) -> std::io::Result<()> {
    std::process::Command::new("explorer").arg(dir).spawn()?;
    Ok(())
}

/// Waited for on a thread of its own: a child that is never waited for
/// stays behind as a zombie until the app exits, one per click.
#[cfg(unix)]
fn open_folder(dir: &Path) -> std::io::Result<()> {
    let mut child = std::process::Command::new("xdg-open").arg(dir).spawn()?;
    thread::spawn(move || child.wait());
    Ok(())
}

/// Opens the folder containing `path`, highlighting that file specifically
/// where the platform supports it.
#[cfg(windows)]
fn reveal_file(path: &Path) -> std::io::Result<()> {
    let mut arg = std::ffi::OsString::from("/select,");
    arg.push(path);
    std::process::Command::new("explorer").arg(arg).spawn()?;
    Ok(())
}

#[cfg(unix)]
fn reveal_file(path: &Path) -> std::io::Result<()> {
    open_folder(path.parent().unwrap_or(path))
}

/// A live stream fetches video and audio at the same time, so yt-dlp
/// reports two progress lines. They belong to one download as far as the
/// person watching is concerned, so they are added up.
fn combined(streams: &BTreeMap<u32, Progress>) -> Progress {
    let speed: f64 = streams.values().filter_map(|p| p.speed).sum();
    Progress {
        stream: 0,
        downloaded: streams.values().map(|p| p.downloaded).sum(),
        // Unknown as soon as one part doesn't know its size, which is the
        // usual case for a live stream.
        total: streams.values().map(|p| p.total).sum::<Option<u64>>(),
        speed: (speed > 0.0).then_some(speed),
        // 0 means "no idea" here, not "about to finish".
        eta: streams
            .values()
            .filter_map(|p| p.eta)
            .filter(|e| *e > 0)
            .max(),
        done: streams.values().all(|p| p.done),
    }
}

fn progress_view(progress: Progress) -> Element<'static, Message> {
    let mb = |bytes: u64| bytes as f64 / 1_000_000.0;
    let (amount, fraction) = match progress.total {
        Some(total) if total > 0 => {
            let fraction = (progress.downloaded as f64 / total as f64).min(1.0);
            (
                format!(
                    "{:.1} / {:.1} MB  ({:.0}%)",
                    mb(progress.downloaded),
                    mb(total),
                    fraction * 100.0
                ),
                fraction as f32,
            )
        }
        _ => (format!("{:.1} MB", mb(progress.downloaded)), 0.0),
    };
    let mut details = amount;
    if let Some(speed) = progress.speed {
        details.push_str(&format!("  at {:.1} MB/s", mb(speed as u64)));
    }
    if let Some(eta) = progress.eta.filter(|_| !progress.done) {
        details.push_str(&format!("  {} left", duration(eta)));
    }
    column![
        row![
            text(if progress.done {
                "Processing"
            } else {
                "Downloading"
            })
            .size(13),
            space::horizontal(),
            text(details).size(13).style(|t: &Theme| text::Style {
                color: Some(style::muted(t))
            }),
        ]
        .align_y(Alignment::Center),
        progress_bar(0.0..=1.0, fraction)
            .girth(8)
            .style(style::progress),
    ]
    .spacing(8)
    .into()
}

fn duration(seconds: u64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {:02}s", s / 60, s % 60),
        s => format!("{}h {:02}m", s / 3600, (s % 3600) / 60),
    }
}

/// Debug builds only: `YTP_DEV_DOWNLOAD=<link>` starts with that link
/// already typed in, so a real download can be checked without clicking.
/// Release builds ignore it.
/// A comma-separated list queues more than one link, to check that the
/// queue moves on to the next item by itself once one finishes.
pub(super) fn dev_urls() -> Vec<String> {
    if !cfg!(debug_assertions) {
        return Vec::new();
    }
    let Ok(value) = std::env::var("YTP_DEV_DOWNLOAD") else {
        return Vec::new();
    };
    value
        .split(',')
        .map(str::trim)
        .filter(|url| valid_url(url))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::tempdir;

    impl Download {
        fn item_kind(&self, index: usize) -> Kind {
            self.items[index].settings.kind
        }
    }

    /// Closing the app with links still to do used to lose them.
    #[test]
    fn the_queue_comes_back_after_a_restart() {
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        let mut screen = Download::new(paths.clone());
        let audio = Settings {
            kind: Kind::AudioOnly,
            ..Settings::default()
        };
        for url in [
            "https://example.com/a",
            "https://example.com/b",
            "https://example.com/c",
        ] {
            let _ = screen.update(Message::UrlInput(url.into()), &audio);
            let _ = screen.update(Message::AddUrl, &audio);
        }
        screen.items[1].status = ItemStatus::Completed { path: None };
        screen.items[2].status = ItemStatus::Failed("gone".into());
        let id = screen.items[0].id;
        // Any change to the queue saves it.
        let _ = screen.update(Message::ItemQuality(id, Quality::P720), &audio);

        let mut reopened = Download::new(paths);
        let _ = reopened.restore();
        let restored: Vec<(&str, &ItemStatus)> = reopened
            .items
            .iter()
            .map(|i| (i.url.as_str(), &i.status))
            .collect();
        assert_eq!(
            restored,
            [
                ("https://example.com/a", &ItemStatus::Queued),
                ("https://example.com/c", &ItemStatus::Failed("gone".into())),
            ],
            "finished items are not brought back"
        );
        assert_eq!(reopened.items[0].settings.kind, Kind::AudioOnly);
        assert_eq!(reopened.items[0].settings.quality, Quality::P720);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parallel_downloads_are_shown_as_one() {
        let streams = BTreeMap::from([
            (
                1,
                Progress {
                    stream: 1,
                    downloaded: 20_000,
                    total: Some(50_000),
                    speed: Some(1_000.0),
                    eta: Some(30),
                    done: false,
                },
            ),
            (
                2,
                Progress {
                    stream: 2,
                    downloaded: 5_000,
                    // A live stream doesn't know its size.
                    total: None,
                    speed: Some(500.0),
                    eta: Some(0),
                    done: false,
                },
            ),
        ]);
        let all = combined(&streams);
        assert_eq!(all.downloaded, 25_000);
        assert_eq!(all.total, None, "one unknown size makes the whole unknown");
        assert_eq!(all.speed, Some(1_500.0));
        assert_eq!(all.eta, Some(30), "a zero eta means unknown, not now");
        assert!(!all.done);
    }

    #[test]
    fn a_line_yt_dlp_keeps_repeating_is_collapsed() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        for _ in 0..3 {
            screen.note((Level::Info, "[youtube] Sleeping 1.5 seconds ...".into()));
        }
        screen.note((Level::Info, "[download] Destination: /tmp/v.mkv".into()));
        assert_eq!(screen.log.len(), 2);
        assert_eq!(screen.log[0].1, "[youtube] Sleeping 1.5 seconds ...  (x3)");
        assert_eq!(screen.log[1].1, "[download] Destination: /tmp/v.mkv");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_activity_log_keeps_only_the_most_recent_lines() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        for n in 0..MAX_LOG + 50 {
            screen.note((Level::Info, format!("line {n}")));
        }
        assert_eq!(screen.log.len(), MAX_LOG);
        // The newest line survives, the oldest ones are gone.
        assert_eq!(
            screen.log.last().unwrap().1,
            format!("line {}", MAX_LOG + 49)
        );
        assert_eq!(screen.log.first().unwrap().1, "line 50");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn adding_a_url_queues_it_and_starts_a_metadata_lookup() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let _ = screen.update(
            Message::UrlInput("https://example.com/video".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        assert_eq!(screen.items.len(), 1);
        assert_eq!(screen.items[0].url, "https://example.com/video");
        assert_eq!(screen.items[0].status, ItemStatus::Queued);
        assert_eq!(screen.items[0].meta, MetaState::Fetching);
        assert_eq!(screen.url_input, "", "the input clears once queued");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_invalid_url_is_rejected_without_touching_the_queue() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let _ = screen.update(Message::UrlInput("not a link".into()), &Settings::default());
        let _ = screen.update(Message::AddUrl, &Settings::default());
        assert!(screen.items.is_empty());
        assert!(screen.problem.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_duplicate_url_is_not_queued_twice() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let _ = screen.update(
            Message::UrlInput("https://example.com/video".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        let _ = screen.update(
            Message::UrlInput("https://example.com/video".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        assert_eq!(screen.items.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn metadata_and_a_thumbnail_fill_the_card_in_once_they_resolve() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let _ = screen.update(
            Message::UrlInput("https://example.com/video".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        let id = screen.items[0].id;

        let meta = Metadata {
            title: "A video".into(),
            thumbnail_url: Some("https://example.com/thumb.jpg".into()),
            duration: Some(90.0),
            approx_size: Some(5_000_000),
            playlist: None,
        };
        let _ = screen.update(
            Message::MetadataFetched(id, Ok(meta.clone())),
            &Settings::default(),
        );
        assert_eq!(screen.items[0].meta, MetaState::Ready(meta));

        let _ = screen.update(
            Message::ThumbnailFetched(id, Ok(vec![1, 2, 3])),
            &Settings::default(),
        );
        assert!(screen.items[0].thumbnail.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_lookup_leaves_the_item_queued_with_a_plainer_card() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let _ = screen.update(
            Message::UrlInput("https://example.com/video".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        let id = screen.items[0].id;

        let _ = screen.update(
            Message::MetadataFetched(id, Err("network error".into())),
            &Settings::default(),
        );
        assert_eq!(
            screen.items[0].meta,
            MetaState::Failed("network error".into())
        );
        assert_eq!(
            screen.items[0].status,
            ItemStatus::Queued,
            "still queueable"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn removing_an_item_drops_it_from_the_queue() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let _ = screen.update(
            Message::UrlInput("https://example.com/a".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        let _ = screen.update(
            Message::UrlInput("https://example.com/b".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        let first_id = screen.items[0].id;

        let _ = screen.update(Message::RemoveItem(first_id), &Settings::default());
        assert_eq!(screen.items.len(), 1);
        assert_eq!(screen.items[0].url, "https://example.com/b");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Several links pasted at once used to become one "link" with spaces
    /// or line breaks inside, which yt-dlp then failed on.
    #[test]
    fn several_pasted_links_are_queued_one_by_one() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let _ = screen.update(
            Message::UrlInput(
                "https://example.com/a https://example.com/b\nhttps://example.com/c".into(),
            ),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        let urls: Vec<&str> = screen.items.iter().map(|i| i.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://example.com/a",
                "https://example.com/b",
                "https://example.com/c"
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_duplicate_link_says_so_instead_of_vanishing() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        for _ in 0..2 {
            let _ = screen.update(
                Message::UrlInput("https://example.com/video".into()),
                &Settings::default(),
            );
            let _ = screen.update(Message::AddUrl, &Settings::default());
        }
        assert_eq!(screen.items.len(), 1);
        assert!(
            screen
                .problem
                .as_deref()
                .is_some_and(|p| p.contains("already")),
            "{:?}",
            screen.problem
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A playlist reports one file per video. The first one used to mark
    /// the whole item Completed while the rest were still downloading.
    #[test]
    fn a_finished_file_does_not_complete_an_item_that_is_still_running() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let _ = screen.update(
            Message::UrlInput("https://example.com/list".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        let _ = screen.update(Message::Start, &Settings::default());
        let _ = screen.update(
            Message::Report(Event::File("/v/1.mkv".into())),
            &Settings::default(),
        );
        assert!(
            matches!(screen.items[0].status, ItemStatus::Downloading(_)),
            "{:?}",
            screen.items[0].status
        );
        let _ = screen.update(
            Message::Report(Event::File("/v/2.mkv".into())),
            &Settings::default(),
        );
        let _ = screen.update(
            Message::Report(Event::Finished(Finish::Completed)),
            &Settings::default(),
        );
        assert_eq!(
            screen.items[0].status,
            ItemStatus::Completed {
                path: Some("/v/2.mkv".into())
            }
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_item_that_already_ran_keeps_the_choices_it_ran_with() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let _ = screen.update(
            Message::UrlInput("https://example.com/a".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        screen.items[0].status = ItemStatus::Completed { path: None };
        let id = screen.items[0].id;
        let _ = screen.update(Message::ItemKind(id, Kind::AudioOnly), &Settings::default());
        assert_eq!(screen.item_kind(0), Kind::VideoAudio);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The Settings page says its values apply to links queued from then
    /// on. They used to be read when the item's turn came instead, so a
    /// change made while the queue waited rewrote items already in it.
    /// Checked black box: a stand-in yt-dlp records the argv it was given.
    #[cfg(unix)]
    #[test]
    fn a_queued_link_keeps_the_settings_it_was_queued_with() {
        use std::os::unix::fs::PermissionsExt;
        let _guard = crate::testing::exec_guard();
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        std::fs::create_dir_all(&paths.bin_dir).unwrap();
        let argv = dir.join("argv");
        let script = paths.tool("yt-dlp");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > {}\n",
                argv.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut screen = Download::new(paths);
        let queued_with = Settings::default();
        let _ = screen.update(
            Message::UrlInput("https://example.com/a".into()),
            &queued_with,
        );
        let _ = screen.update(Message::AddUrl, &queued_with);
        // Changed on the Settings page after the link was queued.
        let changed = Settings {
            sponsor: crate::config::Sponsor::Mark,
            ..Settings::default()
        };
        let _ = screen.update(Message::Start, &changed);
        let mut recorded = String::new();
        for _ in 0..100 {
            recorded = std::fs::read_to_string(&argv).unwrap_or_default();
            if recorded.contains("\n--\n") {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            recorded.contains("\n--\n"),
            "yt-dlp never ran: {recorded:?}"
        );
        assert!(
            !recorded.contains("--sponsorblock-mark"),
            "a setting changed after queueing leaked into the item"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn per_item_kind_and_quality_can_be_changed_independently() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let _ = screen.update(
            Message::UrlInput("https://example.com/a".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        let _ = screen.update(
            Message::UrlInput("https://example.com/b".into()),
            &Settings::default(),
        );
        let _ = screen.update(Message::AddUrl, &Settings::default());
        let a = screen.items[0].id;

        let _ = screen.update(Message::ItemKind(a, Kind::AudioOnly), &Settings::default());
        let _ = screen.update(Message::ItemQuality(a, Quality::P720), &Settings::default());
        assert_eq!(screen.items[0].settings.kind, Kind::AudioOnly);
        assert_eq!(screen.items[0].settings.quality, Quality::P720);
        // The other item is untouched.
        assert_eq!(screen.items[1].settings.kind, Kind::VideoAudio);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
