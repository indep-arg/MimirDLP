//! The Download screen: the queue and the activity log.
//!
//! Adding a link queues it at once and starts a lookup (`metadata::fetch`,
//! then the thumbnail) that fills its card in; a failed lookup only leaves
//! a plainer card. Each item keeps a copy of the `Settings` it was queued
//! with, and its kind and quality can be changed on the card while it
//! waits. Items download one at a time, in order, through `runner`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::thread;

use iced::futures::channel::mpsc;
use iced::widget::{
    Column, button, column, container, image, progress_bar, row, scrollable, space, text,
};
use iced::{Alignment, Element, Length, Task};
use tr::tr;

use super::{amount, dropdown, entry, in_background, log as log_panel, style};
use crate::config::{self, Kind, Quality, Settings, valid_url};
use crate::engine::{ThumbnailPlan, thumbnail_plan};
use crate::log::Level;
use crate::metadata::{self, Metadata};
use crate::provision::Paths;
use crate::runner::{self, Cancel, Event, Finish, Progress};

/// How many activity lines are kept: enough for a whole run, and a limit
/// for a recording that goes on for hours.
const MAX_LOG: usize = 2000;

#[derive(Debug, Clone)]
pub enum Message {
    UrlInput(String),
    AddUrl,
    /// The clipboard, read when the window got the focus.
    Clipboard(Option<String>),
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
    /// What opening a folder came to.
    FolderOpened(Result<(), String>),
    /// Shows or hides the activity log, which is folded away by default.
    ToggleLog,
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

impl ItemStatus {
    fn is_finished(&self) -> bool {
        matches!(self, ItemStatus::Completed { .. } | ItemStatus::Failed(_))
    }
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
    show_log: bool,
    /// The clipboard text last put into the link field, so a link the
    /// user cleared isn't put back every time the window gets the focus.
    offered: Option<String>,
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
            show_log: false,
            offered: None,
        }
    }

    /// The link field is empty, so a copied link may be offered in it.
    pub fn wants_clipboard(&self) -> bool {
        self.url_input.trim().is_empty()
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
            Message::Clipboard(text) => self.offer(text.as_deref().unwrap_or_default()),
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
            Message::ToggleLog => self.show_log = !self.show_log,
            Message::ClearFinished => {
                self.items.retain(|i| !i.status.is_finished());
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
                    return Task::perform(
                        in_background(move || metadata::fetch_thumbnail(&url)),
                        move |result| {
                            Message::ThumbnailFetched(id, result.unwrap_or(Err(String::new())))
                        },
                    );
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
                    self.log((Level::Warn, tr!("Stopping the download...")));
                    cancel.cancel();
                }
                return Task::none();
            }
            Message::OpenFolder => {
                let dir = settings.output_dir(&self.paths);
                if !dir.is_dir() {
                    self.log((Level::Info, tr!("The output folder doesn't exist yet.")));
                } else {
                    return super::open::open(dir).map(Message::FolderOpened);
                }
            }
            Message::OpenItemFolder(path) => {
                return super::open::reveal(Path::new(&path)).map(Message::FolderOpened);
            }
            Message::FolderOpened(result) => {
                if let Err(e) = result {
                    self.log((Level::Warn, tr!("Could not open the folder: {e}", e)));
                }
            }
            Message::Report(event) => return self.report(event),
        }
        Task::none()
    }

    /// Adds a line to the activity log, keeping the last [`MAX_LOG`]. A
    /// line repeated right away (yt-dlp says so every time it sleeps during
    /// a live recording) is counted on the line before instead.
    pub fn log(&mut self, entry: (Level, String)) {
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

    /// A link copied somewhere else is put into the empty link field, ready
    /// for Add: it is never queued by itself. Only links (one, or several
    /// separated by spaces or line breaks), not ones already in the queue,
    /// and not the same text twice.
    fn offer(&mut self, text: &str) {
        let text = text.trim();
        let links: Vec<&str> = text.split_whitespace().collect();
        if !self.wants_clipboard()
            || links.is_empty()
            || !links.iter().all(|l| valid_url(l))
            || links.iter().all(|l| self.items.iter().any(|i| i.url == *l))
            || self.offered.as_deref() == Some(text)
        {
            return;
        }
        self.offered = Some(text.to_string());
        self.url_input = text.to_string();
        self.problem = None;
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
            Some(tr!(
                "That doesn't look like a link (it should start with http)."
            ))
        } else if input.trim().is_empty() {
            Some(tr!("Paste a link first."))
        } else if duplicates > 0 {
            Some(if duplicates == 1 {
                tr!("That link is already in the queue.")
            } else {
                tr!("{n} of those links is already in the queue."
                    | "{n} of those links are already in the queue." % duplicates)
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
            cookies: settings.cookies(&self.paths),
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
        let paths = self.paths.clone();
        Task::perform(
            in_background(move || metadata::fetch(&paths, &url, &lookup)),
            move |result| {
                let result = result.unwrap_or_else(|| Err(tr!("the lookup was interrupted")));
                Message::MetadataFetched(id, result)
            },
        )
    }

    /// Brings back the queue the app was closed with. Items that were
    /// running are queued again (their `.part` file resumes them), failed
    /// ones keep their reason, and every card is looked up again.
    pub fn restore(&mut self) -> Task<Message> {
        let saved = config::load_queue(&self.paths);
        if !saved.is_empty() {
            self.log((
                Level::Info,
                tr!("Restored {n} link from the last session."
                    | "Restored {n} links from the last session." % saved.len()),
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
            self.log((Level::Warn, tr!("Could not save the queue: {e}", e)));
        }
    }

    fn start(&mut self) -> Task<Message> {
        if self.running() {
            return Task::none();
        }
        if !self.items.iter().any(|i| i.status == ItemStatus::Queued) {
            self.problem = Some(tr!("Add at least one link first."));
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
                self.log((Level::Error, format!("{url}: {problem}")));
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
                self.log((Level::Info, String::new()));
            }
            self.log((Level::Download, url.clone()));
            self.log((
                Level::Info,
                tr!("Saving into: {folder}", folder = output_dir.display()),
            ));
            if let ThumbnailPlan::Skip(ext) = thumbnail_plan(&options)
                && options.embed_thumbnail
            {
                self.log((
                    Level::Info,
                    tr!("No thumbnail: a .{ext} file can't hold one.", ext),
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
            Event::Log(level, line) => self.log((level, line)),
            Event::Progress(progress) => {
                self.progress.insert(progress.stream, progress);
                let combined = combine(&self.progress);
                if let Some(id) = self.current
                    && let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                {
                    item.status = ItemStatus::Downloading(combined);
                }
            }
            // Not the end of the item: a playlist reports one file per
            // video, and the item completes when yt-dlp exits.
            Event::File(path) => {
                self.log((Level::Ok, tr!("Saved: {path}", path)));
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
                            0 => tr!("Finished, with nothing new to download"),
                            1 => tr!("Download completed"),
                            files => tr!("Download completed ({n} file)"
                                | "Download completed ({n} files)" % files),
                        },
                    ),
                    Finish::StoppedAsConfigured => (
                        Level::Success,
                        tr!("Stopped as configured (the limit or the history said so)"),
                    ),
                    Finish::Cancelled => (
                        Level::Warn,
                        tr!(
                            "Download cancelled. A partial file was kept, so starting again resumes it."
                        ),
                    ),
                    Finish::Failed(reason) => (Level::Error, reason.clone()),
                };
                self.log((level, message));
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
            entry(&tr!("Paste a video or playlist link..."), &self.url_input)
                .on_input(Message::UrlInput)
                .on_submit(Message::AddUrl)
                .padding(10),
            button(text(tr!("Add")))
                .padding([10, 22])
                .style(style::pill)
                .on_press(Message::AddUrl),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let queue: Element<'_, Message> = if self.items.is_empty() {
            // Adwaita's status page: what this screen is for, said once.
            container(
                column![
                    text(tr!("Download a video")).size(24).font(style::BOLD),
                    text(tr!(
                        "Paste a link from YouTube or almost any other site above."
                    ))
                    .style(style::muted_text),
                ]
                .spacing(8)
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

        let mut page = column![top_bar, queue, self.queue_progress(), self.actions()]
            .spacing(16)
            .max_width(800);
        if self.show_log {
            page = page.push(log_panel::panel(
                &self.log,
                tr!("Nothing yet. Every step of the download will be listed here."),
                Length::Fixed(150.0),
            ));
        }
        container(page.padding(24).height(Length::Fill))
            .center_x(Length::Fill)
            .into()
    }

    /// How much of the whole queue is done.
    fn queue_progress(&self) -> Element<'_, Message> {
        let total = self.items.len();
        if total == 0 {
            return space::vertical().height(0).into();
        }
        let done = self.items.iter().filter(|i| i.status.is_finished()).count();
        let summary = if self.running() {
            tr!("Downloading queue: {done} of {total} done", done, total)
        } else if done == total {
            tr!("All done.")
        } else {
            tr!("Ready: {done} of {total} done", done, total)
        };
        column![
            text(summary).size(13).style(style::muted_text),
            progress_bar(0.0..=1.0, done as f32 / total as f32)
                .girth(6)
                .style(style::progress),
        ]
        .spacing(6)
        .into()
    }

    fn actions(&self) -> Element<'_, Message> {
        let button = if self.running() {
            button(text(tr!("Stop")))
                .padding([10, 28])
                .style(style::pill_danger)
                .on_press(Message::Stop)
        } else {
            button(text(tr!("Download")))
                .padding([10, 28])
                .style(style::pill)
                .on_press(Message::Start)
        };
        let open_folder = iced::widget::button(text(tr!("Open folder")))
            .padding([10, 20])
            .style(style::pill_secondary)
            .on_press(Message::OpenFolder);
        let log_toggle = iced::widget::button(text(if self.show_log {
            tr!("Hide activity")
        } else {
            tr!("Show activity")
        }))
        .padding([10, 16])
        .style(style::flat(self.show_log))
        .on_press(Message::ToggleLog);
        let mut actions = row![button, open_folder]
            .spacing(12)
            .align_y(Alignment::Center);
        if self.items.iter().any(|i| i.status.is_finished()) {
            actions = actions.push(
                iced::widget::button(text(tr!("Clear finished")))
                    .padding([10, 20])
                    .style(style::pill_secondary)
                    .on_press(Message::ClearFinished),
            );
        }
        if let Some(problem) = &self.problem {
            actions = actions.push(text(problem).style(text::danger));
        } else if self.running() {
            actions = actions.push(text(tr!("Working...")).style(style::muted_text));
        }
        actions.push(space::horizontal()).push(log_toggle).into()
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
            container(text(glyph).size(20).style(style::muted_text))
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
        MetaState::Fetching => text(tr!("Looking up this link..."))
            .size(13)
            .style(style::muted_text)
            .into(),
        MetaState::Failed(e) => text(tr!("Could not look this link up ({e})", e))
            .size(13)
            .style(text::warning)
            .into(),
        MetaState::Ready(meta) => {
            let mut parts = Vec::new();
            match meta.playlist {
                Some(Some(count)) => {
                    parts.push(tr!("Playlist, {n} video" | "Playlist, {n} videos" % count))
                }
                Some(None) => parts.push(tr!("Playlist")),
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
                    .style(style::muted_text)
                    .into()
            }
        }
    };

    // Only a waiting item can still change; quality means nothing for audio.
    let (kind, quality) = (item.settings.kind, item.settings.quality);
    let picks: Element<'_, Message> = if item.status == ItemStatus::Queued {
        let mut picks = row![dropdown(Kind::ALL, Some(kind), move |k| {
            Message::ItemKind(id, k)
        })]
        .spacing(8);
        if kind == Kind::VideoAudio {
            picks = picks.push(dropdown(Quality::ALL, Some(quality), move |q| {
                Message::ItemQuality(id, q)
            }));
        }
        picks.into()
    } else {
        let label = match kind {
            Kind::VideoAudio => tr!(
                "{kind}, {quality}",
                kind,
                quality = quality.to_string().to_lowercase()
            ),
            Kind::AudioOnly => kind.to_string(),
        };
        text(label).size(13).style(style::muted_text).into()
    };

    let status_row: Element<'_, Message> = match &item.status {
        ItemStatus::Queued => text(tr!("Queued")).size(13).style(style::muted_text).into(),
        ItemStatus::Downloading(progress) => progress_view(*progress),
        // No file: yt-dlp skipped it (already in the history, or the file
        // is already there).
        ItemStatus::Completed { path: None } => text(tr!("Nothing new to download"))
            .size(13)
            .style(style::muted_text)
            .into(),
        ItemStatus::Completed { path: Some(path) } => row![
            text(tr!("Completed")).size(13).style(text::success),
            button(text(tr!("Open folder")).size(13))
                .padding([4, 12])
                .style(style::pill_secondary)
                .on_press(Message::OpenItemFolder(path.clone())),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
        ItemStatus::Failed(reason) => row![
            text(tr!("Failed: {reason}", reason))
                .size(13)
                .style(text::danger)
                .width(Length::Fill),
            button(text(tr!("Retry")).size(13))
                .padding([4, 12])
                .style(style::pill_secondary)
                .on_press(Message::RetryItem(id)),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
    };

    let removable = !matches!(item.status, ItemStatus::Downloading(_));
    let remove = button(text(tr!("Remove")).size(13))
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

/// A live stream is fetched as video and audio at once, so yt-dlp reports
/// two progress lines for one download. They are added up.
fn combine(streams: &BTreeMap<u32, Progress>) -> Progress {
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
    let (mut details, fraction) = amount(progress.downloaded, progress.total);
    if let Some(speed) = progress.speed {
        details.push_str("  ");
        details.push_str(&tr!(
            "at {speed} MB/s",
            speed = format!("{:.1}", mb(speed as u64))
        ));
    }
    if let Some(eta) = progress.eta.filter(|_| !progress.done) {
        details.push_str("  ");
        details.push_str(&tr!("{time} left", time = duration(eta)));
    }
    column![
        row![
            text(if progress.done {
                tr!("Processing")
            } else {
                tr!("Downloading")
            })
            .size(13),
            space::horizontal(),
            text(details).size(13).style(style::muted_text),
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

/// Debug builds only: `YTP_DEV_DOWNLOAD=<link>[,<link>...]` queues those
/// links and starts, so a real download can be checked without clicking.
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
        let all = combine(&streams);
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
            screen.log((Level::Info, "[youtube] Sleeping 1.5 seconds ...".into()));
        }
        screen.log((Level::Info, "[download] Destination: /tmp/v.mkv".into()));
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
            screen.log((Level::Info, format!("line {n}")));
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
    fn a_copied_link_is_offered_in_the_empty_field_but_never_queued() {
        let dir = tempdir();
        let mut screen = Download::new(Paths::new(dir.clone()));
        let settings = Settings::default();
        let link = "https://example.com/video";
        let clipboard = |text: &str| Message::Clipboard(Some(text.into()));

        // Not a link: left alone.
        let _ = screen.update(clipboard("some copied words"), &settings);
        assert_eq!(screen.url_input, "");
        // A link: offered, not queued.
        let _ = screen.update(clipboard(&format!("  {link}\n")), &settings);
        assert_eq!(screen.url_input, link);
        assert!(screen.items.is_empty());
        // Cleared by the user: not put back for the same clipboard.
        let _ = screen.update(Message::UrlInput(String::new()), &settings);
        let _ = screen.update(clipboard(link), &settings);
        assert_eq!(screen.url_input, "");
        // Something typed is never replaced.
        let _ = screen.update(
            Message::UrlInput("https://typed.example/x".into()),
            &settings,
        );
        let _ = screen.update(clipboard("https://example.com/other"), &settings);
        assert_eq!(screen.url_input, "https://typed.example/x");
        // Already in the queue: not offered again.
        let _ = screen.update(Message::AddUrl, &settings);
        let _ = screen.update(clipboard("https://typed.example/x"), &settings);
        assert_eq!(screen.url_input, "");
        // Several links at once, as pasting them would.
        let _ = screen.update(
            clipboard("https://a.example/1 https://b.example/2"),
            &settings,
        );
        assert_eq!(screen.url_input, "https://a.example/1 https://b.example/2");
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
