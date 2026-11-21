//! The picture of the app the window draws: plain data, as JSON.
//!
//! [`Snapshot`] is everything that changes while the app runs and is sent
//! whenever it does; [`Fixed`] is what only changes with the language. The
//! activity logs are left out of both and asked for line by line
//! ([`lines_since`]), since a log can be two thousand lines long.
//!
//! Choices travel as the keys the settings file uses (`video`, `1080`,
//! `nightly`), states as lower case words. Texts that depend on a state are
//! worded by the window; the ones that come from here (a failure's reason,
//! a setting's problem) are already in the interface's language.

use std::collections::BTreeMap;

use serde::Serialize;

use super::log::Log;
use super::queue::{ItemStatus, MetaState, QueueItem};
use super::setup::{Job, Row};
use super::{App, Queue, SettingsPage, Setup};
use crate::log::Level;
use crate::provision::{Channel, Status, UpdateCheck};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// A newer Osprey, when the daily look found one.
    pub app_update: Option<String>,
    pub can_download: bool,
    /// Every tool's file was there when the app opened.
    pub tools_present: bool,
    /// The first inspection hasn't said yet whether those tools run.
    pub confirming_tools: bool,
    pub queue: QueueView,
    pub setup: SetupView,
    pub settings: SettingsView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueView {
    pub items: Vec<ItemView>,
    /// What the link field should hold after the last Add or clipboard
    /// offer: empty, the links that couldn't be queued, or a copied link.
    pub input: String,
    pub problem: Option<String>,
    pub running: bool,
    /// The running download is a live stream: Stop saves what it got.
    pub recording: bool,
    /// yt-dlp is running, for a download or a lookup.
    pub uses_tools: bool,
    /// How far the whole queue is, from 0 to 1.
    pub fraction: f32,
    pub done: usize,
    pub has_queued: bool,
    pub has_finished: bool,
    pub log_revision: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: u64,
    pub url: String,
    pub title: Option<String>,
    /// `fetching`, `ready` or `failed`.
    pub lookup: &'static str,
    pub lookup_error: Option<String>,
    pub playlist: bool,
    pub playlist_count: Option<u64>,
    /// Seconds.
    pub duration: Option<f64>,
    /// Bytes, approximate.
    pub size: Option<u64>,
    pub thumbnail: bool,
    pub kind: &'static str,
    pub quality: &'static str,
    /// `queued`, `downloading`, `completed` or `failed`.
    pub status: &'static str,
    pub progress: Option<ProgressView>,
    /// The finished file. A completed item without one had nothing new to
    /// download.
    pub path: Option<String>,
    pub error: Option<String>,
    /// It is the live stream being recorded.
    pub live: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressView {
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Bytes per second.
    pub speed: Option<f64>,
    /// Seconds left.
    pub eta: Option<u64>,
    /// Downloaded, now being merged or converted.
    pub processing: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupView {
    pub components: Vec<ComponentView>,
    pub channel: &'static str,
    /// `inspecting`, `installing` or `checking`.
    pub job: Option<&'static str>,
    pub ready: bool,
    pub needs_install: bool,
    /// The tools came with the package and are only shown.
    pub bundled: bool,
    /// Installed by a package: the files are in the user's data folder.
    pub installed: bool,
    pub bin_dir: String,
    pub confirming_reinstall: bool,
    pub transfer: Option<TransferView>,
    /// The first launch's welcome is showing instead of the full screen.
    pub welcoming: bool,
    pub first_launch: bool,
    /// Which of the components the welcome is getting, from 1.
    pub welcome_step: usize,
    pub welcome_fraction: f32,
    pub ytdlp_version: Option<String>,
    /// Days, once yt-dlp is old enough to say so.
    pub ytdlp_age: Option<i64>,
    pub last_error: Option<String>,
    pub log_revision: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentView {
    pub name: &'static str,
    /// `checking`, `missing`, `broken` or `installed`.
    pub status: &'static str,
    pub version: Option<String>,
    /// `current`, `outdated`, `unknown` or `failed`, once checked.
    pub update: Option<&'static str>,
    /// Days, for a yt-dlp old enough to say so.
    pub stale_age: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferView {
    pub file: String,
    pub received: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// Every setting by its key in the settings file.
    pub values: BTreeMap<&'static str, String>,
    /// A video password is set for the links added next.
    pub video_password: bool,
    /// How many videos the history holds, when it exists.
    pub history: Option<usize>,
    pub auto_subs_problem: Option<String>,
    /// Linux: Osprey is in the applications menu.
    pub desktop_entry: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fixed {
    pub version: &'static str,
    /// The language the interface is in, as a code from `languages`.
    pub language: &'static str,
    pub languages: Vec<Choice>,
    /// What each choice among the settings offers, by the setting's key.
    pub choices: BTreeMap<&'static str, Vec<Choice>>,
    pub channels: Vec<Choice>,
    pub linux: bool,
    pub installed: bool,
    pub bundled: bool,
    pub default_folder: String,
    /// The translations of the interface's own texts; none for English.
    pub catalog: Option<crate::i18n::Exported>,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct LineView {
    pub seq: u64,
    pub level: &'static str,
    pub text: String,
}

pub fn lines_since(log: &Log, seq: u64) -> Vec<LineView> {
    log.since(seq)
        .map(|line| LineView {
            seq: line.seq,
            level: level(line.level),
            text: line.text.clone(),
        })
        .collect()
}

fn level(level: Level) -> &'static str {
    match level {
        Level::Info => "info",
        Level::Install => "install",
        Level::Download => "download",
        Level::Verify => "verify",
        Level::Ok => "ok",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Success => "success",
    }
}

impl App {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            app_update: self.app_update.clone(),
            can_download: self.can_download(),
            tools_present: self.tools_present,
            confirming_tools: self.confirming_tools,
            queue: queue(&self.queue),
            setup: setup(&self.setup),
            settings: settings(&self.settings),
        }
    }

    pub fn fixed(&self) -> Fixed {
        let paths = self.settings.paths();
        let choice = |(key, label): (&str, String)| Choice {
            key: key.to_string(),
            label,
        };
        Fixed {
            version: env!("CARGO_PKG_VERSION"),
            language: crate::i18n::resolve(&self.settings.settings().language).code,
            languages: crate::i18n::LANGUAGES
                .iter()
                .map(|l| choice((l.code, l.name.to_string())))
                .collect(),
            choices: crate::config::choices()
                .into_iter()
                .map(|(setting, options)| (setting, options.into_iter().map(choice).collect()))
                .collect(),
            channels: Channel::ALL
                .into_iter()
                .map(|c| choice((c.key(), c.to_string())))
                .collect(),
            linux: cfg!(target_os = "linux"),
            installed: paths.installed,
            bundled: paths.bundled_tools(),
            default_folder: self.settings.default_folder_hint(),
            catalog: crate::i18n::export(&self.settings.settings().language),
        }
    }
}

fn queue(queue: &Queue) -> QueueView {
    let items = queue.items();
    QueueView {
        items: items
            .iter()
            .map(|i| item(i, queue.recording() && queue.current() == Some(i.id)))
            .collect(),
        input: queue.url_input().to_string(),
        problem: queue.problem().map(str::to_string),
        running: queue.running(),
        recording: queue.recording(),
        uses_tools: queue.uses_tools(),
        fraction: queue.fraction(),
        done: items.iter().filter(|i| i.status.is_finished()).count(),
        has_queued: items.iter().any(|i| i.status == ItemStatus::Queued),
        has_finished: items.iter().any(|i| i.status.is_finished()),
        log_revision: queue.activity().revision(),
    }
}

fn item(item: &QueueItem, live: bool) -> ItemView {
    let meta = match &item.meta {
        MetaState::Ready(meta) => Some(meta),
        _ => None,
    };
    let (status, progress, path, error) = match &item.status {
        ItemStatus::Queued => ("queued", None, None, None),
        ItemStatus::Downloading(p) => (
            "downloading",
            Some(ProgressView {
                downloaded: p.downloaded,
                total: p.total,
                speed: p.speed,
                eta: p.eta,
                processing: p.done,
            }),
            None,
            None,
        ),
        ItemStatus::Completed { path } => ("completed", None, path.clone(), None),
        ItemStatus::Failed(reason) => ("failed", None, None, Some(reason.clone())),
    };
    ItemView {
        id: item.id,
        url: item.url.clone(),
        title: meta.map(|m| m.title.clone()).filter(|t| !t.is_empty()),
        lookup: match &item.meta {
            MetaState::Fetching => "fetching",
            MetaState::Ready(_) => "ready",
            MetaState::Failed(_) => "failed",
        },
        lookup_error: match &item.meta {
            MetaState::Failed(e) => Some(e.clone()),
            _ => None,
        },
        playlist: meta.is_some_and(|m| m.playlist.is_some()),
        playlist_count: meta.and_then(|m| m.playlist.flatten()),
        duration: meta.and_then(|m| m.duration),
        size: meta.and_then(|m| m.approx_size),
        thumbnail: item.thumbnail.is_some(),
        kind: item.settings.kind.key(),
        quality: item.settings.quality.key(),
        status,
        progress,
        path,
        error,
        live,
    }
}

fn setup(setup: &Setup) -> SetupView {
    let (welcome_step, welcome_fraction) = setup.welcome_progress();
    SetupView {
        components: setup
            .rows()
            .map(|(component, row)| ComponentView {
                name: component.label(),
                status: match &row.status {
                    None => "checking",
                    Some(Status::Missing) => "missing",
                    Some(Status::Broken) => "broken",
                    Some(Status::Installed { .. }) => "installed",
                },
                version: match &row.status {
                    Some(Status::Installed { version }) => Some(version.clone()),
                    _ => None,
                },
                update: update(row),
                stale_age: setup.stale_age(component),
            })
            .collect(),
        channel: setup.channel().key(),
        job: setup.job().map(|job| match job {
            Job::Inspecting => "inspecting",
            Job::Installing(_) => "installing",
            Job::CheckingUpdates => "checking",
        }),
        ready: setup.ready(),
        needs_install: setup.needs_install(),
        bundled: setup.bundled(),
        installed: setup.paths().installed,
        bin_dir: setup.paths().bin_dir.display().to_string(),
        confirming_reinstall: setup.confirming_reinstall(),
        transfer: setup.transfer().map(|t| TransferView {
            file: t.file.clone(),
            received: t.received,
            total: t.total,
        }),
        welcoming: setup.welcoming(),
        first_launch: setup.first_launch(),
        welcome_step,
        welcome_fraction,
        ytdlp_version: setup.ytdlp_version().map(str::to_string),
        ytdlp_age: setup.ytdlp_age(),
        last_error: setup.activity().latest(Level::Error).map(str::to_string),
        log_revision: setup.activity().revision(),
    }
}

fn update(row: &Row) -> Option<&'static str> {
    Some(match row.update.as_ref()? {
        UpdateCheck::Current => "current",
        UpdateCheck::Outdated => "outdated",
        UpdateCheck::Unknown => "unknown",
        UpdateCheck::Failed(_) => "failed",
    })
}

fn settings(page: &SettingsPage) -> SettingsView {
    let settings = page.settings();
    SettingsView {
        values: settings.pairs().into_iter().collect(),
        video_password: !settings.video_password.is_empty(),
        history: page.history(),
        auto_subs_problem: settings.auto_subs_problem(),
        desktop_entry: page.desktop_entry(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::provision::Paths;
    use crate::testing::tempdir;

    /// The window is written against these names, so a renamed field must
    /// fail here rather than show up as an empty screen.
    #[test]
    fn the_snapshot_has_the_shape_the_window_reads() {
        let dir = tempdir();
        // In English whatever this machine's language is: the translation
        // in use is shared by every test of the crate.
        let english = Settings {
            language: "en".into(),
            ..Settings::default()
        };
        let (mut app, _) = App::boot(Paths::new(dir.clone()), english);
        let _ = app.update(super::super::Message::Queue(
            super::super::queue::Message::UrlInput("https://example.com/a".into()),
        ));
        let _ = app.update(super::super::Message::Queue(
            super::super::queue::Message::AddUrl,
        ));
        let json = serde_json::to_value(app.snapshot()).unwrap();
        assert_eq!(json["queue"]["items"][0]["url"], "https://example.com/a");
        assert_eq!(json["queue"]["items"][0]["status"], "queued");
        assert_eq!(json["queue"]["items"][0]["lookup"], "fetching");
        assert_eq!(json["queue"]["items"][0]["kind"], "video");
        assert_eq!(json["queue"]["hasQueued"], true);
        assert_eq!(json["queue"]["usesTools"], true);
        assert_eq!(json["setup"]["components"][0]["name"], "yt-dlp");
        assert_eq!(json["setup"]["components"][0]["status"], "checking");
        assert_eq!(json["setup"]["job"], "inspecting");
        assert_eq!(json["setup"]["channel"], "nightly");
        assert_eq!(json["setup"]["welcoming"], true);
        assert_eq!(json["settings"]["values"]["kind"], "video");
        assert_eq!(json["settings"]["values"]["thumbnail"], "yes");
        assert_eq!(json["settings"]["videoPassword"], false);
        assert_eq!(json["canDownload"], false);

        let fixed = serde_json::to_value(app.fixed()).unwrap();
        assert_eq!(fixed["choices"]["quality"][0]["key"], "best");
        assert_eq!(fixed["channels"][1]["key"], "stable");
        assert_eq!(fixed["languages"][1]["key"], "es");
        assert_eq!(fixed["version"], env!("CARGO_PKG_VERSION"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
