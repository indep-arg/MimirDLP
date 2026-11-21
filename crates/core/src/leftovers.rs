//! Files a download leaves behind that nobody asked for, removed when the
//! run ends.
//!
//! - A postprocessor stopped halfway. `ExtractAudio` and the video
//!   converters write straight to the final name, so a Stop during the
//!   conversion leaves a truncated `Title.mp3` that looks finished, and
//!   that yt-dlp itself skips the next time as "already downloaded". The
//!   others write `Title.temp.ext` and rename it when done.
//! - A video's thumbnail. Osprey only asks for them to embed them, and
//!   yt-dlp deletes them once embedded, so one still there after the run
//!   belongs to a download that failed or was stopped. A playlist's own
//!   thumbnail, which nothing embeds, stays where it always did.
//!
//! The `.part` files of a stopped download are not touched: they are what
//! resumes it. Neither is anything yt-dlp reported as saved, or found
//! already downloaded.
//!
//! Everything read here comes down yt-dlp's stdout, in order: with
//! `--no-quiet` only warnings and errors go to stderr.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

/// Postprocessors that write their output under its final name.
const IN_PLACE: [&str; 3] = ["ExtractAudio", "VideoConvertor", "VideoRemuxer"];

/// Postprocessors that write `name.temp.ext` and rename it when done, with
/// what comes before the file name in their line. The name is quoted except
/// for `ModifyChapters`.
const VIA_TEMP: [(&str, &str); 5] = [
    ("Merger", "Merging formats into "),
    ("EmbedThumbnail", "Adding thumbnail to "),
    ("Metadata", "Adding metadata to "),
    ("EmbedSubtitle", "Embedding subtitles in "),
    ("ModifyChapters", "Removing chapters from "),
];

#[derive(Debug, Default)]
pub struct Leftovers {
    /// Being written by a postprocessor that hadn't said it finished, with
    /// that postprocessor's name.
    unfinished: Vec<(String, PathBuf)>,
    /// A postprocessor's file that never said it finished, from a video
    /// yt-dlp went on from anyway: to the next video, or (with
    /// `--ignore-errors`) to moving the source into place.
    abandoned: Vec<PathBuf>,
    thumbnails: Vec<PathBuf>,
    /// What was downloaded, or found already downloaded, in this run.
    downloaded: Vec<PathBuf>,
    /// Reported by `--print after_move:filepath`: never removed.
    saved: Vec<PathBuf>,
}

impl Leftovers {
    /// Takes note of what one line of yt-dlp's output says is being
    /// written.
    pub fn note(&mut self, line: &str) {
        if let Some(path) = line.strip_prefix("[download] Destination:") {
            // A new download while a postprocessor's file is still pending:
            // the video before failed during postprocessing and yt-dlp went
            // on to the next one.
            self.abandoned
                .extend(self.unfinished.drain(..).map(|(_, path)| path));
            self.downloaded.push(PathBuf::from(path.trim()));
            return;
        }
        // A file from an earlier run, which the postprocessors still work
        // on. Complete: it must never be taken for one of theirs.
        if let Some(path) = line
            .strip_prefix("[download] ")
            .and_then(|rest| rest.strip_suffix(" has already been downloaded"))
        {
            self.downloaded.push(PathBuf::from(path));
            return;
        }
        if let Some(path) = thumbnail_written(line) {
            self.thumbnails.push(path);
            return;
        }
        if let Some(path) = thumbnail_converted(line) {
            self.thumbnails.push(path);
            return;
        }
        if let Some(path) = thumbnail_renamed(line) {
            self.thumbnails.push(path);
            return;
        }
        if let Some(path) = keyframes_file(line) {
            self.unfinished.push(("ModifyChapters".into(), path));
            return;
        }
        if let Some((name, path)) = postprocessor_output(line) {
            // ExtractAudio re-encoding a file that already has the target
            // extension names the downloaded file itself, and writes a
            // .temp next to it: the download is complete and stays.
            let path = if self.downloaded.contains(&path) {
                temp_name(&path)
            } else {
                path
            };
            self.unfinished.push((name.to_string(), path));
        }
    }

    /// The postprocessor `name` finished: what it wrote is complete.
    pub fn finished(&mut self, name: &str) {
        self.unfinished.retain(|(pp, _)| pp != name);
    }

    /// yt-dlp moved a file into place. A postprocessor that finished said
    /// so before this (both come down stdout, in order), so one that
    /// didn't failed, and `--ignore-errors` went on without it: what it
    /// left is half-written.
    pub fn saved(&mut self, path: &str) {
        self.abandoned
            .extend(self.unfinished.drain(..).map(|(_, path)| path));
        self.saved.push(PathBuf::from(path));
    }

    /// Removes what is left. Returns what was removed and what couldn't
    /// be. A file that is still held open (Windows, for a moment after a
    /// kill) is tried again a few times.
    pub fn remove(self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let saved = self.saved;
        let downloaded = self.downloaded;
        let candidates = self
            .unfinished
            .into_iter()
            .map(|(_, path)| path)
            .chain(self.abandoned)
            .chain(self.thumbnails);
        let (mut removed, mut stuck) = (Vec::new(), Vec::new());
        for path in candidates {
            if saved.contains(&path)
                || downloaded.contains(&path)
                || removed.contains(&path)
                || stuck.contains(&path)
                || !path.is_file()
            {
                continue;
            }
            if remove_file(&path) {
                removed.push(path);
            } else {
                stuck.push(path);
            }
        }
        (removed, stuck)
    }
}

fn remove_file(path: &Path) -> bool {
    for attempt in 0..5 {
        if fs::remove_file(path).is_ok() || !path.exists() {
            return true;
        }
        if attempt < 4 {
            thread::sleep(Duration::from_millis(200));
        }
    }
    false
}

/// `[info] Writing video thumbnail 32 to: /dir/Title [id].webp`. Not a
/// playlist's thumbnail ("Writing playlist thumbnail"): that one is kept.
fn thumbnail_written(line: &str) -> Option<PathBuf> {
    let rest = line.strip_prefix("[info] Writing video thumbnail ")?;
    let (_, path) = rest.split_once(" to: ")?;
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// `[ThumbnailsConvertor] Converting thumbnail "/dir/Title.webp" to jpg`:
/// the converted file is `/dir/Title.jpg`.
fn thumbnail_converted(line: &str) -> Option<PathBuf> {
    let rest = line.strip_prefix("[ThumbnailsConvertor] Converting thumbnail \"")?;
    let (path, format) = rest.rsplit_once("\" to ")?;
    let format = format.trim();
    if path.is_empty() || format.is_empty() || format.contains(char::is_whitespace) {
        return None;
    }
    Some(Path::new(path).with_extension(format))
}

/// `Correcting thumbnail "/dir/Title.jpg" extension to webp`: the file is
/// now `/dir/Title.webp`.
fn thumbnail_renamed(line: &str) -> Option<PathBuf> {
    let (_, rest) = line.split_once("] Correcting thumbnail \"")?;
    let (path, ext) = rest.rsplit_once("\" extension to ")?;
    let ext = ext.trim();
    (!path.is_empty() && !ext.is_empty()).then(|| Path::new(path).with_extension(ext))
}

/// `[ModifyChapters] Re-encoding "/dir/Title.mkv" with appropriate
/// keyframes`, into `/dir/Title.keyframes.temp.mkv`.
fn keyframes_file(line: &str) -> Option<PathBuf> {
    let (_, rest) = line.split_once("] Re-encoding \"")?;
    let path = Path::new(rest.strip_suffix("\" with appropriate keyframes")?);
    let mut name = path.file_stem()?.to_os_string();
    name.push(".keyframes.temp");
    if let Some(ext) = path.extension() {
        name.push(".");
        name.push(ext);
    }
    Some(path.with_file_name(name))
}

/// The file a postprocessor is about to write, from the line it prints
/// before starting ffmpeg.
fn postprocessor_output(line: &str) -> Option<(&str, PathBuf)> {
    let rest = line.strip_prefix('[')?;
    let (name, message) = rest.split_once("] ")?;
    if IN_PLACE.contains(&name) {
        // "Destination: X", or "Converting video from webm to mp4;
        // Destination: X". The first one: the title in the path may hold
        // the same text.
        let (_, path) = message.split_once("Destination: ")?;
        return (!path.is_empty()).then(|| (name, PathBuf::from(path)));
    }
    // FixupM4a, FixupM3u8 and the rest: "Correcting container of "X"".
    if name.starts_with("Fixup") {
        let (_, path) = message.split_once(" of \"")?;
        let path = path.strip_suffix('"')?;
        return (!path.is_empty()).then(|| (name, temp_name(Path::new(path))));
    }
    let (_, lead) = VIA_TEMP.iter().find(|(pp, _)| *pp == name)?;
    // EmbedThumbnail says which program it uses first: "ffmpeg: Adding...".
    let (_, path) = message.split_once(lead)?;
    let path = path
        .strip_prefix('"')
        .and_then(|p| p.strip_suffix('"'))
        .unwrap_or(path);
    (!path.is_empty()).then(|| (name, temp_name(Path::new(path))))
}

/// yt-dlp's `prepend_extension(path, 'temp')`: `Title.mkv` becomes
/// `Title.temp.mkv`.
fn temp_name(path: &Path) -> PathBuf {
    let mut name = path.file_stem().unwrap_or_default().to_os_string();
    name.push(".temp");
    if let Some(ext) = path.extension() {
        name.push(".");
        name.push(ext);
    }
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::tempdir;

    /// Lines recorded from a real run that was stopped while converting to
    /// MP3 (SoundCloud, audio only).
    #[test]
    fn a_stop_during_the_audio_conversion_removes_the_truncated_file() {
        let dir = tempdir();
        let thumb = dir.join("Flickermood [293].jpg");
        let source = dir.join("Flickermood [293].m4a");
        let mp3 = dir.join("Flickermood [293].mp3");
        for f in [&thumb, &source, &mp3] {
            fs::write(f, "x").unwrap();
        }
        let mut leftovers = Leftovers::default();
        for line in [
            format!(
                "[info] Writing video thumbnail original to: {}",
                thumb.display()
            ),
            format!("[download] Destination: {}", source.display()),
            format!("[ExtractAudio] Destination: {}", mp3.display()),
        ] {
            leftovers.note(&line);
        }
        let (mut removed, stuck) = leftovers.remove();
        removed.sort();
        assert_eq!(removed, vec![thumb.clone(), mp3.clone()]);
        assert!(stuck.is_empty());
        // The source stays: the next run converts it again.
        assert!(source.exists() && !mp3.exists() && !thumb.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_finished_file_is_never_removed() {
        let dir = tempdir();
        let mp3 = dir.join("Song [1].mp3");
        fs::write(&mp3, "x").unwrap();
        let line = format!("[ExtractAudio] Destination: {}", mp3.display());

        // The postprocessor said it finished, then the file was saved.
        let mut leftovers = Leftovers::default();
        leftovers.note(&line);
        leftovers.finished("ExtractAudio");
        leftovers.saved(&mp3.display().to_string());
        assert!(leftovers.remove().0.is_empty());

        // yt-dlp reported the file as saved.
        let mut leftovers = Leftovers::default();
        leftovers.note(&line);
        leftovers.saved(&mp3.display().to_string());
        assert!(leftovers.remove().0.is_empty());

        // Saved after the next video's download started.
        let mut leftovers = Leftovers::default();
        leftovers.note(&line);
        leftovers.note(&format!(
            "[download] Destination: {}",
            dir.join("b.m4a").display()
        ));
        leftovers.saved(&mp3.display().to_string());
        assert!(leftovers.remove().0.is_empty());

        assert!(mp3.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    /// WAV is always re-encoded, also when the file is there from an
    /// earlier run: the destination is that complete file.
    #[test]
    fn a_file_already_downloaded_is_never_taken_for_a_half_written_one() {
        let dir = tempdir();
        let wav = dir.join("Song.wav");
        fs::write(&wav, "x").unwrap();
        let mut leftovers = Leftovers::default();
        leftovers.note(&format!(
            "[download] {} has already been downloaded",
            wav.display()
        ));
        leftovers.note(&format!("[ExtractAudio] Destination: {}", wav.display()));
        assert!(leftovers.remove().0.is_empty());
        assert!(wav.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    /// An MP3 re-encoded to MP3: yt-dlp names the download itself as the
    /// destination and writes `.temp.mp3`. Only that goes.
    #[test]
    fn re_encoding_to_the_same_extension_keeps_the_download() {
        let dir = tempdir();
        let mp3 = dir.join("Song.mp3");
        let temp = dir.join("Song.temp.mp3");
        fs::write(&mp3, "x").unwrap();
        fs::write(&temp, "x").unwrap();
        let mut leftovers = Leftovers::default();
        leftovers.note(&format!("[download] Destination: {}", mp3.display()));
        leftovers.note(&format!("[ExtractAudio] Destination: {}", mp3.display()));
        assert_eq!(leftovers.remove().0, vec![temp]);
        assert!(mp3.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    /// With --ignore-errors a failed conversion doesn't stop yt-dlp: it
    /// moves the source into place and reports that, never the MP3.
    #[test]
    fn a_failed_conversion_that_yt_dlp_went_past_loses_its_file() {
        let dir = tempdir();
        let source = dir.join("Song.m4a");
        let mp3 = dir.join("Song.mp3");
        fs::write(&source, "x").unwrap();
        fs::write(&mp3, "half").unwrap();
        let mut leftovers = Leftovers::default();
        leftovers.note(&format!("[download] Destination: {}", source.display()));
        leftovers.note(&format!("[ExtractAudio] Destination: {}", mp3.display()));
        leftovers.saved(&source.display().to_string());
        assert_eq!(leftovers.remove().0, vec![mp3]);
        assert!(source.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_video_abandoned_during_postprocessing_loses_its_half_written_file() {
        let dir = tempdir();
        let first = dir.join("One.mp3");
        fs::write(&first, "x").unwrap();
        let mut leftovers = Leftovers::default();
        leftovers.note(&format!("[ExtractAudio] Destination: {}", first.display()));
        // yt-dlp moved on to the next video, which then finished.
        leftovers.note(&format!(
            "[download] Destination: {}",
            dir.join("Two.m4a").display()
        ));
        leftovers.finished("ExtractAudio");
        leftovers.saved(&dir.join("Two.mp3").display().to_string());
        assert_eq!(leftovers.remove().0, vec![first.clone()]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn postprocessors_that_write_a_temp_file_leave_the_real_one_alone() {
        let dir = tempdir();
        let video = dir.join("Clip [x].mkv");
        let temp = dir.join("Clip [x].temp.mkv");
        fs::write(&video, "x").unwrap();
        fs::write(&temp, "x").unwrap();
        let mut leftovers = Leftovers::default();
        leftovers.note(&format!(
            "[EmbedThumbnail] ffmpeg: Adding thumbnail to \"{}\"",
            video.display()
        ));
        assert_eq!(leftovers.remove().0, vec![temp.clone()]);
        assert!(video.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_file_each_postprocessor_writes_is_recognised() {
        let file = |line: &str| postprocessor_output(line).map(|(name, p)| (name.to_string(), p));
        assert_eq!(
            file("[VideoRemuxer] Remuxing video from webm to mkv; Destination: /d/a b.mkv"),
            Some(("VideoRemuxer".into(), PathBuf::from("/d/a b.mkv")))
        );
        assert_eq!(
            file("[VideoConvertor] Converting video from mkv to mp4; Destination: /d/a.mp4"),
            Some(("VideoConvertor".into(), PathBuf::from("/d/a.mp4")))
        );
        assert_eq!(
            file("[Merger] Merging formats into \"/d/a [id].mkv\""),
            Some(("Merger".into(), PathBuf::from("/d/a [id].temp.mkv")))
        );
        assert_eq!(
            file("[Metadata] Adding metadata to \"/d/a.mp3\""),
            Some(("Metadata".into(), PathBuf::from("/d/a.temp.mp3")))
        );
        assert_eq!(
            file("[EmbedSubtitle] Embedding subtitles in \"/d/a.mkv\""),
            Some(("EmbedSubtitle".into(), PathBuf::from("/d/a.temp.mkv")))
        );
        assert_eq!(
            file("[ModifyChapters] Removing chapters from /d/a.mkv"),
            Some(("ModifyChapters".into(), PathBuf::from("/d/a.temp.mkv")))
        );
        assert_eq!(
            file("[FixupM4a] Correcting container of \"/d/a.m4a\""),
            Some(("FixupM4a".into(), PathBuf::from("/d/a.temp.m4a")))
        );
        assert_eq!(file("[download] Destination: /d/a.mkv"), None);
        assert_eq!(
            file(
                "[ExtractAudio] Not converting audio /d/a.mp3; file is already in target format mp3"
            ),
            None
        );
    }

    /// The title is part of the path, and may hold what the line is
    /// searched for.
    #[test]
    fn a_title_that_looks_like_the_message_does_not_confuse_it() {
        let line = "[ExtractAudio] Destination: /d/Destination: the movie.mp3";
        assert_eq!(
            postprocessor_output(line).map(|(_, p)| p),
            Some(PathBuf::from("/d/Destination: the movie.mp3"))
        );
        let line = "[FixupM4a] Correcting container of \"/d/Best of \"me\".m4a\"";
        assert_eq!(
            postprocessor_output(line).map(|(_, p)| p),
            Some(PathBuf::from("/d/Best of \"me\".temp.m4a"))
        );
    }

    #[test]
    fn thumbnails_are_recognised_written_converted_and_renamed() {
        assert_eq!(
            thumbnail_written("[info] Writing video thumbnail 32 to: /d/Me [id].webp"),
            Some(PathBuf::from("/d/Me [id].webp"))
        );
        // A playlist's own thumbnail is nothing anybody embeds: it stays.
        assert_eq!(
            thumbnail_written("[info] Writing playlist thumbnail original to: /d/List.jpg"),
            None
        );
        assert_eq!(
            thumbnail_written("[info] Writing video subtitles to: /d/a.en.vtt"),
            None
        );
        assert_eq!(
            thumbnail_converted(
                "[ThumbnailsConvertor] Converting thumbnail \"/d/Me [id].webp\" to jpg"
            ),
            Some(PathBuf::from("/d/Me [id].jpg"))
        );
        assert_eq!(
            thumbnail_renamed(
                "[ThumbnailsConvertor] Correcting thumbnail \"/d/Me.jpg\" extension to webp"
            ),
            Some(PathBuf::from("/d/Me.webp"))
        );
        assert_eq!(
            keyframes_file(
                "[ModifyChapters] Re-encoding \"/d/a [x].mkv\" with appropriate keyframes"
            ),
            Some(PathBuf::from("/d/a [x].keyframes.temp.mkv"))
        );
    }
}
