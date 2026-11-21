//! The window itself: how it opens and what is remembered of it.

use std::time::Duration;

use osprey_core::config::{ThemePref, WindowState};
use osprey_core::provision::Paths;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const MAIN: &str = "main";

/// Below this the Settings rows and the queue cards overlap.
const MIN_SIZE: (f64, f64) = (720.0, 520.0);

/// The latest the window is shown, even if the page never said it was
/// drawn (see [`open`]).
const REVEAL_DEADLINE: Duration = Duration::from_secs(3);

/// Opens the main window as it was when it was last closed (`saved`).
///
/// It starts hidden: a webview is white until its page has drawn, which on
/// a dark desktop is a flash. The page says when it is ready
/// (`commands::window_ready`), and the window is shown then, or after
/// [`REVEAL_DEADLINE`] in any case.
pub fn open(
    app: &AppHandle,
    paths: &Paths,
    saved: Option<WindowState>,
    theme: ThemePref,
) -> tauri::Result<()> {
    let (width, height) = saved.map_or(WindowState::DEFAULT_SIZE, |w| (w.width, w.height));
    let mut builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::default())
        .title("Osprey")
        .inner_size(
            f64::from(width).max(MIN_SIZE.0),
            f64::from(height).max(MIN_SIZE.1),
        )
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .theme(native_theme(theme))
        .visible(false)
        // Tauri would otherwise keep the webview's cache and storage in the
        // user's local data folder, and a portable copy leaves nothing behind.
        .data_directory(paths.app_dir.join("webview"));
    if let Some((x, y)) = saved.and_then(|w| w.position) {
        builder = builder.position(f64::from(x), f64::from(y));
    }
    if saved.is_some_and(|w| w.maximized) {
        builder = builder.maximized(true);
    }
    builder.build()?;

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(REVEAL_DEADLINE);
        reveal(&app);
    });
    Ok(())
}

/// Shows the window and gives it the focus. Harmless when it already shows.
pub fn reveal(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN)
        && !window.is_visible().unwrap_or(true)
    {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// The title bar follows the theme chosen in the app, or the system's.
pub fn apply_theme(app: &AppHandle, theme: ThemePref) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.set_theme(native_theme(theme));
    }
}

fn native_theme(theme: ThemePref) -> Option<tauri::Theme> {
    match theme {
        ThemePref::System => None,
        ThemePref::Light => Some(tauri::Theme::Light),
        ThemePref::Dark => Some(tauri::Theme::Dark),
    }
}

/// Saves the window's size and place for the next launch. If that fails
/// the window still closes.
pub fn remember(window: &tauri::Window, paths: &Paths) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let size = window
        .inner_size()
        .map(|size| size.to_logical::<f32>(scale))
        .map(|size| (size.width, size.height))
        .unwrap_or((0.0, 0.0));
    // Only Windows reads and sets the place the same way: X11 reports where
    // the inside of the window is but puts the frame there, so the window
    // would creep down by its title bar on every launch, and Wayland tells
    // nothing.
    let position = window
        .outer_position()
        .ok()
        .filter(|_| cfg!(windows))
        .map(|position| position.to_logical::<f32>(scale))
        .map(|position| (position.x, position.y));
    let state = WindowState::when_closed(
        WindowState::load(paths),
        size,
        position,
        window.is_maximized().unwrap_or(false),
        window.is_minimized().unwrap_or(false),
    );
    if let Some(state) = state {
        let _ = state.save(paths);
    }
}
