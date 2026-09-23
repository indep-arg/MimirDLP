//! Starting, watching and stopping the tools in `bin/`.
//!
//! Every external tool goes through [`command`] and is stopped with
//! [`kill_tree`], because of two facts about the real binaries:
//!
//! - yt-dlp's release binaries are PyInstaller bootloaders that run the
//!   actual program as a child process, and yt-dlp itself starts ffmpeg.
//!   Killing only the direct child leaves the rest running: checked with
//!   `ps` on `yt-dlp_linux`, whose child was reparented and kept going
//!   after its parent got SIGKILL.
//! - On Windows this is a GUI program (`windows_subsystem = "windows"`),
//!   and a console program started from one opens a console window of its
//!   own unless told not to.

use std::ffi::OsStr;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// `CREATE_NO_WINDOW`, from the Windows process creation flags.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A `Command` for one of the tools: no console window on Windows, and on
/// Unix a process group of its own, so [`kill_tree`] can reach everything
/// it starts.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Kills `child` and everything it started. Only valid for a child made by
/// [`command`] and not yet waited for, so its id can't have been reused.
pub fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: kill(2) only takes integers. A negative pid addresses the
        // process group, which `command` made this child the leader of.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        // /T takes the whole tree, which is what a Job Object would do, but
        // without holding one open for every tool that is started.
        let _ = command("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

/// What a finished tool printed. Not UTF-8 is replaced, not an error: one
/// odd byte in a path must not cost the whole answer.
#[derive(Debug)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
pub enum RunError {
    Spawn(io::Error),
    Wait(io::Error),
    TimedOut,
}

/// Runs `command` to completion with both pipes drained concurrently, and
/// kills it (with everything it started) after `limit`.
///
/// The concurrent draining is not optional: a child whose output is bigger
/// than the OS pipe buffer blocks on write() until someone reads, and a
/// loop that only polls `try_wait()` never does. That deadlock was found on
/// real `yt-dlp -j` output. This is what `Child::wait_with_output()` does
/// internally, redone by hand so the timeout can still kill the child.
pub fn run_captured(mut command: Command, limit: Duration) -> Result<Output, RunError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunError::Spawn)?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        pipe.map(|mut pipe| {
            thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = pipe.read_to_end(&mut bytes);
                String::from_utf8_lossy(&bytes).into_owned()
            })
        })
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as _));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > limit => {
                kill_tree(&mut child);
                let _ = child.wait();
                return Err(RunError::TimedOut);
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                kill_tree(&mut child);
                let _ = child.wait();
                return Err(RunError::Wait(e));
            }
        }
    };
    let join =
        |t: Option<thread::JoinHandle<String>>| t.and_then(|t| t.join().ok()).unwrap_or_default();
    Ok(Output {
        status,
        stdout: join(stdout),
        stderr: join(stderr),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("tool");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn alive(pid: &str) -> bool {
        fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| !stat.contains(") Z "))
    }

    #[test]
    fn a_timeout_kills_the_grandchildren_too() {
        let _guard = crate::testing::exec_guard();
        let dir = crate::testing::tempdir();
        let pid_file = dir.join("grandchild.pid");
        let tool = script(
            &dir,
            &format!(
                "sh -c 'echo $$ > {}; exec sleep 60' &\nwait",
                pid_file.display()
            ),
        );
        let started = Instant::now();
        let result = run_captured(command(&tool), Duration::from_millis(500));
        assert!(matches!(result, Err(RunError::TimedOut)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
        let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
        let mut still_alive = alive(&pid);
        for _ in 0..20 {
            if !still_alive {
                break;
            }
            thread::sleep(Duration::from_millis(50));
            still_alive = alive(&pid);
        }
        assert!(!still_alive, "grandchild {pid} survived the timeout");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn output_that_is_not_utf8_is_kept_rather_than_dropped() {
        let _guard = crate::testing::exec_guard();
        let dir = crate::testing::tempdir();
        let tool = script(&dir, "printf 'Jos\\351\\n'; printf 'err\\n' >&2");
        let out = run_captured(command(&tool), Duration::from_secs(10)).unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, "Jos\u{fffd}\n");
        assert_eq!(out.stderr, "err\n");
        fs::remove_dir_all(dir).unwrap();
    }
}
