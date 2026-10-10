//! Real downloads through the app's own runner, with the file that comes
//! out checked by ffprobe. `tests/engine_scenarios.rs` pins the argv; this
//! checks what yt-dlp and FFmpeg then actually do with it (thumbnails,
//! chapters, SponsorBlock cuts, containers), which is where things break.
//!
//! Not part of the normal run (network, about 30 MB of video plus the tools
//! the first time):
//!
//! ```text
//! cargo test --test e2e -- --ignored --nocapture
//! ```
//!
//! The tools are provisioned once, with the app's own installer, into
//! `target/e2e-app` (or `MIMIRDLP_E2E_DIR`) and reused afterwards. The
//! videos are real YouTube uploads; if one disappears, pick another with
//! the same property (see each constant) rather than weakening a check.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use mimirdlp::config::{Audio, Cats, Container, Kind, Quality, Settings, Sponsor, VideoFormat};
use mimirdlp::metadata::{self, Lookup};
use mimirdlp::provision::{self, Component, Paths, Status};
use mimirdlp::runner::{self, Cancel, Event, Finish};

/// 19 seconds, the first YouTube video. Tiny, and has a thumbnail.
const ZOO: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";
/// 80 seconds, 3 chapters, and a locked SponsorBlock "intro" segment from
/// 0 to 3 s (checked against sponsor.ajay.app on 2026-09-23).
const CHAPTERS: &str = "https://www.youtube.com/watch?v=b1Fo_M_tj6w";
/// A public playlist of 13 videos (2026-09-23), and a link to one of its
/// videos inside it.
const PLAYLIST: &str = "https://www.youtube.com/playlist?list=PL4-kP2YUyNMdnxYKRkdRQgXbjJ7KtqMGZ";
const VIDEO_IN_PLAYLIST: &str =
    "https://www.youtube.com/watch?v=BRQnJ6XAC2U&list=PL4-kP2YUyNMdnxYKRkdRQgXbjJ7KtqMGZ";

/// The tools, installed once for the whole run.
fn app() -> &'static Paths {
    static APP: OnceLock<Paths> = OnceLock::new();
    APP.get_or_init(|| {
        let dir = std::env::var_os("MIMIRDLP_E2E_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("target/e2e-app"));
        let paths = Paths::new(dir);
        for component in Component::ALL {
            if !matches!(
                provision::inspect(component, &paths),
                Status::Installed { .. }
            ) {
                provision::install(component, &paths, &mut |_| {})
                    .unwrap_or_else(|e| panic!("installing {}: {e}", component.label()));
            }
        }
        paths
    })
}

/// One download at a time: gentler on YouTube, and the log reads in order.
///
/// After a few dozen downloads in a row YouTube starts answering some
/// video requests with `HTTP Error 403` for a while (seen here: the same
/// test passing, then failing, then passing again with nothing changed).
/// That one error, and only that one, is tried again after a pause; any
/// other failure fails the test at once.
fn download(settings: &Settings, url: &str) -> (tempdir::Dir, PathBuf) {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let paths = app();
    let options = settings.to_options(paths);
    let (out, events) = (1..=3)
        .map(|attempt| {
            let out = tempdir::Dir::new();
            let mut events = Vec::new();
            runner::run(
                paths,
                &options,
                Some(out.path()),
                &[url.to_string()],
                &Cancel::default(),
                &mut |e| events.push(e),
            );
            let throttled = matches!(
                events.last(),
                Some(Event::Finished(Finish::Failed(m))) if m.contains("HTTP Error 403")
            );
            if throttled && attempt < 3 {
                eprintln!("YouTube answered 403, trying again ({attempt}/3)");
                std::thread::sleep(std::time::Duration::from_secs(20));
                None
            } else {
                Some((out, events))
            }
        })
        .find_map(|result| result)
        .unwrap();
    let log = || {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Log(level, line) => Some(format!("{} {line}", level.tag())),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        events.last(),
        Some(&Event::Finished(Finish::Completed)),
        "{}",
        log()
    );
    let file = events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::File(path) => Some(PathBuf::from(path)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file reported\n{}", log()));
    assert!(file.is_file(), "{} is missing", file.display());
    (out, file)
}

#[derive(Debug)]
struct Probe {
    /// `(codec_type, codec_name, attached_pic)` per stream.
    streams: Vec<(String, String, bool)>,
    chapters: Vec<String>,
    duration: f64,
}

impl Probe {
    fn has_cover(&self) -> bool {
        self.streams.iter().any(|(_, _, cover)| *cover)
    }

    fn has(&self, kind: &str) -> bool {
        self.streams.iter().any(|(t, _, cover)| t == kind && !cover)
    }
}

fn probe(file: &Path) -> Probe {
    let output = std::process::Command::new(app().tool("ffprobe"))
        .args(["-v", "error", "-of", "json", "-show_chapters"])
        .args(["-show_entries", "format=duration"])
        .args([
            "-show_entries",
            "stream=codec_type,codec_name:stream_disposition=attached_pic",
        ])
        .arg(file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "ffprobe failed on {}",
        file.display()
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
    Probe {
        streams: json["streams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    text(&s["codec_type"]),
                    text(&s["codec_name"]),
                    s["disposition"]["attached_pic"].as_i64() == Some(1),
                )
            })
            .collect(),
        chapters: json["chapters"]
            .as_array()
            .map(|c| c.iter().map(|c| text(&c["tags"]["title"])).collect())
            .unwrap_or_default(),
        duration: json["format"]["duration"]
            .as_str()
            .and_then(|d| d.parse().ok())
            .unwrap_or(0.0),
    }
}

fn extension(file: &Path) -> &str {
    file.extension().and_then(|e| e.to_str()).unwrap_or("")
}

#[test]
#[ignore = "downloads from YouTube"]
fn the_defaults_give_an_mkv_with_the_thumbnail_embedded() {
    let (_dir, file) = download(&Settings::default(), ZOO);
    let p = probe(&file);
    assert_eq!(extension(&file), "mkv");
    assert!(p.has("video") && p.has("audio"), "{p:?}");
    assert!(p.has_cover(), "{p:?}");
}

#[test]
#[ignore = "downloads from YouTube"]
fn mp4_holds_the_thumbnail_too() {
    let settings = Settings {
        container: Container::Mp4,
        ..Settings::default()
    };
    let (_dir, file) = download(&settings, ZOO);
    let p = probe(&file);
    assert_eq!(extension(&file), "mp4");
    assert!(p.has("video") && p.has("audio") && p.has_cover(), "{p:?}");
}

#[test]
#[ignore = "downloads from YouTube"]
fn audio_as_mp3_gets_a_cover() {
    let settings = Settings {
        kind: Kind::AudioOnly,
        audio: Audio::Mp3,
        ..Settings::default()
    };
    let (_dir, file) = download(&settings, ZOO);
    let p = probe(&file);
    assert_eq!(extension(&file), "mp3");
    assert!(p.has("audio") && !p.has("video") && p.has_cover(), "{p:?}");
}

/// YouTube serves this as opus in webm, which can't hold a thumbnail:
/// the engine rewraps it into .opus (`webm>opus`) so it can.
#[test]
#[ignore = "downloads from YouTube"]
fn audio_kept_as_served_is_rewrapped_so_the_cover_fits() {
    let settings = Settings {
        kind: Kind::AudioOnly,
        audio_convert: false,
        ..Settings::default()
    };
    let (_dir, file) = download(&settings, ZOO);
    let p = probe(&file);
    assert_ne!(extension(&file), "webm", "{p:?}");
    assert!(p.has("audio") && p.has_cover(), "{} {p:?}", file.display());
}

/// WAV can't hold a thumbnail; asking for one anyway used to fail the
/// whole download at the postprocessing step.
#[test]
#[ignore = "downloads from YouTube"]
fn wav_skips_the_thumbnail_instead_of_failing() {
    let settings = Settings {
        kind: Kind::AudioOnly,
        audio: Audio::Wav,
        ..Settings::default()
    };
    let (_dir, file) = download(&settings, ZOO);
    assert_eq!(extension(&file), "wav");
    assert!(probe(&file).has("audio"));
}

fn small() -> Settings {
    Settings {
        quality: Quality::P480,
        ..Settings::default()
    }
}

/// yt-dlp turns chapters on by itself with metadata; the engine says
/// `--no-embed-chapters` so "metadata only" means exactly that.
#[test]
#[ignore = "downloads from YouTube"]
fn metadata_without_chapters_really_has_no_chapters() {
    let settings = Settings {
        metadata: true,
        chapters: false,
        ..small()
    };
    let (_dir, file) = download(&settings, CHAPTERS);
    assert_eq!(probe(&file).chapters, Vec::<String>::new());
}

#[test]
#[ignore = "downloads from YouTube"]
fn chapters_are_embedded_when_asked_for() {
    let settings = Settings {
        chapters: true,
        ..small()
    };
    let (_dir, file) = download(&settings, CHAPTERS);
    assert_eq!(probe(&file).chapters.len(), 3);
}

#[test]
#[ignore = "downloads from YouTube and sponsor.ajay.app"]
fn sponsorblock_marking_adds_the_segment_as_a_chapter() {
    let settings = Settings {
        sponsor: Sponsor::Mark,
        cats: Cats::Custom,
        custom_cats: "intro".into(),
        ..small()
    };
    let (_dir, file) = download(&settings, CHAPTERS);
    let chapters = probe(&file).chapters;
    assert!(chapters.iter().any(|c| c.contains("Intro")), "{chapters:?}");
}

#[test]
#[ignore = "downloads from YouTube and sponsor.ajay.app"]
fn sponsorblock_removal_really_cuts_the_segment() {
    let full = probe(&download(&small(), CHAPTERS).1).duration;
    let settings = Settings {
        sponsor: Sponsor::Remove,
        cats: Cats::Custom,
        custom_cats: "intro".into(),
        ..small()
    };
    let cut = probe(&download(&settings, CHAPTERS).1).duration;
    // The intro is 3 s long.
    assert!(
        (full - cut - 3.0).abs() < 1.0,
        "full {full:.2} s, cut {cut:.2} s"
    );
}

/// "Video only" with MP4 chosen: YouTube's best video-only stream is VP9
/// in webm, which is rewrapped for the thumbnail. Pins what that gives.
#[test]
#[ignore = "downloads from YouTube"]
fn video_only_keeps_a_thumbnail() {
    let settings = Settings {
        video_format: VideoFormat::VideoOnly,
        container: Container::Mp4,
        ..Settings::default()
    };
    let (_dir, file) = download(&settings, ZOO);
    let p = probe(&file);
    assert!(
        p.has("video") && !p.has("audio") && p.has_cover(),
        "{} {p:?}",
        file.display()
    );
}

/// MP4 chosen and a webm result (forced here, since YouTube's best
/// video-only stream is often VP9 in MP4 already): the rewrap for the
/// thumbnail follows the choice instead of silently giving an .mkv.
/// Checked by hand first: VP9 in MP4 with an attached cover is valid.
#[test]
#[ignore = "downloads from YouTube"]
fn a_webm_result_follows_the_chosen_mp4_container() {
    let settings = Settings {
        video_format: VideoFormat::Custom,
        custom_format: "bestvideo[ext=webm]".into(),
        container: Container::Mp4,
        ..Settings::default()
    };
    let (_dir, file) = download(&settings, CHAPTERS);
    let p = probe(&file);
    assert_eq!(extension(&file), "mp4", "{p:?}");
    assert!(p.has("video") && p.has_cover(), "{p:?}");
}

#[test]
#[ignore = "asks YouTube"]
fn a_video_lookup_gives_its_title_and_length() {
    let meta = metadata::fetch(app(), ZOO, &Lookup::default()).unwrap();
    assert_eq!(meta.title, "Me at the zoo");
    assert_eq!(meta.duration, Some(19.0));
    assert_eq!(meta.playlist, None);
    assert!(meta.thumbnail_url.is_some());
}

#[test]
#[ignore = "asks YouTube"]
fn a_playlist_lookup_gives_the_playlist_not_its_first_video() {
    let meta = metadata::fetch(app(), PLAYLIST, &Lookup::default()).unwrap();
    assert!(meta.title.contains("Chapters Interactive"), "{meta:?}");
    assert_eq!(meta.playlist, Some(Some(13)), "{meta:?}");
    assert!(meta.thumbnail_url.is_some(), "{meta:?}");
}

/// A video link inside a playlist is the playlist by default, and just
/// the video with "Just this video", like the download itself.
#[test]
#[ignore = "asks YouTube"]
fn a_video_inside_a_playlist_follows_the_playlist_choice() {
    let whole = metadata::fetch(app(), VIDEO_IN_PLAYLIST, &Lookup::default()).unwrap();
    assert!(whole.playlist.is_some(), "{whole:?}");
    let single = Lookup {
        single_video: true,
        ..Lookup::default()
    };
    let video = metadata::fetch(app(), VIDEO_IN_PLAYLIST, &single).unwrap();
    assert_eq!(video.playlist, None, "{video:?}");
    assert!(video.duration.is_some(), "{video:?}");
}

/// A throwaway output folder, removed when the test is done with it.
mod tempdir {
    use std::path::{Path, PathBuf};

    pub struct Dir(PathBuf);

    impl Dir {
        pub fn new() -> Dir {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "mimirdlp-e2e-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Dir(dir)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
