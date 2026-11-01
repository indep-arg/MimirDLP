//! Saves what a live recording got so far when the user stops it.
//!
//! Stopping kills yt-dlp, which leaves `.part` files behind: one MPEG-TS
//! file when ffmpeg records the stream (even when it is named `.mkv.part`),
//! or a video and an audio file with `--live-from-start`. ffmpeg copies
//! either into a normal file without re-encoding. A video that isn't live
//! is never touched here: its `.part` file is what resumes it.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use tr::tr;

use crate::process;

/// Long enough to copy hours of a recording: nothing is re-encoded.
const SAVE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The file yt-dlp is writing to, from a `[download] Destination: ...`
/// line.
pub fn destination_from_line(line: &str) -> Option<PathBuf> {
    let path = line.strip_prefix("[download] Destination:")?.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// The name of the saved recording. With several streams their names carry
/// the format (`Title.f397.mp4`, `Title.f140.m4a`): the saved file is
/// `Title.mp4`, or `Title.mkv` when the streams use different containers.
pub fn saved_path(destinations: &[PathBuf]) -> Option<PathBuf> {
    let first = destinations.first()?;
    if destinations.len() == 1 {
        return Some(first.clone());
    }
    let extensions: Vec<_> = destinations.iter().map(|d| d.extension()).collect();
    let extension = if extensions.iter().all(|e| *e == extensions[0]) {
        extensions[0]?.to_string_lossy().into_owned()
    } else {
        "mkv".to_string()
    };
    let stem = first.file_stem()?.to_string_lossy();
    let stem = match stem.rsplit_once('.') {
        Some((title, format)) if is_format_id(format) => title.to_string(),
        _ => stem.into_owned(),
    };
    Some(first.with_file_name(format!("{stem}.{extension}")))
}

fn is_format_id(part: &str) -> bool {
    part.len() > 1
        && part.starts_with('f')
        && part[1..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The file that holds what was recorded for one destination.
fn recorded_file(destination: &Path) -> Option<PathBuf> {
    let part = part_path(destination);
    if is_non_empty(&part) {
        Some(part)
    } else if is_non_empty(destination) {
        Some(destination.to_path_buf())
    } else {
        None
    }
}

fn part_path(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

fn is_non_empty(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// Files yt-dlp leaves next to a stopped download: `X.part`, `X.ytdl` and
/// the fragments `X.part-Frag12.part`.
fn leftovers(destination: &Path) -> Vec<PathBuf> {
    let mut files = vec![part_path(destination)];
    let mut ytdl = destination.as_os_str().to_owned();
    ytdl.push(".ytdl");
    files.push(PathBuf::from(ytdl));
    if let (Some(dir), Some(name)) = (destination.parent(), destination.file_name()) {
        let fragment_prefix = format!("{}.part-Frag", name.to_string_lossy());
        if let Ok(entries) = fs::read_dir(dir) {
            files.extend(
                entries
                    .flatten()
                    .filter(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .starts_with(&fragment_prefix)
                    })
                    .map(|e| e.path()),
            );
        }
    }
    files
}

/// Waits, a few seconds at most, until the recorded files stop growing:
/// a writer may still be finishing its last bytes right after the kill.
fn wait_until_written(destinations: &[PathBuf]) {
    let size = || -> u64 {
        destinations
            .iter()
            .filter_map(|d| recorded_file(d))
            .filter_map(|f| fs::metadata(f).ok())
            .map(|m| m.len())
            .sum()
    };
    let mut last = size();
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(250));
        let now = size();
        if now == last {
            return;
        }
        last = now;
    }
}

/// Copies the recorded streams into one file with ffmpeg and removes the
/// partial files. Returns the path of the saved file.
pub fn save_recording(ffmpeg: &Path, destinations: &[PathBuf]) -> Result<PathBuf, String> {
    wait_until_written(destinations);
    let inputs: Vec<PathBuf> = destinations
        .iter()
        .filter_map(|d| recorded_file(d))
        .collect();
    if inputs.is_empty() {
        return Err(tr!("Nothing was recorded yet."));
    }
    let target = saved_path(destinations).ok_or_else(|| tr!("The recording has no file name."))?;
    let extension = target
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mkv".into());
    // ffmpeg picks the container from the extension, so it stays last.
    let temporary = target.with_extension(format!("saving.{extension}"));

    let mut command = process::command(ffmpeg);
    command.args(["-v", "error", "-y"]);
    for input in &inputs {
        command.arg("-i").arg(input);
    }
    // Only picture and sound: a stream may carry data tracks (ID3 tags,
    // for one) that other containers can't hold.
    for index in 0..inputs.len() {
        command
            .arg("-map")
            .arg(format!("{index}:v?"))
            .arg("-map")
            .arg(format!("{index}:a?"));
    }
    command.args(["-c", "copy"]).arg(&temporary);

    let output = process::run_captured(command, SAVE_TIMEOUT).map_err(|e| match e {
        process::RunError::Spawn(e) => tr!("Could not start FFmpeg: {e}", e),
        process::RunError::Wait(e) => e.to_string(),
        process::RunError::TimedOut => tr!("FFmpeg took too long to save the recording."),
    })?;
    if !output.status.success() {
        let _ = fs::remove_file(&temporary);
        let error = output
            .stderr
            .lines()
            .last()
            .unwrap_or("")
            .trim()
            .to_string();
        return Err(tr!("FFmpeg could not save the recording: {error}", error));
    }
    fs::rename(&temporary, &target)
        .map_err(|e| tr!("Could not move the recording into place: {e}", e))?;
    for destination in destinations {
        for file in leftovers(destination) {
            if file != target {
                let _ = fs::remove_file(file);
            }
        }
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::tempdir;

    #[test]
    fn the_destination_is_read_from_yt_dlps_line() {
        assert_eq!(
            destination_from_line("[download] Destination: /videos/News.mp4"),
            Some(PathBuf::from("/videos/News.mp4"))
        );
        assert_eq!(destination_from_line("[download] Destination: "), None);
        assert_eq!(
            destination_from_line("[info] Downloading 1 format(s): 0"),
            None
        );
    }

    #[test]
    fn one_stream_keeps_its_name() {
        let destinations = [PathBuf::from("/v/News live.mkv")];
        assert_eq!(
            saved_path(&destinations),
            Some(PathBuf::from("/v/News live.mkv"))
        );
        assert_eq!(saved_path(&[]), None);
    }

    #[test]
    fn separate_streams_lose_the_format_in_the_name() {
        let same = [
            PathBuf::from("/v/News.f397.mp4"),
            PathBuf::from("/v/News.f140.mp4"),
        ];
        assert_eq!(saved_path(&same), Some(PathBuf::from("/v/News.mp4")));
        let mixed = [
            PathBuf::from("/v/A.b.f303.webm"),
            PathBuf::from("/v/A.b.f140.m4a"),
        ];
        assert_eq!(saved_path(&mixed), Some(PathBuf::from("/v/A.b.mkv")));
    }

    #[test]
    fn only_format_ids_are_removed() {
        assert!(is_format_id("f397"));
        assert!(is_format_id("f303-1"));
        assert!(!is_format_id("f"));
        assert!(!is_format_id("final cut"));
    }

    #[test]
    fn the_files_a_stopped_download_leaves_are_found() {
        let dir = tempdir();
        let destination = dir.join("News.f397.mp4");
        for name in [
            "News.f397.mp4.part",
            "News.f397.mp4.ytdl",
            "News.f397.mp4.part-Frag50.part",
            "Other.mp4.part",
        ] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let mut found: Vec<_> = leftovers(&destination)
            .into_iter()
            .filter(|f| f.exists())
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                "News.f397.mp4.part",
                "News.f397.mp4.part-Frag50.part",
                "News.f397.mp4.ytdl"
            ]
        );
        assert_eq!(
            recorded_file(&destination),
            Some(dir.join("News.f397.mp4.part"))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nothing_recorded_is_an_error() {
        let dir = tempdir();
        let result = save_recording(Path::new("ffmpeg"), &[dir.join("News.mkv")]);
        assert_eq!(result, Err("Nothing was recorded yet.".to_string()));
        fs::remove_dir_all(dir).unwrap();
    }

    /// A stand-in ffmpeg that writes its last argument (the output), so
    /// the whole save can be followed: every recorded stream is an input,
    /// the result is renamed into place and the leftovers are removed.
    #[cfg(unix)]
    #[test]
    fn a_recording_is_copied_into_one_file_and_the_leftovers_go() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = crate::testing::exec_guard();
        let dir = tempdir();
        let ffmpeg = dir.join("ffmpeg");
        let args = dir.join("args");
        fs::write(
            &ffmpeg,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > {}\n\
                 for last; do :; done\necho saved > \"$last\"\n",
                args.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&ffmpeg, fs::Permissions::from_mode(0o755)).unwrap();
        let video = dir.join("Live [id].f299.mp4");
        let audio = dir.join("Live [id].f140.m4a");
        for leftover in [
            "Live [id].f299.mp4.part",
            "Live [id].f299.mp4.ytdl",
            "Live [id].f140.m4a.part",
            "Live [id].f140.m4a.part-Frag7.part",
        ] {
            fs::write(dir.join(leftover), b"data").unwrap();
        }

        let saved = save_recording(&ffmpeg, &[video, audio]).unwrap();
        assert_eq!(saved, dir.join("Live [id].mkv"));
        assert_eq!(fs::read_to_string(&saved).unwrap(), "saved\n");
        let args = fs::read_to_string(&args).unwrap();
        let args: Vec<&str> = args.lines().collect();
        let inputs: Vec<&str> = args
            .windows(2)
            .filter(|w| w[0] == "-i")
            .map(|w| w[1])
            .collect();
        assert_eq!(inputs.len(), 2, "{args:?}");
        assert!(args.windows(2).any(|w| w == ["-c", "copy"]), "{args:?}");
        let mut left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["Live [id].mkv", "args", "ffmpeg"]);
        fs::remove_dir_all(dir).unwrap();
    }

    /// A failed copy leaves the recorded files alone, so nothing is lost.
    #[cfg(unix)]
    #[test]
    fn a_failed_save_keeps_what_was_recorded() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = crate::testing::exec_guard();
        let dir = tempdir();
        let ffmpeg = dir.join("ffmpeg");
        fs::write(
            &ffmpeg,
            "#!/bin/sh\necho 'Invalid data found' >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&ffmpeg, fs::Permissions::from_mode(0o755)).unwrap();
        let destination = dir.join("Live.mkv");
        fs::write(dir.join("Live.mkv.part"), b"data").unwrap();

        let error = save_recording(&ffmpeg, &[destination]).unwrap_err();
        assert!(error.contains("Invalid data found"), "{error}");
        assert!(dir.join("Live.mkv.part").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
