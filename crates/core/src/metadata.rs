//! What a queue card shows before the download starts: title, thumbnail,
//! duration and approximate size of a video, or the title and number of
//! videos of a playlist.
//!
//! Both lookups block, so the GUI runs them on a thread of their own. They
//! are not retried: a failure only means a plainer card (the link as its
//! title, no thumbnail), never a link refused.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Deserialize;
use tr::tr;

use crate::process::{self, RunError};
use crate::provision::Paths;
use crate::provision::download::agent;

const TIMEOUT: Duration = Duration::from_secs(30);

/// What the lookup has to agree on with the download it previews.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lookup {
    /// `--cookies-from-browser`, as the download will use it: a video that
    /// needs an account can't be looked up without it either.
    pub cookies: Option<String>,
    /// `--no-playlist`: a `watch?v=...&list=...` link is then the video,
    /// not the playlist, exactly as the download will treat it.
    pub single_video: bool,
    /// A protected video can't be looked up without its password either.
    /// Handed over on stdin, like the download does.
    pub video_password: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    pub title: String,
    pub thumbnail_url: Option<String>,
    /// Seconds.
    pub duration: Option<f64>,
    /// Bytes. `filesize` when yt-dlp knows it exactly, `filesize_approx`
    /// otherwise; `None` when it has no idea either (common for formats
    /// assembled from live segments).
    pub approx_size: Option<u64>,
    /// `Some` for a playlist: how many videos it holds, when yt-dlp knows.
    pub playlist: Option<Option<u64>>,
    /// What the site offers, for the qualities worth offering and the
    /// size of each choice. Empty for a playlist.
    pub formats: Vec<Format>,
}

/// One of the formats a video is offered in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Format {
    /// The shorter side, which is what yt-dlp's `res:N` compares: a
    /// 1080x1920 short is "1080p".
    pub height: Option<u32>,
    /// The longer side: 3840 for a 3840x1600 film, which is 4K.
    pub width: Option<u32>,
    /// Bytes, exact or approximate.
    pub size: Option<u64>,
    pub video: bool,
    pub audio: bool,
}

/// What a download is set to get, as far as its size goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    Audio,
    /// Video with audio, merged from two formats when the best video has
    /// none of its own (the default).
    Video {
        cap: Option<u32>,
    },
    /// One format holding both, if there is one.
    SingleFile {
        cap: Option<u32>,
    },
    VideoOnly {
        cap: Option<u32>,
    },
    /// A format string of the user's own: nobody can tell.
    Custom,
}

impl Metadata {
    /// The tallest video on offer, when the site says.
    pub fn max_height(&self) -> Option<u32> {
        self.formats
            .iter()
            .filter(|f| f.video)
            .filter_map(|f| f.height)
            .max()
    }

    /// What the best format is called: "8K", "4K", or "1080p" and the like.
    pub fn best_resolution(&self) -> Option<String> {
        let best = self
            .formats
            .iter()
            .filter(|f| f.video && f.height.is_some())
            .max_by_key(|f| (f.height, f.width))?;
        let (height, width) = (best.height?, best.width.unwrap_or(0));
        Some(if height >= 4320 || width >= 7680 {
            "8K".into()
        } else if height >= 2160 || width >= 3840 {
            "4K".into()
        } else {
            format!("{height}p")
        })
    }

    /// About how big the download will be. Picks formats the way yt-dlp's
    /// default order does in the main (the tallest that fits under the
    /// cap, the smallest above it when none does), which is close enough
    /// for a size shown with a "~".
    pub fn estimate(&self, wanted: Wanted) -> Option<u64> {
        let best_audio = || {
            self.formats
                .iter()
                .filter(|f| f.audio && !f.video)
                .filter_map(|f| f.size)
                .max()
        };
        let pick = |cap: Option<u32>, keep: &dyn Fn(&Format) -> bool| -> Option<&Format> {
            let offered: Vec<&Format> =
                self.formats.iter().filter(|f| f.video && keep(f)).collect();
            let height = |f: &&Format| f.height.unwrap_or(0);
            let fitting: Vec<&Format> = match cap {
                Some(cap) if offered.iter().any(|f| height(f) <= cap) => {
                    offered.into_iter().filter(|f| height(f) <= cap).collect()
                }
                Some(_) => {
                    let lowest = offered.iter().map(height).min()?;
                    offered
                        .into_iter()
                        .filter(|f| height(f) == lowest)
                        .collect()
                }
                None => offered,
            };
            let tallest = fitting.iter().map(height).max()?;
            fitting
                .into_iter()
                .filter(|f| height(f) == tallest)
                .max_by_key(|f| f.size.unwrap_or(0))
        };
        // Nothing to choose from: the size of yt-dlp's own pick, which is
        // the best of everything.
        if self.formats.is_empty() {
            return match wanted {
                Wanted::Video { cap: None } => self.approx_size,
                _ => None,
            };
        }
        let uncapped =
            |cap: Option<u32>| cap.is_none_or(|c| self.max_height().is_some_and(|m| m <= c));
        match wanted {
            Wanted::Custom => None,
            Wanted::Audio => best_audio(),
            // yt-dlp's own pick is exact where it applies.
            Wanted::Video { cap } if uncapped(cap) && self.approx_size.is_some() => {
                self.approx_size
            }
            Wanted::Video { cap } => {
                let video = pick(cap, &|_| true)?;
                let size = video.size?;
                Some(if video.audio {
                    size
                } else {
                    size + best_audio().unwrap_or(0)
                })
            }
            Wanted::SingleFile { cap } => match pick(cap, &|f| f.audio) {
                Some(both) => both.size,
                None => self.estimate(Wanted::Video { cap }),
            },
            Wanted::VideoOnly { cap } => pick(cap, &|f| !f.audio).and_then(|f| f.size),
        }
    }
}

#[derive(Deserialize, Default)]
struct Info {
    #[serde(rename = "_type")]
    kind: Option<String>,
    title: Option<String>,
    thumbnail: Option<String>,
    /// A playlist has no single `thumbnail`, only this list, best last.
    #[serde(default)]
    thumbnails: Vec<Thumbnail>,
    duration: Option<f64>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
    playlist_count: Option<u64>,
    #[serde(default)]
    formats: Vec<FormatInfo>,
}

/// Numbers as `f64`: a site may give a height or a size with a fraction.
#[derive(Deserialize)]
struct FormatInfo {
    height: Option<f64>,
    width: Option<f64>,
    filesize: Option<f64>,
    filesize_approx: Option<f64>,
    /// Kilobits per second, for a size from the duration when the site
    /// gives none.
    tbr: Option<f64>,
    /// `"none"` when there is no video in it; missing when unknown.
    vcodec: Option<String>,
    acodec: Option<String>,
}

impl FormatInfo {
    fn into_format(self, duration: Option<f64>) -> Option<Format> {
        let has = |codec: &Option<String>| codec.as_deref() != Some("none");
        let (video, audio) = (has(&self.vcodec), has(&self.acodec));
        // Storyboards and the like: pictures, not something to download.
        if !video && !audio {
            return None;
        }
        let bytes = |n: f64| (n.is_finite() && n > 0.0).then_some(n as u64);
        let size = self
            .filesize
            .or(self.filesize_approx)
            .and_then(bytes)
            .or_else(|| bytes(self.tbr? * 125.0 * duration?));
        let side = |n: Option<f64>| n.filter(|n| n.is_finite() && *n > 0.0);
        // yt-dlp's res:N sorts on the shorter side; either one when only
        // one is known.
        let (shorter, longer) = match (side(self.height), side(self.width)) {
            (Some(h), Some(w)) => (Some(h.min(w)), Some(h.max(w))),
            (h, w) => (h.or(w), None),
        };
        Some(Format {
            height: shorter.map(|n| n as u32),
            width: longer.map(|n| n as u32),
            size,
            video,
            audio,
        })
    }
}

#[derive(Deserialize)]
struct Thumbnail {
    url: Option<String>,
}

impl From<Info> for Metadata {
    fn from(info: Info) -> Self {
        let thumbnail_url = info
            .thumbnail
            .or_else(|| info.thumbnails.into_iter().rev().find_map(|t| t.url));
        if info.kind.as_deref() == Some("playlist") {
            return Metadata {
                title: info.title.unwrap_or_default(),
                thumbnail_url,
                duration: None,
                approx_size: None,
                playlist: Some(info.playlist_count),
                formats: Vec::new(),
            };
        }
        let duration = info.duration;
        Metadata {
            title: info.title.unwrap_or_default(),
            thumbnail_url,
            duration,
            approx_size: info.filesize.or(info.filesize_approx),
            playlist: None,
            formats: info
                .formats
                .into_iter()
                .filter_map(|f| f.into_format(duration))
                .collect(),
        }
    }
}

/// How many lookups may run at once. Each one starts a PyInstaller
/// program that unpacks itself first, so pasting thirty links must not
/// start thirty of them.
const MAX_LOOKUPS: usize = 3;

/// A counting semaphore (`std` has none) for [`MAX_LOOKUPS`].
struct Slots {
    running: Mutex<usize>,
    freed: Condvar,
}

static SLOTS: Slots = Slots {
    running: Mutex::new(0),
    freed: Condvar::new(),
};

/// Held while a lookup runs; frees its slot when dropped, panics included.
struct Slot;

impl Slot {
    fn acquire() -> Slot {
        let mut running = SLOTS.running.lock().unwrap_or_else(|e| e.into_inner());
        while *running >= MAX_LOOKUPS {
            running = SLOTS.freed.wait(running).unwrap_or_else(|e| e.into_inner());
        }
        *running += 1;
        Slot
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut running = SLOTS.running.lock().unwrap_or_else(|e| e.into_inner());
        *running -= 1;
        SLOTS.freed.notify_one();
    }
}

/// Runs `yt-dlp -J --flat-playlist --playlist-items 1` on `url` and reads
/// the JSON it prints. A playlist answers with its own title and count
/// instead of walking every video. Waits while [`MAX_LOOKUPS`] others run.
pub fn fetch(paths: &Paths, url: &str, lookup: &Lookup) -> Result<Metadata, String> {
    let _slot = Slot::acquire();
    let tool = paths.tool("yt-dlp");
    // Same JS runtime the real download uses: without it, sites that throw
    // JavaScript challenges at extractors (YouTube chief among them) can
    // fail or hang here even though the download itself works fine.
    let mut js_runtime = std::ffi::OsString::from("deno:");
    js_runtime.push(paths.tool("deno"));
    let mut command = process::command(&tool);
    command
        .args([
            // No -P here, so yt-dlp would otherwise read a yt-dlp.conf from
            // whatever directory the app was started in.
            crate::engine::IGNORE_CONFIG,
            "-J",
            "--flat-playlist",
            "--no-warnings",
            "--playlist-items",
            "1",
        ])
        .arg("--js-runtimes")
        .arg(js_runtime);
    if let Some(browser) = &lookup.cookies {
        command.args(["--cookies-from-browser", browser]);
    }
    if lookup.single_video {
        command.arg("--no-playlist");
    }
    let stdin_config = lookup
        .video_password
        .as_deref()
        .map(crate::engine::password_config);
    if stdin_config.is_some() {
        command.args(crate::engine::STDIN_CONFIG);
    }
    command
        // "--" as in the download itself, so a link can never be an option.
        .arg("--")
        .arg(url);

    let output = match process::run_captured_with_input(command, TIMEOUT, stdin_config.as_deref()) {
        Ok(output) => output,
        Err(RunError::TimedOut) => return Err(tr!("Timed out looking up this link.")),
        Err(RunError::Spawn(e)) => {
            return Err(tr!("Could not start {tool}: {e}", tool = tool.display(), e));
        }
        Err(RunError::Wait(e)) => return Err(e.to_string()),
    };

    if !output.status.success() {
        // A known problem in plain words, or else yt-dlp's own, which stay
        // in English.
        return Err(match output.stderr.lines().find(|l| !l.trim().is_empty()) {
            Some(line) => {
                let line = crate::redact::hide_addresses(line.trim_start_matches("ERROR: "));
                let line = line.as_ref();
                crate::explain::explain(line, paths.bundled_tools())
                    .unwrap_or_else(|| line.to_string())
            }
            None => tr!("yt-dlp could not look up this link."),
        });
    }

    let line = output
        .stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| tr!("yt-dlp returned no information for this link."))?;
    let info: Info =
        serde_json::from_str(line).map_err(|e| tr!("Could not read yt-dlp's output: {e}", e))?;
    Ok(info.into())
}

/// The thumbnail's bytes, one attempt.
pub fn fetch_thumbnail(url: &str) -> Result<Vec<u8>, String> {
    agent()
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .into_body()
        .read_to_vec()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `yt-dlp -J` for archive.org's Big Buck Bunny, cut down to what is
    /// read: three files of one size each, codecs unknown.
    const ARCHIVE_ORG: &str = r#"{
        "title": "Big Buck Bunny", "duration": 596.46, "filesize": 332243668,
        "formats": [
            {"format_id": "0", "ext": "ogv", "height": 300, "filesize": 46935223},
            {"format_id": "1", "ext": "mp4", "height": 360, "filesize": 61878609},
            {"format_id": "2", "ext": "avi", "height": 720, "filesize": 332243668}
        ]
    }"#;

    /// The same for a SoundCloud track: audio only, sizes from the bitrate.
    const SOUNDCLOUD: &str = r#"{
        "title": "Flickermood", "duration": 213.886, "filesize_approx": 4277720,
        "formats": [
            {"format_id": "hls_aac_96k", "vcodec": "none", "acodec": "mp4a.40.2", "filesize_approx": 2566632, "tbr": 96},
            {"format_id": "hls_aac_160k", "vcodec": "none", "acodec": "mp4a.40.2", "filesize_approx": 4277720, "tbr": 160}
        ]
    }"#;

    fn meta(json: &str) -> Metadata {
        serde_json::from_str::<Info>(json).unwrap().into()
    }

    /// Choosing "Up to 480p" there downloaded the 360p MP4, 61.9 MB, while
    /// the card went on saying ~332 MB.
    #[test]
    fn the_size_follows_the_quality_chosen() {
        let bunny = meta(ARCHIVE_ORG);
        assert_eq!(bunny.max_height(), Some(720));
        assert_eq!(bunny.estimate(Wanted::Video { cap: None }), Some(332243668));
        assert_eq!(
            bunny.estimate(Wanted::Video { cap: Some(1080) }),
            Some(332243668)
        );
        assert_eq!(
            bunny.estimate(Wanted::Video { cap: Some(480) }),
            Some(61878609)
        );
        // Nothing that low: yt-dlp takes the smallest there is.
        assert_eq!(
            bunny.estimate(Wanted::Video { cap: Some(144) }),
            Some(46935223)
        );
        assert_eq!(bunny.estimate(Wanted::Custom), None);
    }

    #[test]
    fn audio_only_is_the_size_of_the_best_audio() {
        let track = meta(SOUNDCLOUD);
        assert_eq!(track.max_height(), None);
        assert_eq!(track.estimate(Wanted::Audio), Some(4277720));
    }

    /// The usual YouTube case: video and audio come apart and are merged.
    #[test]
    fn a_merged_download_adds_the_best_audio_to_the_video() {
        let video = meta(
            r#"{
                "title": "v", "duration": 100, "filesize_approx": 999,
                "formats": [
                    {"height": 1080, "vcodec": "vp9", "acodec": "none", "filesize": 50000000},
                    {"height": 720, "vcodec": "vp9", "acodec": "none", "filesize": 20000000},
                    {"height": 720, "vcodec": "avc1", "acodec": "none", "filesize": 25000000},
                    {"height": 360, "vcodec": "avc1", "acodec": "mp4a", "filesize": 8000000},
                    {"vcodec": "none", "acodec": "opus", "filesize": 3000000},
                    {"vcodec": "none", "acodec": "mp4a", "tbr": 128},
                    {"vcodec": "none", "acodec": "none", "format_note": "storyboard"}
                ]
            }"#,
        );
        assert_eq!(
            video.formats.len(),
            6,
            "the storyboard is no format to download"
        );
        // 128 kb/s for 100 s, from the bitrate.
        assert_eq!(video.formats[5].size, Some(1_600_000));
        assert_eq!(
            video.estimate(Wanted::Video { cap: Some(720) }),
            Some(28_000_000)
        );
        assert_eq!(
            video.estimate(Wanted::VideoOnly { cap: Some(720) }),
            Some(25_000_000)
        );
        assert_eq!(
            video.estimate(Wanted::SingleFile { cap: None }),
            Some(8_000_000)
        );
        assert_eq!(video.estimate(Wanted::Audio), Some(3_000_000));
    }

    /// A YouTube short is taller than wide: yt-dlp calls 1080x1920 1080p,
    /// so does Osprey.
    #[test]
    fn a_portrait_video_is_measured_by_its_shorter_side() {
        let short = meta(
            r#"{
                "title": "s", "duration": 30,
                "formats": [
                    {"width": 1080, "height": 1920, "vcodec": "vp9", "acodec": "none", "filesize": 9000000},
                    {"width": 480, "height": 854, "vcodec": "vp9", "acodec": "none", "filesize": 2000000},
                    {"vcodec": "none", "acodec": "opus", "filesize": 500000}
                ]
            }"#,
        );
        assert_eq!(short.max_height(), Some(1080));
        assert_eq!(short.best_resolution().as_deref(), Some("1080p"));
        assert_eq!(
            short.estimate(Wanted::Video { cap: Some(1080) }),
            Some(9_500_000)
        );
        assert_eq!(
            short.estimate(Wanted::Video { cap: Some(720) }),
            Some(2_500_000)
        );
    }

    #[test]
    fn the_best_resolution_has_the_name_people_know() {
        let named = |w: u32, h: u32| {
            meta(&format!(
                r#"{{"title": "v", "formats": [{{"width": {w}, "height": {h}, "vcodec": "vp9"}}]}}"#
            ))
            .best_resolution()
        };
        assert_eq!(named(7680, 4320).as_deref(), Some("8K"));
        assert_eq!(named(3840, 2160).as_deref(), Some("4K"));
        assert_eq!(named(3840, 1600).as_deref(), Some("4K"));
        assert_eq!(named(2560, 1440).as_deref(), Some("1440p"));
        assert_eq!(meta(ARCHIVE_ORG).best_resolution().as_deref(), Some("720p"));
        assert_eq!(meta(SOUNDCLOUD).best_resolution(), None);
    }

    #[test]
    fn without_formats_only_the_best_has_a_size() {
        let bare = meta(r#"{"title": "x", "filesize_approx": 1000}"#);
        assert_eq!(bare.estimate(Wanted::Video { cap: None }), Some(1000));
        assert_eq!(bare.estimate(Wanted::Video { cap: Some(480) }), None);
        assert_eq!(bare.estimate(Wanted::Audio), None);
        assert_eq!(bare.estimate(Wanted::Custom), None);
    }

    #[test]
    fn a_full_info_dict_is_mapped_to_metadata() {
        let info: Info = serde_json::from_str(
            r#"{
                "title": "Me at the zoo",
                "thumbnail": "https://i.ytimg.com/vi/jNQXAC9IVRw/maxresdefault.jpg",
                "duration": 19.0,
                "filesize": 1234567,
                "filesize_approx": 9999999
            }"#,
        )
        .unwrap();
        let meta: Metadata = info.into();
        assert_eq!(meta.title, "Me at the zoo");
        assert_eq!(
            meta.thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/jNQXAC9IVRw/maxresdefault.jpg")
        );
        assert_eq!(meta.duration, Some(19.0));
        // The exact size wins over the estimate when both are present.
        assert_eq!(meta.approx_size, Some(1234567));
    }

    /// Shape checked against a real `-J --flat-playlist --playlist-items 1`
    /// of a YouTube playlist: one entry, but the count of all of them.
    #[test]
    fn a_playlist_previews_as_itself() {
        let info: Info = serde_json::from_str(
            r#"{"_type": "playlist", "title": "All Tutorials", "playlist_count": 13,
                "thumbnails": [{"url": "https://i.ytimg.com/small.jpg"},
                               {"url": "https://i.ytimg.com/big.jpg"}],
                "entries": [{"_type": "url", "title": "Part 6", "duration": 165}]}"#,
        )
        .unwrap();
        let meta: Metadata = info.into();
        assert_eq!(meta.title, "All Tutorials");
        assert_eq!(meta.playlist, Some(Some(13)));
        assert_eq!(meta.duration, None, "not the first video's");
        assert_eq!(
            meta.thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/big.jpg")
        );
    }

    #[test]
    fn a_missing_exact_size_falls_back_to_the_estimate() {
        let info: Info = serde_json::from_str(r#"{"filesize_approx": 555}"#).unwrap();
        let meta: Metadata = info.into();
        assert_eq!(meta.approx_size, Some(555));
    }

    #[test]
    fn fields_yt_dlp_does_not_know_are_left_out_rather_than_guessed() {
        let info: Info = serde_json::from_str("{}").unwrap();
        let meta: Metadata = info.into();
        assert_eq!(meta.title, "");
        assert_eq!(meta.thumbnail_url, None);
        assert_eq!(meta.duration, None);
        assert_eq!(meta.approx_size, None);
    }

    /// Pasting many links used to start one yt-dlp per link at once, each
    /// a PyInstaller program unpacking itself. Lookups now take turns.
    #[test]
    fn lookups_run_a_few_at_a_time_not_all_at_once() {
        use std::fs;

        let _guard = crate::testing::exec_guard();
        let dir = crate::testing::tempdir();
        let paths = Paths::new(dir.clone());
        let log = dir.join("log");
        crate::testing::stand_in(
            &paths,
            "yt-dlp",
            &format!(
                "append {0}|s\nsleep 0.3\nappend {0}|e\nout {{\"title\": \"x\"}}",
                log.display()
            ),
        );

        let workers: Vec<_> = (0..8)
            .map(|_| {
                let paths = paths.clone();
                std::thread::spawn(move || {
                    fetch(&paths, "https://example.com/v", &Lookup::default()).unwrap()
                })
            })
            .collect();
        for w in workers {
            assert_eq!(w.join().unwrap().title, "x");
        }
        let (mut now, mut most) = (0i32, 0i32);
        for line in fs::read_to_string(&log).unwrap().lines() {
            now += if line == "s" { 1 } else { -1 };
            most = most.max(now);
        }
        assert!(most <= 3, "{most} lookups ran at the same time");
        fs::remove_dir_all(dir).unwrap();
    }

    /// The lookup treats the link the way its download will: same cookies,
    /// same single-video choice.
    #[test]
    fn the_lookup_asks_what_the_download_will_ask() {
        use std::fs;

        let _guard = crate::testing::exec_guard();
        let dir = crate::testing::tempdir();
        let paths = Paths::new(dir.clone());
        let argv = dir.join("argv");
        crate::testing::stand_in(
            &paths,
            "yt-dlp",
            &format!("args-to {}\nout {{\"title\": \"x\"}}", argv.display()),
        );

        let lookup = Lookup {
            cookies: Some("firefox:work".into()),
            single_video: true,
            video_password: None,
        };
        fetch(&paths, "https://example.com/watch?v=a&list=b", &lookup).unwrap();
        let args = fs::read_to_string(&argv).unwrap();
        let args: Vec<&str> = args.lines().collect();
        let pair = args
            .windows(2)
            .any(|w| w == ["--cookies-from-browser", "firefox:work"]);
        assert!(pair && args.contains(&"--no-playlist"), "{args:?}");
        // A yt-dlp.conf in the starting directory could hold --exec.
        assert!(args.contains(&"--ignore-config"), "{args:?}");
        assert_eq!(
            args[args.len() - 2..],
            ["--", "https://example.com/watch?v=a&list=b"]
        );

        fetch(&paths, "https://example.com/v", &Lookup::default()).unwrap();
        let args = fs::read_to_string(&argv).unwrap();
        assert!(
            !args.contains("--no-playlist") && !args.contains("--cookies"),
            "{args}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    /// A real video's `-j` output can run past the OS pipe buffer (a video
    /// with many formats easily prints several hundred KB of JSON). Once
    /// found the hard way: reading stdout only after the child exits lets
    /// the child block on a full pipe forever, since try_wait() alone never
    /// drains it. Regression test for that deadlock, not just a parsing check.
    /// (`process::run_captured` does the draining now.)
    #[test]
    fn a_response_bigger_than_the_pipe_buffer_does_not_deadlock() {
        use std::fs;

        let _guard = crate::testing::exec_guard();
        let dir = crate::testing::tempdir();
        let paths = Paths::new(dir.clone());
        crate::testing::stand_in(
            &paths,
            "yt-dlp",
            &format!(
                "out {{\"title\": \"big\", \"duration\": 5, \"pad\": \"{}\"}}",
                "a".repeat(200_000)
            ),
        );

        let started = std::time::Instant::now();
        let meta = fetch(&paths, "https://example.com/video", &Lookup::default()).unwrap();
        assert!(
            started.elapsed() < TIMEOUT,
            "must finish well before the timeout, not by hitting it"
        );
        assert_eq!(meta.title, "big");
        assert_eq!(meta.duration, Some(5.0));
        fs::remove_dir_all(dir).unwrap();
    }

    /// A protected video's lookup gets the password the same way as the
    /// download: on stdin, never on the command line.
    #[test]
    fn the_lookup_hands_the_video_password_over_on_stdin() {
        use std::fs;

        let _guard = crate::testing::exec_guard();
        let dir = crate::testing::tempdir();
        let paths = Paths::new(dir.clone());
        let (argv, stdin) = (dir.join("argv"), dir.join("stdin"));
        crate::testing::stand_in(
            &paths,
            "yt-dlp",
            &format!(
                "args-to {}\nstdin-to {}\nout {{\"title\": \"x\"}}",
                argv.display(),
                stdin.display()
            ),
        );

        let lookup = Lookup {
            video_password: Some("hunter2".into()),
            ..Lookup::default()
        };
        fetch(&paths, "https://vimeo.com/123", &lookup).unwrap();
        assert_eq!(
            fs::read_to_string(&stdin).unwrap(),
            "--video-password \"hunter2\"\n"
        );
        let args = fs::read_to_string(&argv).unwrap();
        assert!(
            args.contains("--config-locations\n-\n") && !args.contains("hunter2"),
            "{args}"
        );

        // Without a password, stdin stays empty and isn't asked for.
        fetch(&paths, "https://vimeo.com/123", &Lookup::default()).unwrap();
        assert_eq!(fs::read_to_string(&stdin).unwrap(), "");
        assert!(
            !fs::read_to_string(&argv)
                .unwrap()
                .contains("--config-locations")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    /// A failed lookup shows a known problem in plain words, and anything
    /// else in yt-dlp's own, without its "ERROR: " prefix.
    #[test]
    fn a_failed_lookup_explains_known_errors() {
        use std::fs;

        let _guard = crate::testing::exec_guard();
        let dir = crate::testing::tempdir();
        let paths = Paths::new(dir.clone());
        let fail_with = |line: &str| {
            crate::testing::stand_in(&paths, "yt-dlp", &format!("err ERROR: {line}\nexit 1"));
            fetch(&paths, "https://example.com/v", &Lookup::default()).unwrap_err()
        };

        let geo = "[youtube] abc: This video is not available in your country";
        assert_eq!(fail_with(geo), crate::explain::explain(geo, false).unwrap());
        assert_eq!(fail_with("[x] abc: odd failure"), "[x] abc: odd failure");
        fs::remove_dir_all(dir).unwrap();
    }
}
