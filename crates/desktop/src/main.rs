//! Osprey's window: a webview showing `ui/`, and the bridge between it and
//! `osprey-core`.
//!
//! The core holds all the state. The page sends actions as commands
//! (`commands.rs`), and after every change a snapshot of the state is sent
//! back as the `state` event, a few times a second at most. Files, links,
//! the clipboard and dialogs are only touched from here, on behalf of an
//! action: the page itself has no access to them.

// Windows opens a console behind a program unless it says otherwise. Debug
// builds keep it, so panics still have somewhere to print.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod window;

use std::borrow::Cow;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use osprey_core::app::{self, App, Message};
use osprey_core::config::{Settings, WindowState};
use osprey_core::provision::Paths;
use tauri::http;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// How long to wait after sending a snapshot before sending the next: a
/// download reports its progress far more often than anyone can read it.
const BETWEEN_SNAPSHOTS: Duration = Duration::from_millis(60);

pub struct Shared {
    app: Mutex<App>,
    pub paths: Paths,
    /// Something changed since the last snapshot was sent.
    dirty: Mutex<bool>,
    changed: Condvar,
}

impl Shared {
    pub fn app(&self) -> MutexGuard<'_, App> {
        // A panic while the state was held must not take every later
        // action down with it.
        self.app.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The state changed: the page gets a new snapshot shortly.
    pub fn touch(&self) {
        *self.dirty.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.changed.notify_one();
    }
}

/// Applies `message` and runs whatever work that leaves, whose own
/// messages come back through here.
pub fn dispatch(shared: &Arc<Shared>, message: Message) {
    let task = shared.app().update(message);
    shared.touch();
    run(shared, task);
}

fn run(shared: &Arc<Shared>, task: app::Task<Message>) {
    let shared = shared.clone();
    task.run(Arc::new(move |message| dispatch(&shared, message)));
}

/// Sends the page a snapshot whenever the state changed, on a thread of its
/// own so nothing that changes the state waits for the page.
fn send_snapshots(shared: Arc<Shared>, handle: AppHandle) {
    std::thread::spawn(move || {
        loop {
            {
                let mut dirty = shared.dirty.lock().unwrap_or_else(|e| e.into_inner());
                while !*dirty {
                    dirty = shared
                        .changed
                        .wait(dirty)
                        .unwrap_or_else(|e| e.into_inner());
                }
                *dirty = false;
            }
            let snapshot = shared.app().snapshot();
            let _ = handle.emit("state", &snapshot);
            std::thread::sleep(BETWEEN_SNAPSHOTS);
        }
    });
}

/// `thumb://localhost/<id>`: the thumbnail of the queue item `id`, which
/// the core fetched. The page never talks to the sites itself.
fn thumbnail(
    context: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: http::Request<Vec<u8>>,
) -> http::Response<Cow<'static, [u8]>> {
    let answer = |status: http::StatusCode, kind: &str, body: Vec<u8>| {
        http::Response::builder()
            .status(status)
            .header(http::header::CONTENT_TYPE, kind)
            .header(http::header::CACHE_CONTROL, "no-store")
            .body(Cow::Owned(body))
            .expect("a valid response")
    };
    let bytes = request
        .uri()
        .path()
        .trim_matches('/')
        .parse::<u64>()
        .ok()
        .and_then(|id| {
            let shared = context.app_handle().try_state::<Arc<Shared>>()?;
            let app = shared.app();
            app.queue.thumbnail(id)
        });
    match bytes {
        Some(bytes) => answer(http::StatusCode::OK, image_type(&bytes), bytes.to_vec()),
        None => answer(http::StatusCode::NOT_FOUND, "text/plain", Vec::new()),
    }
}

/// What kind of image these bytes are, from how they start. Sites serve
/// JPEG, WebP and now and then PNG, and not always under the right name.
fn image_type(bytes: &[u8]) -> &'static str {
    match bytes {
        [0xff, 0xd8, ..] => "image/jpeg",
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', ..] => "image/gif",
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => "image/webp",
        [_, _, _, _, b'f', b't', b'y', b'p', b'a', b'v', b'i', ..] => "image/avif",
        _ => "application/octet-stream",
    }
}

fn start(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let paths = match Paths::for_current_exe() {
        Ok(paths) => paths,
        Err(e) => {
            // Nowhere to keep anything: say so and leave.
            app.dialog()
                .message(app::folder_not_found(&e.to_string()))
                .title("Osprey")
                .kind(MessageDialogKind::Error)
                .show(|_| std::process::exit(1));
            return Ok(());
        }
    };
    app::adopt_earlier_files(&paths);
    let settings = Settings::load(&paths);
    let theme = settings.theme;
    let saved = WindowState::load(&paths);
    let (core, task) = App::boot(paths.clone(), settings);
    let shared = Arc::new(Shared {
        app: Mutex::new(core),
        paths,
        dirty: Mutex::new(false),
        changed: Condvar::new(),
    });
    app.manage(shared.clone());
    send_snapshots(shared.clone(), app.handle().clone());
    run(&shared, task);
    window::open(app.handle(), &shared.paths, saved, theme)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _dir| {
            // Started a second time: bring the window that is there forward.
            if let Some(window) = app.get_webview_window(window::MAIN) {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .register_uri_scheme_protocol("thumb", thumbnail)
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::fixed,
            commands::log_since,
            commands::window_ready,
            commands::dev_hints,
            commands::add_links,
            commands::offer_clipboard,
            commands::queue_do,
            commands::setup_do,
            commands::set_setting,
            commands::set_desktop_entry,
            commands::choose_folder,
            commands::history_lines,
            commands::clear_history,
            commands::dismiss_update,
            commands::open_link,
        ])
        .setup(start)
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event
                && let Some(shared) = window.try_state::<Arc<Shared>>()
            {
                // The window opens next time as it is closed now, and a
                // running download is stopped rather than left behind.
                window::remember(window, &shared.paths);
                shared.app().shutdown();
            }
        })
        .run(tauri::generate_context!())
        .expect("the window could not be opened");
}

#[cfg(test)]
mod tests {
    use super::image_type;

    #[test]
    fn a_thumbnail_is_told_apart_by_its_first_bytes() {
        assert_eq!(image_type(&[0xff, 0xd8, 0xff, 0xe0]), "image/jpeg");
        assert_eq!(image_type(b"\x89PNG\r\n\x1a\n"), "image/png");
        assert_eq!(image_type(b"RIFF\x10\0\0\0WEBPVP8 "), "image/webp");
        assert_eq!(image_type(b"\0\0\0\x1cftypavif"), "image/avif");
        assert_eq!(image_type(b"<html>"), "application/octet-stream");
    }
}
