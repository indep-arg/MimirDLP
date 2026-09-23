//! Looks up a link before it is queued: title, thumbnail, duration and an
//! approximate size for a video, or the title and the number of videos for
//! a playlist, the way a queue card needs to show something useful before
//! the download itself has even started.
//!
//! Both lookups are blocking and run on a worker thread from the GUI, same
//! as everything else in `provision` and `runner`. Neither is retried: a
//! failure here just means a plainer card (the raw URL as its title, no
//! thumbnail), not a reason to refuse queuing the link.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Deserialize;

use crate::process::{self, RunError};
use crate::provision::Paths;
use crate::provision::download::agent;

/// A playlist is looked up flat (`--flat-playlist`, one entry), so it
/// answers with its own title and count instead of walking every video.
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
            };
        }
        Metadata {
            title: info.title.unwrap_or_default(),
            thumbnail_url,
            duration: info.duration,
            approx_size: info.filesize.or(info.filesize_approx),
            playlist: None,
        }
    }
}

/// How many lookups may run at once. Each one starts a PyInstaller
/// program that unpacks itself first, so pasting thirty links must not
/// start thirty of them.
const MAX_LOOKUPS: usize = 3;

/// A counting semaphore: `std` has none, and this is all it takes.
struct Turns {
    running: Mutex<usize>,
    freed: Condvar,
}

static TURNS: Turns = Turns {
    running: Mutex::new(0),
    freed: Condvar::new(),
};

/// Held while a lookup runs; gives the turn back when dropped, panics
/// included.
struct Turn;

impl Turn {
    fn take() -> Turn {
        let mut running = TURNS.running.lock().unwrap_or_else(|e| e.into_inner());
        while *running >= MAX_LOOKUPS {
            running = TURNS.freed.wait(running).unwrap_or_else(|e| e.into_inner());
        }
        *running += 1;
        Turn
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut running = TURNS.running.lock().unwrap_or_else(|e| e.into_inner());
        *running -= 1;
        TURNS.freed.notify_one();
    }
}

/// Runs `yt-dlp -j` on `url` and parses the one line of JSON it prints.
/// Blocks while [`MAX_LOOKUPS`] others are already running.
pub fn fetch(paths: &Paths, url: &str, lookup: &Lookup) -> Result<Metadata, String> {
    let _turn = Turn::take();
    let tool = paths.tool("yt-dlp");
    // Same JS runtime the real download uses: without it, sites that throw
    // JavaScript challenges at extractors (YouTube chief among them) can
    // fail or hang here even though the download itself works fine.
    let mut js_runtime = std::ffi::OsString::from("deno:");
    js_runtime.push(paths.tool("deno"));
    let mut command = process::command(&tool);
    command
        .args([
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
    command
        // "--" as in the download itself, so a link can never be an option.
        .arg("--")
        .arg(url);

    let output = match process::run_captured(command, TIMEOUT) {
        Ok(output) => output,
        Err(RunError::TimedOut) => return Err("Timed out looking up this link.".into()),
        Err(RunError::Spawn(e)) => return Err(format!("Could not start {}: {e}", tool.display())),
        Err(RunError::Wait(e)) => return Err(e.to_string()),
    };

    if !output.status.success() {
        let first_line = output
            .stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("yt-dlp could not look up this link.");
        return Err(first_line.trim_start_matches("ERROR: ").to_string());
    }

    let line = output
        .stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| "yt-dlp returned no information for this link.".to_string())?;
    let info: Info =
        serde_json::from_str(line).map_err(|e| format!("Could not read yt-dlp's output: {e}"))?;
    Ok(info.into())
}

/// A one-shot, best-effort download of the thumbnail image bytes, handed
/// straight to `iced::widget::image::Handle::from_bytes` by the caller.
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
    #[cfg(unix)]
    #[test]
    fn lookups_run_a_few_at_a_time_not_all_at_once() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let _guard = crate::testing::exec_guard();
        let dir = crate::testing::tempdir();
        let paths = Paths::new(dir.clone());
        fs::create_dir_all(&paths.bin_dir).unwrap();
        let log = dir.join("log");
        let script = paths.tool("yt-dlp");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\necho s >> {0}\nsleep 0.3\necho e >> {0}\necho '{{\"title\": \"x\"}}'\n",
                log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

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
    #[cfg(unix)]
    #[test]
    fn the_lookup_asks_what_the_download_will_ask() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let _guard = crate::testing::exec_guard();
        let dir = crate::testing::tempdir();
        let paths = Paths::new(dir.clone());
        fs::create_dir_all(&paths.bin_dir).unwrap();
        let argv = dir.join("argv");
        let script = paths.tool("yt-dlp");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > {}\necho '{{\"title\": \"x\"}}'\n",
                argv.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

        let lookup = Lookup {
            cookies: Some("firefox:work".into()),
            single_video: true,
        };
        fetch(&paths, "https://example.com/watch?v=a&list=b", &lookup).unwrap();
        let args = fs::read_to_string(&argv).unwrap();
        let args: Vec<&str> = args.lines().collect();
        let pair = args
            .windows(2)
            .any(|w| w == ["--cookies-from-browser", "firefox:work"]);
        assert!(pair && args.contains(&"--no-playlist"), "{args:?}");
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
    #[cfg(unix)]
    #[test]
    fn a_response_bigger_than_the_pipe_buffer_does_not_deadlock() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let _guard = crate::testing::exec_guard();
        let dir = crate::testing::tempdir();
        let paths = Paths::new(dir.clone());
        fs::create_dir_all(&paths.bin_dir).unwrap();
        let script = paths.tool("yt-dlp");
        fs::write(
            &script,
            "#!/bin/sh\n\
             printf '{\"title\": \"big\", \"duration\": 5, \"pad\": \"'\n\
             head -c 200000 /dev/zero | tr '\\0' 'a'\n\
             printf '\"}\\n'\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

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
}
