//! Helpers shared by the unit tests of several modules.

use std::fs;
use std::path::{Path, PathBuf};

use crate::provision::Paths;

/// A fresh directory under the system temp folder. The caller removes it.
/// Unique per process and per call, so tests can run in parallel.
pub(crate) fn tempdir() -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ytp-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Serialises the tests that write a program and then run it.
///
/// Linux refuses to exec a file that any process still holds open for
/// writing (`ETXTBSY`), and a sibling test forking at the wrong moment
/// inherits that handle. It showed up once as "yt-dlp could not be
/// started" in a test that passes on its own, so the tests that do this
/// take turns. Windows has no such restriction; taking turns there too
/// keeps the tests the same on both.
pub(crate) fn exec_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Puts the stand-in (`tests/support/stand_in.rs`) into `paths`' `bin/`
/// as the tool `name`, doing what `script` says. A real program rather
/// than a shell script, so the same tests run on Windows, where a tool has
/// to be an `.exe` and is stopped another way.
pub(crate) fn stand_in(paths: &Paths, name: &str, script: &str) {
    stand_in_at(&paths.tool(name), script);
}

/// [`stand_in`] at `program`, which ends in `.exe` on Windows; its script
/// goes next to it.
pub(crate) fn stand_in_at(program: &Path, script: &str) {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let built = BUILT.get_or_init(build_stand_in);
    fs::create_dir_all(program.parent().unwrap()).unwrap();
    fs::copy(built, program).unwrap();
    fs::write(program.with_extension("script"), script).unwrap();
}

/// Compiled with the crate's own compiler into the build's `OUT_DIR`, and
/// again only when the source is newer.
fn build_stand_in() -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/stand_in.rs");
    let exe_name = |stem: &str| format!("{stem}{}", std::env::consts::EXE_SUFFIX);
    let exe = Path::new(env!("OUT_DIR")).join(exe_name("stand-in"));
    let modified = |path: &Path| fs::metadata(path).and_then(|m| m.modified()).ok();
    if modified(&exe).is_some() && modified(&exe) >= modified(&source) {
        return exe;
    }
    // Built beside it and renamed, so a test run alongside never copies a
    // half-written file.
    let fresh = exe.with_file_name(exe_name(&format!("stand-in-{}", std::process::id())));
    let status = std::process::Command::new(env!("MIMIRDLP_RUSTC"))
        .args(["--edition", "2024", "-C", "debuginfo=0", "-o"])
        .arg(&fresh)
        .arg(&source)
        .status()
        .expect("rustc could not be started");
    assert!(status.success(), "the stand-in did not build");
    fs::rename(&fresh, &exe).unwrap();
    exe
}
