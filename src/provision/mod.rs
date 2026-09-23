//! Downloads, verifies and installs yt-dlp, FFmpeg and Deno into `bin/`
//! next to the application, tracking what was installed in a small
//! `bin/.install_state` file (needed because FFmpeg and Deno publish the
//! checksum of their archive, which is gone after extraction; see
//! `check_update` below). Progress is reported through [`Event`]s.
//!
//! Everything here is blocking and GUI-agnostic; the GUI runs it on a worker
//! thread and forwards the events.

mod archive;
pub(crate) mod download;
mod platform;
pub(crate) mod signature;

use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

pub use platform::supported;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    YtDlp,
    Ffmpeg,
    Deno,
}

impl Component {
    pub const ALL: [Component; 3] = [Component::YtDlp, Component::Ffmpeg, Component::Deno];

    pub fn label(self) -> &'static str {
        match self {
            Component::YtDlp => "yt-dlp",
            Component::Ffmpeg => "FFmpeg",
            Component::Deno => "Deno",
        }
    }

    /// Executables this component puts in `bin/` (without `.exe`).
    pub fn binaries(self) -> &'static [&'static str] {
        match self {
            Component::YtDlp => &["yt-dlp"],
            Component::Ffmpeg => &["ffmpeg", "ffprobe"],
            Component::Deno => &["deno"],
        }
    }
}

/// Where everything lives. Portable: all of it sits next to the executable.
#[derive(Debug, Clone)]
pub struct Paths {
    pub app_dir: PathBuf,
    pub bin_dir: PathBuf,
}

impl Paths {
    pub fn new(app_dir: PathBuf) -> Self {
        let bin_dir = app_dir.join("bin");
        Paths { app_dir, bin_dir }
    }

    /// The directory of the running executable, symlinks resolved.
    /// `YTP_APP_DIR` overrides it, which is only meant for development
    /// (`cargo run` would otherwise use `target/debug/`).
    pub fn for_current_exe() -> io::Result<Self> {
        if let Some(dir) = std::env::var_os("YTP_APP_DIR") {
            return Ok(Paths::new(PathBuf::from(dir)));
        }
        let exe = plain_path(std::env::current_exe()?.canonicalize()?);
        let dir = exe
            .parent()
            .ok_or_else(|| io::Error::other("executable has no parent directory"))?;
        Ok(Paths::new(dir.to_path_buf()))
    }

    pub fn tool(&self, name: &str) -> PathBuf {
        self.bin_dir
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    }

    fn state_file(&self) -> PathBuf {
        self.bin_dir.join(".install_state")
    }
}

/// Drops the `\\?\` that Windows' `canonicalize` puts in front of a path.
///
/// It is a valid path for Rust, but it travels badly: it ends up in
/// `--ffmpeg-location`, in `-P` and in the log, where it confuses both
/// yt-dlp's own path handling and the person reading. Nothing on Linux
/// starts with it, so this needs no `cfg`.
///
/// A network share comes back as `\\?\UNC\server\share`, which has to
/// become `\\server\share`: dropping only the prefix would leave a
/// relative `UNC\server\share`.
fn plain_path(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{share}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

/// The log tags (`[INSTALL]`, `[DOWNLOAD]`, ...), shared with the downloader.
pub use crate::log::Level;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Log(Level, String),
    /// Progress of the file currently downloading.
    Transfer {
        file: String,
        received: u64,
        total: Option<u64>,
    },
    TransferFinished,
}

pub type Emit<'a> = &'a mut dyn FnMut(Event);

#[derive(Debug)]
pub enum Error {
    Unsupported,
    Io(String),
    Download(String),
    Checksum(String),
    Archive(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported => write!(
                f,
                "This platform isn't supported (only Linux and Windows on x86_64)."
            ),
            Error::Io(m) | Error::Download(m) | Error::Checksum(m) | Error::Archive(m) => {
                f.write_str(m)
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Missing,
    /// Present but wouldn't run (truncated, wrong architecture...).
    Broken,
    Installed {
        version: String,
    },
}

/// What is installed, based on actually running the binaries.
pub fn inspect(component: Component, paths: &Paths) -> Status {
    for name in component.binaries() {
        if !paths.tool(name).is_file() {
            return Status::Missing;
        }
    }
    let main = paths.tool(component.binaries()[0]);
    let arg = if component == Component::Ffmpeg {
        "-version"
    } else {
        "--version"
    };
    match run_for_output(&main, arg, Duration::from_secs(30)) {
        Some(out) => match parse_version(component, &out) {
            Some(version) => Status::Installed { version },
            None => Status::Broken,
        },
        None => Status::Broken,
    }
}

fn run_for_output(program: &Path, arg: &str, limit: Duration) -> Option<String> {
    let mut command = crate::process::command(program);
    command.arg(arg);
    let output = crate::process::run_captured(command, limit).ok()?;
    output.status.success().then_some(output.stdout)
}

/// Extracts the version from each tool's own `--version`/`-version` output.
pub fn parse_version(component: Component, output: &str) -> Option<String> {
    let first = output.lines().next()?.trim();
    let version = match component {
        Component::YtDlp => first,
        // "ffmpeg version N-126495-g3a165c77dc-20260910 Copyright ..."
        Component::Ffmpeg => first
            .strip_prefix("ffmpeg version ")?
            .split_whitespace()
            .next()?,
        // "deno 2.9.6 (stable, release, x86_64-unknown-linux-gnu)"
        Component::Deno => first.split_whitespace().nth(1)?,
    };
    (!version.is_empty()).then(|| version.to_string())
}

/// After this many days an installed yt-dlp is worth updating: sites
/// change their pages all the time and extractors follow within days.
pub const YTDLP_STALE_DAYS: i64 = 14;

/// How old a yt-dlp build is, from its version, which is its build date
/// (`2026.09.16` or a nightly's `2026.09.16.232951`). `None` for anything
/// else rather than a guess.
pub fn ytdlp_age_days(version: &str, today: i64) -> Option<i64> {
    let mut parts = version.split('.');
    let mut next = || parts.next()?.parse::<i64>().ok();
    let (year, month, day) = (next()?, next()?, next()?);
    if !(2000..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((today - days_from_civil(year, month, day)).max(0))
}

/// Days since 1970-01-01 today, in UTC.
pub fn today() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (secs / 86_400) as i64
}

/// Days since 1970-01-01 for a calendar date (Howard Hinnant's
/// `days_from_civil`), so no date crate is needed for one subtraction.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ---------------------------------------------------------------------------
// Install state (a small key=value file)
// ---------------------------------------------------------------------------

pub fn read_state(paths: &Paths, key: &str) -> Option<String> {
    let text = fs::read_to_string(paths.state_file()).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
        .map(str::to_string)
        .filter(|v| !v.is_empty())
}

pub fn write_state(paths: &Paths, key: &str, value: &str) -> io::Result<()> {
    let file = paths.state_file();
    let mut lines: Vec<String> = fs::read_to_string(&file)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.split_once('=').map(|(k, _)| k) != Some(key))
        .map(str::to_string)
        .collect();
    lines.push(format!("{key}={value}"));
    let tmp = file.with_extension("tmp");
    fs::write(&tmp, lines.join("\n") + "\n")?;
    fs::rename(tmp, file)
}

// ---------------------------------------------------------------------------
// Update checks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheck {
    Current,
    Outdated,
    /// Installed before update tracking existed (no recorded checksum).
    Unknown,
    Failed(String),
}

/// The checksum file, with its signature checked when upstream signs one.
/// Both are fetched from "latest", which can move between the two
/// requests, so a signature that doesn't match is fetched once more
/// before it is believed.
fn fetch_sums(source: &platform::Source) -> Result<String, Error> {
    let Some(signer) = &source.signed_by else {
        return download::fetch_text(source.checksum_url);
    };
    let attempt = || -> Result<String, Error> {
        let sums = download::fetch_text(source.checksum_url)?;
        let sig = download::fetch_bytes(signer.url)
            .map_err(|e| Error::Checksum(format!("Could not get the signature: {e}")))?;
        signature::verify(sums.as_bytes(), &sig, signer)?;
        Ok(sums)
    };
    attempt().or_else(|_| attempt())
}

pub fn check_update(component: Component, paths: &Paths) -> UpdateCheck {
    let Some(source) = platform::source(component) else {
        return UpdateCheck::Failed(Error::Unsupported.to_string());
    };
    let published =
        match fetch_sums(&source).and_then(|text| source.checksum.find(&text, source.asset)) {
            Ok(hash) => hash,
            Err(e) => return UpdateCheck::Failed(e.to_string()),
        };
    let installed = match source.state_key {
        // yt-dlp publishes the checksum of the binary itself: hash the file.
        None => match sha256_file(&paths.tool(component.binaries()[0])) {
            Ok(h) => h,
            Err(e) => return UpdateCheck::Failed(e.to_string()),
        },
        // FFmpeg and Deno publish the checksum of the archive, which is gone
        // after extraction, so compare with what was recorded at install.
        Some(key) => match read_state(paths, key) {
            Some(h) => h,
            None => return UpdateCheck::Unknown,
        },
    };
    if installed.eq_ignore_ascii_case(&published) {
        UpdateCheck::Current
    } else {
        UpdateCheck::Outdated
    }
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Install
// ---------------------------------------------------------------------------

/// Whether the application folder can be written to, which everything
/// portable depends on (`bin/`, the settings, the history). It can't in
/// `C:\Program Files` without administrator rights, or on a read-only
/// mount, and the only fix is moving the folder.
pub fn app_dir_writable(paths: &Paths) -> io::Result<()> {
    let probe = paths
        .app_dir
        .join(format!(".write-test-{}", std::process::id()));
    fs::write(&probe, b"")?;
    fs::remove_file(probe)
}

/// Removes leftovers of an interrupted run (partial downloads, half-written
/// binaries). Only files this module creates, identified by their prefix.
pub fn clean_leftovers(paths: &Paths) {
    let Ok(entries) = fs::read_dir(&paths.bin_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".download-") || name.starts_with(".tmp-") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The last 16 hex digits, grouped the way `gpg` prints fingerprints.
fn short_fingerprint(fingerprint: &str) -> String {
    let tail = &fingerprint[fingerprint.len().saturating_sub(16)..];
    tail.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Logs an error before passing it on, so every failure shows up in the log.
fn logged<T>(emit: Emit<'_>, result: Result<T, Error>) -> Result<T, Error> {
    if let Err(e) = &result {
        emit(Event::Log(Level::Error, e.to_string()));
    }
    result
}

/// Downloads, verifies and installs one component.
/// Every failure is already in the log when this returns an error.
pub fn install(component: Component, paths: &Paths, emit: Emit<'_>) -> Result<(), Error> {
    let source = logged(emit, platform::source(component).ok_or(Error::Unsupported))?;
    install_source(component, &source, paths, emit)
}

fn install_source(
    component: Component,
    source: &platform::Source,
    paths: &Paths,
    emit: Emit<'_>,
) -> Result<(), Error> {
    let created = fs::create_dir_all(&paths.bin_dir).map_err(|e| {
        Error::Io(format!(
            "Cannot create directory: {} ({e})",
            paths.bin_dir.display()
        ))
    });
    logged(emit, created)?;

    emit(Event::Log(
        Level::Install,
        format!("{}...", component.label()),
    ));

    let sums = download::fetch_text_logged(source.checksum_url, emit)?;
    let sums = match &source.signed_by {
        None => sums,
        Some(signer) => {
            emit(Event::Log(
                Level::Verify,
                format!(
                    "Checking who signed {}...",
                    download::file_name(source.checksum_url)
                ),
            ));
            // The first copy may predate a release published a moment
            // later; `fetch_sums` asks again once before giving up.
            let verified = download::fetch_bytes(signer.url)
                .map_err(|e| Error::Checksum(format!("Could not get the signature: {e}")))
                .and_then(|sig| signature::verify(sums.as_bytes(), &sig, signer).map(|()| sums))
                .or_else(|_| fetch_sums(source));
            let sums = logged(emit, verified)?;
            emit(Event::Log(
                Level::Ok,
                format!(
                    "Signed by the expected key ({})",
                    short_fingerprint(signer.fingerprint)
                ),
            ));
            sums
        }
    };
    let mut expected = logged(emit, source.checksum.find(&sums, source.asset))?;

    let part = paths.bin_dir.join(format!(".download-{}", source.asset));
    let actual = download::download_to_file(source.url, source.asset, &part, emit)?;

    emit(Event::Log(
        Level::Verify,
        format!("Checking SHA256 for {}...", source.asset),
    ));
    if !actual.eq_ignore_ascii_case(&expected) {
        // "latest" can move while a large file downloads (FFmpeg-Builds
        // replaces its assets daily), so the file may be a newer build
        // than the checksum fetched before it. Ask once more before
        // calling it a mismatch.
        // Signed like the first one: this path must not be a way around
        // the signature check.
        if let Ok(sums) = fetch_sums(source)
            && let Ok(refreshed) = source.checksum.find(&sums, source.asset)
            && refreshed.eq_ignore_ascii_case(&actual)
        {
            emit(Event::Log(
                Level::Info,
                "A newer release was published during the download; its checksum matches.".into(),
            ));
            expected = refreshed;
        }
    }
    if !actual.eq_ignore_ascii_case(&expected) {
        let _ = fs::remove_file(&part);
        emit(Event::Log(
            Level::Error,
            format!("Hash mismatch for {}!", source.asset),
        ));
        emit(Event::Log(Level::Info, format!("Expected: {expected}")));
        emit(Event::Log(Level::Info, format!("Actual:   {actual}")));
        return Err(Error::Checksum(format!(
            "The download of {} didn't match its published checksum.",
            source.asset
        )));
    }
    emit(Event::Log(Level::Ok, "Hash verification passed".into()));

    if !matches!(source.packaging, platform::Packaging::Raw { .. }) {
        emit(Event::Log(Level::Info, "Extracting...".into()));
    }
    let result = archive::install_from(&source.packaging, &part, paths);
    let _ = fs::remove_file(&part);
    logged(emit, result)?;

    if let Some(key) = source.state_key {
        logged(
            emit,
            write_state(paths, key, &expected).map_err(Error::from),
        )?;
    }
    emit(Event::Log(
        Level::Ok,
        format!("{} installed successfully", component.label()),
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::tempdir;
    use sha2::{Digest, Sha256};

    #[test]
    fn versions_are_parsed_from_each_tools_own_output() {
        assert_eq!(
            parse_version(Component::YtDlp, "2026.08.30.232658\n").as_deref(),
            Some("2026.08.30.232658")
        );
        assert_eq!(
            parse_version(
                Component::Ffmpeg,
                "ffmpeg version N-126495-g3a165c77dc-20260910 Copyright (c) 2000-2026\nbuilt with gcc"
            )
            .as_deref(),
            Some("N-126495-g3a165c77dc-20260910")
        );
        assert_eq!(
            parse_version(
                Component::Deno,
                "deno 2.9.6 (stable, release, x86_64-unknown-linux-gnu)\nv8 15.0"
            )
            .as_deref(),
            Some("2.9.6")
        );
        assert_eq!(parse_version(Component::Ffmpeg, "garbage"), None);
        assert_eq!(parse_version(Component::Deno, ""), None);
    }

    #[test]
    fn a_yt_dlp_version_tells_its_age() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        let today = days_from_civil(2026, 9, 23);
        assert_eq!(ytdlp_age_days("2026.09.16.232951", today), Some(7));
        assert_eq!(ytdlp_age_days("2026.09.16", today), Some(7));
        assert_eq!(ytdlp_age_days("2025.12.31", today), Some(266));
        // A clock behind the build is not a negative age.
        assert_eq!(ytdlp_age_days("2026.09.30", today), Some(0));
        assert_eq!(ytdlp_age_days("nightly", today), None);
        assert_eq!(ytdlp_age_days("2026.13.01", today), None);
    }

    #[test]
    fn state_file_reads_and_writes_the_documented_key_value_format() {
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        fs::create_dir_all(&paths.bin_dir).unwrap();
        fs::write(
            paths.state_file(),
            "ffmpeg_archive_sha256=806d1dd\ndeno_zip_sha256=394f07f\n",
        )
        .unwrap();
        assert_eq!(
            read_state(&paths, "ffmpeg_archive_sha256").as_deref(),
            Some("806d1dd")
        );
        assert_eq!(
            read_state(&paths, "deno_zip_sha256").as_deref(),
            Some("394f07f")
        );
        assert_eq!(read_state(&paths, "deno_zip"), None);

        write_state(&paths, "deno_zip_sha256", "abc").unwrap();
        let text = fs::read_to_string(paths.state_file()).unwrap();
        assert_eq!(text, "ffmpeg_archive_sha256=806d1dd\ndeno_zip_sha256=abc\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_windows_extended_length_path_is_handed_over_as_a_plain_one() {
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\C:\Apps\MimirDLP")),
            PathBuf::from(r"C:\Apps\MimirDLP")
        );
        // Everything else is left exactly as it is.
        assert_eq!(
            plain_path(PathBuf::from("/home/user/apps")),
            PathBuf::from("/home/user/apps")
        );
        assert_eq!(
            plain_path(PathBuf::from(r"C:\Apps")),
            PathBuf::from(r"C:\Apps")
        );
    }

    /// Run from a network share, canonicalize() gives `\\?\UNC\server\...`.
    /// Dropping only `\\?\` left `UNC\server\...`, a relative path.
    #[test]
    fn a_network_share_keeps_its_leading_backslashes() {
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\UNC\server\share\MimirDLP")),
            PathBuf::from(r"\\server\share\MimirDLP")
        );
    }

    /// FFmpeg-Builds replaces its "latest" assets every day, and yt-dlp
    /// publishes nightlies. A download that starts before an update and
    /// ends after it is the new file, checked against the checksum file as
    /// it was *before*. The checksum file is fetched again once before
    /// calling that a mismatch.
    #[test]
    fn a_release_updated_during_the_download_is_not_a_hash_mismatch() {
        let (result, installed) = install_while_upstream_answers(true);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(installed.unwrap(), b"new build, published mid-download");
    }

    /// Asking again must not turn a real mismatch into a pass.
    #[test]
    fn a_file_that_matches_no_published_checksum_is_still_refused() {
        let (result, installed) = install_while_upstream_answers(false);
        assert!(matches!(result, Err(Error::Checksum(_))), "{result:?}");
        assert!(installed.is_none(), "nothing may be installed");
    }

    /// Serves a checksum file and an asset (always the new build). The
    /// checksum file lists the old build first; when `updates` it lists
    /// the new one from the second request on.
    fn install_while_upstream_answers(updates: bool) -> (Result<(), Error>, Option<Vec<u8>>) {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicU32, Ordering};

        let old = b"old build".to_vec();
        let new = b"new build, published mid-download".to_vec();
        let hash = |bytes: &[u8]| hex(&Sha256::digest(bytes));
        let (old_hash, new_hash) = (hash(&old), hash(&new));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let counter = std::sync::Arc::new(AtomicU32::new(0));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 2 {
                    line.clear();
                }
                let body = if request.contains("/SUMS") {
                    // The first answer predates the update, later ones don't.
                    let hash = if counter.fetch_add(1, Ordering::SeqCst) == 0 || !updates {
                        &old_hash
                    } else {
                        &new_hash
                    };
                    format!("{hash}  tool.bin\n").into_bytes()
                } else {
                    new.clone()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let source = platform::Source {
            url: leak(format!("http://{addr}/tool.bin")),
            asset: "tool.bin",
            checksum_url: leak(format!("http://{addr}/SUMS")),
            checksum: platform::ChecksumFormat::SumsFile,
            packaging: platform::Packaging::Raw { install_as: "tool" },
            state_key: None,
            signed_by: None,
        };
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        let result = install_source(Component::YtDlp, &source, &paths, &mut |_| {});
        let installed = fs::read(paths.tool("tool")).ok();
        fs::remove_dir_all(dir).unwrap();
        (result, installed)
    }

    /// Serves fixed bodies by path on a local port; anything else is a 404.
    fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> String {
        serve_with(move |path, _| {
            routes
                .iter()
                .find(|(p, _)| *p == path)
                .map(|(_, body)| body.clone())
        })
    }

    /// Like `serve`, but `answer` also gets how many times that path has
    /// been asked for before, so an answer can change between requests.
    fn serve_with(answer: impl Fn(&str, usize) -> Option<Vec<u8>> + Send + 'static) -> String {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut seen: Vec<String> = Vec::new();
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 2 {
                    line.clear();
                }
                let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                let asked = seen.iter().filter(|p| **p == path).count();
                seen.push(path.clone());
                match answer(&path, asked) {
                    Some(body) => {
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .unwrap();
                        stream.write_all(&body).unwrap();
                    }
                    None => {
                        let _ = stream.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                    }
                }
            }
        });
        format!("http://{addr}")
    }

    const TEST_KEY: &str = include_str!("testdata/test-key.asc");
    const TEST_SUMS: &[u8] = include_bytes!("testdata/test-SUMS");
    const TEST_SIG: &[u8] = include_bytes!("testdata/test-SUMS.sig");

    /// Installs `tool.bin` ("signed tool", whose hash is in `test-SUMS`)
    /// from a local server, with `test-SUMS` signed by a key made for these
    /// tests. `sig` is what the server answers for the signature, `None`
    /// for a 404.
    fn install_signed(sig: Option<&[u8]>) -> (Result<(), Error>, Option<Vec<u8>>, Vec<Event>) {
        let mut routes = vec![
            ("/SUMS", TEST_SUMS.to_vec()),
            ("/tool.bin", b"signed tool".to_vec()),
        ];
        if let Some(sig) = sig {
            routes.push(("/SUMS.sig", sig.to_vec()));
        }
        let base = serve(routes);
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let source = platform::Source {
            url: leak(format!("{base}/tool.bin")),
            asset: "tool.bin",
            checksum_url: leak(format!("{base}/SUMS")),
            checksum: platform::ChecksumFormat::SumsFile,
            packaging: platform::Packaging::Raw { install_as: "tool" },
            state_key: None,
            signed_by: Some(platform::SignedBy {
                url: leak(format!("{base}/SUMS.sig")),
                key: TEST_KEY,
                fingerprint: leak(
                    include_str!("testdata/test-key.fingerprint")
                        .trim()
                        .to_ascii_lowercase(),
                ),
            }),
        };
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        let mut log = Vec::new();
        let result = install_source(Component::YtDlp, &source, &paths, &mut |e| log.push(e));
        let installed = fs::read(paths.tool("tool")).ok();
        fs::remove_dir_all(dir).unwrap();
        (result, installed, log)
    }

    #[test]
    fn a_checksum_file_signed_by_the_right_key_is_trusted() {
        let (result, installed, log) = install_signed(Some(TEST_SIG));
        assert!(result.is_ok(), "{result:?}\n{log:?}");
        assert_eq!(installed.as_deref(), Some(&b"signed tool"[..]));
    }

    /// The checksum proves the file arrived whole; only the signature
    /// proves who published the checksum. A signature by anyone else (here
    /// yt-dlp's own, over a different file) must stop the install.
    #[test]
    fn a_checksum_file_with_a_wrong_signature_is_refused() {
        let wrong = include_bytes!("testdata/ytdlp-SHA2-256SUMS.sig");
        let (result, installed, log) = install_signed(Some(wrong));
        assert!(
            matches!(result, Err(Error::Checksum(_))),
            "{result:?}\n{log:?}"
        );
        assert_eq!(installed, None, "nothing may be installed");
    }

    /// The second look at the checksums (for a release that moved during
    /// the download) must be signed too, or it would be a way around the
    /// signature: here the first answer is properly signed but doesn't
    /// match the file, and the second matches it but isn't signed.
    #[test]
    fn asking_again_for_the_checksums_does_not_skip_the_signature() {
        let evil = b"evil tool".to_vec();
        let evil_sums = format!("{}  tool.bin\n", hex(&Sha256::digest(&evil)));
        let base = serve_with(move |path, asked| match path {
            "/SUMS" if asked == 0 => Some(TEST_SUMS.to_vec()),
            "/SUMS" => Some(evil_sums.clone().into_bytes()),
            "/SUMS.sig" => Some(TEST_SIG.to_vec()),
            "/tool.bin" => Some(evil.clone()),
            _ => None,
        });
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let source = platform::Source {
            url: leak(format!("{base}/tool.bin")),
            asset: "tool.bin",
            checksum_url: leak(format!("{base}/SUMS")),
            checksum: platform::ChecksumFormat::SumsFile,
            packaging: platform::Packaging::Raw { install_as: "tool" },
            state_key: None,
            signed_by: Some(platform::SignedBy {
                url: leak(format!("{base}/SUMS.sig")),
                key: TEST_KEY,
                fingerprint: leak(
                    include_str!("testdata/test-key.fingerprint")
                        .trim()
                        .to_ascii_lowercase(),
                ),
            }),
        };
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        let result = install_source(Component::YtDlp, &source, &paths, &mut |_| {});
        assert!(matches!(result, Err(Error::Checksum(_))), "{result:?}");
        assert!(!paths.tool("tool").exists(), "the unsigned build got in");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_checksum_file_without_its_signature_is_refused() {
        let (result, installed, _) = install_signed(None);
        assert!(result.is_err(), "{result:?}");
        assert_eq!(installed, None, "nothing may be installed");
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_app_folder_is_noticed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        assert!(app_dir_writable(&paths).is_ok());
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        // root writes anywhere, so only check where permissions mean something.
        let root = fs::write(dir.join("x"), b"").is_ok();
        if !root {
            assert!(app_dir_writable(&paths).is_err());
        }
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn leftovers_are_cleaned_but_nothing_else() {
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        fs::create_dir_all(&paths.bin_dir).unwrap();
        for f in [".download-x.zip", ".tmp-ffmpeg", "ffmpeg", ".install_state"] {
            fs::write(paths.bin_dir.join(f), "x").unwrap();
        }
        clean_leftovers(&paths);
        let mut left: Vec<String> = fs::read_dir(&paths.bin_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [".install_state", "ffmpeg"]);
        fs::remove_dir_all(dir).unwrap();
    }
}
