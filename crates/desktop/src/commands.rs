//! What the page can ask for. Each command is one action of the interface;
//! the state it changes comes back in the next snapshot.
//!
//! The page names things (an item by its id, a link by what it is for) and
//! never passes a path or an address to open: those are looked up here, so
//! a page that misbehaved could still only open what the app itself offers.

use std::sync::Arc;

use osprey_core::app::view::{Fixed, LineView, Snapshot, lines_since};
use osprey_core::app::{self, Message, queue, setup};
use osprey_core::config::{Kind, Quality, ThemePref};
use osprey_core::provision::Channel;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::{Shared, dispatch, window};

type Held<'a> = State<'a, Arc<Shared>>;

#[tauri::command(async)]
pub fn snapshot(shared: Held<'_>) -> Snapshot {
    shared.app().snapshot()
}

/// What only changes with the language: asked for at start and again
/// after choosing another one.
#[tauri::command(async)]
pub fn fixed(shared: Held<'_>) -> Fixed {
    shared.app().fixed()
}

/// The activity of the `queue` or of the `setup` screen, from line `seq`.
#[tauri::command(async)]
pub fn log_since(shared: Held<'_>, which: String, seq: u64) -> Vec<LineView> {
    let app = shared.app();
    match which.as_str() {
        "setup" => lines_since(app.setup.activity(), seq),
        _ => lines_since(app.queue.activity(), seq),
    }
}

/// The page has drawn itself: the window can be shown.
#[tauri::command(async)]
pub fn window_ready(app: AppHandle) {
    window::reveal(&app);
}

/// Debug builds only: how to open, so a screen can be looked at (or
/// captured) without clicking through to it.
/// `OSPREY_DEV_PAGE=setup|download|settings|about` opens on that screen,
/// `OSPREY_DEV_MENU=1` with the header bar's menu open, and
/// `OSPREY_DEV_ADVANCED=1` with the advanced settings unfolded.
#[derive(Serialize, Default)]
pub struct DevHints {
    page: Option<String>,
    menu: bool,
    advanced: bool,
}

#[tauri::command(async)]
pub fn dev_hints() -> DevHints {
    if !cfg!(debug_assertions) {
        return DevHints::default();
    }
    DevHints {
        page: std::env::var("OSPREY_DEV_PAGE").ok(),
        menu: std::env::var_os("OSPREY_DEV_MENU").is_some(),
        advanced: std::env::var_os("OSPREY_DEV_ADVANCED").is_some(),
    }
}

/// What the link field should hold after an Add, and why if not empty.
#[derive(Serialize)]
pub struct LinkField {
    input: String,
    problem: Option<String>,
}

/// Queues what is in the link field (one link or several).
#[tauri::command(async)]
pub fn add_links(shared: Held<'_>, input: String) -> LinkField {
    dispatch(&shared, Message::Queue(queue::Message::UrlInput(input)));
    dispatch(&shared, Message::Queue(queue::Message::AddUrl));
    let app = shared.app();
    LinkField {
        input: app.queue.url_input().to_string(),
        problem: app.queue.problem().map(str::to_string),
    }
}

/// The window got the focus with `input` in the link field: a link copied
/// meanwhile is offered when the field is empty. Returns what to put in it.
#[tauri::command(async)]
pub fn offer_clipboard(app: AppHandle, shared: Held<'_>, input: String) -> Option<String> {
    // Debug builds: screenshots shouldn't show whatever the developer copied.
    if cfg!(debug_assertions) && std::env::var_os("OSPREY_DEV_NO_CLIPBOARD").is_some() {
        return None;
    }
    if !input.trim().is_empty() {
        return None;
    }
    let copied = app.clipboard().read_text().ok();
    dispatch(&shared, Message::Queue(queue::Message::UrlInput(input)));
    dispatch(&shared, Message::Queue(queue::Message::Clipboard(copied)));
    let offered = shared.app().queue.url_input().to_string();
    (!offered.is_empty()).then_some(offered)
}

/// An action on the queue, or on its item `id`.
#[tauri::command(async)]
pub fn queue_do(
    app: AppHandle,
    shared: Held<'_>,
    action: String,
    id: Option<u64>,
    value: Option<String>,
) -> Result<(), String> {
    let item = || id.ok_or_else(|| format!("{action} needs an item"));
    let choice = || value.as_deref().unwrap_or_default();
    let message = match action.as_str() {
        "start" => queue::Message::Start,
        "stop" => queue::Message::Stop,
        "clear" => queue::Message::ClearFinished,
        "remove" => queue::Message::RemoveItem(item()?),
        "retry" => queue::Message::RetryItem(item()?),
        "kind" => queue::Message::ItemKind(
            item()?,
            Kind::from_key(choice()).ok_or("not a kind of download")?,
        ),
        "quality" => queue::Message::ItemQuality(
            item()?,
            Quality::from_key(choice()).ok_or("not a quality")?,
        ),
        "copy" => {
            let id = item()?;
            let url = shared.app().queue.url_of(id).map(str::to_string);
            let Some(url) = url else { return Ok(()) };
            app.clipboard().write_text(url).map_err(|e| e.to_string())?;
            queue::Message::CopyLink(id)
        }
        "open" | "reveal" => {
            let id = item()?;
            let path = {
                let mut core = shared.app();
                let path = core.queue.items().iter().find_map(|i| match &i.status {
                    queue::ItemStatus::Completed { path } if i.id == id => path.clone(),
                    _ => None,
                });
                path.filter(|path| action == "reveal" || core.queue.file_exists(path))
            };
            if let Some(path) = path {
                let opened = if action == "open" {
                    app.opener().open_path(path, None::<&str>)
                } else {
                    app.opener().reveal_item_in_dir(path)
                };
                if let Err(e) = opened {
                    shared.app().queue.open_failed(&e.to_string());
                }
            }
            shared.touch();
            return Ok(());
        }
        "folder" => {
            let folder = {
                let mut core = shared.app();
                let settings = core.settings.settings().clone();
                core.queue.output_folder(&settings)
            };
            if let Some(folder) = folder
                && let Err(e) = app
                    .opener()
                    .open_path(folder.to_string_lossy(), None::<&str>)
            {
                shared.app().queue.open_failed(&e.to_string());
            }
            shared.touch();
            return Ok(());
        }
        other => return Err(format!("unknown queue action: {other}")),
    };
    dispatch(&shared, Message::Queue(message));
    Ok(())
}

/// An action of the Setup screen.
#[tauri::command(async)]
pub fn setup_do(
    app: AppHandle,
    shared: Held<'_>,
    action: String,
    value: Option<String>,
) -> Result<(), String> {
    let message = match action.as_str() {
        "install" => setup::Message::InstallMissing,
        "check" => setup::Message::CheckUpdates,
        "update" => setup::Message::UpdateOutdated,
        "ask_reinstall" => setup::Message::AskReinstall,
        "confirm_reinstall" => setup::Message::ConfirmReinstall,
        "cancel_reinstall" => setup::Message::CancelReinstall,
        "details" => setup::Message::ToggleDetails,
        "channel" => setup::Message::Channel(
            Channel::from_key(value.as_deref().unwrap_or_default())
                .ok_or("not a yt-dlp channel")?,
        ),
        "diagnostics" => {
            let report = shared.app().setup.diagnostics();
            app.clipboard()
                .write_text(report)
                .map_err(|e| e.to_string())?;
            setup::Message::CopyDiagnostics
        }
        // From the "yt-dlp is old" notice on the Download screen.
        "check_from_notice" => {
            dispatch(&shared, Message::CheckForUpdates);
            return Ok(());
        }
        other => return Err(format!("unknown setup action: {other}")),
    };
    dispatch(&shared, Message::Setup(message));
    Ok(())
}

/// Changes one setting, by its key in the settings file.
#[tauri::command(async)]
pub fn set_setting(app: AppHandle, shared: Held<'_>, key: String, value: String) {
    shared.app().set_setting(&key, &value);
    if key == "theme"
        && let Some(theme) = ThemePref::from_key(&value)
    {
        window::apply_theme(&app, theme);
    }
    shared.touch();
}

/// Linux: adds or removes Osprey's entry in the applications menu.
#[tauri::command(async)]
pub fn set_desktop_entry(shared: Held<'_>, wanted: bool) {
    shared.app().set_desktop_entry(wanted);
    shared.touch();
}

/// Opens the desktop's folder dialog and makes what is picked the output
/// folder. It starts in the current one when that exists.
#[tauri::command(async)]
pub fn choose_folder(app: AppHandle, shared: Held<'_>) {
    let start = {
        let core = shared.app();
        core.settings.settings().output_dir(&shared.paths)
    };
    let mut dialog = app.dialog().file().set_title(app::folder_dialog_title());
    if start.is_dir() {
        dialog = dialog.set_directory(start);
    }
    let picked = dialog
        .blocking_pick_folder()
        .and_then(|folder| folder.into_path().ok());
    if let Some(folder) = picked {
        shared
            .app()
            .set_setting("output_dir", &folder.to_string_lossy());
        shared.touch();
    }
}

/// The download history's entries, one per video.
#[tauri::command(async)]
pub fn history_lines(shared: Held<'_>) -> Option<Vec<String>> {
    shared.app().settings.history_lines()
}

#[tauri::command(async)]
pub fn clear_history(shared: Held<'_>) {
    shared.app().clear_history();
    shared.touch();
}

/// The new-version notice was closed for this session.
#[tauri::command(async)]
pub fn dismiss_update(shared: Held<'_>) {
    dispatch(&shared, Message::DismissAppUpdate);
}

/// Opens one of the project's own pages in the browser.
#[tauri::command(async)]
pub fn open_link(app: AppHandle, shared: Held<'_>, which: String) -> Result<(), String> {
    let url = match which.as_str() {
        "website" => app::WEBSITE,
        "report" => app::REPORT,
        "releases" => osprey_core::app_update::RELEASES,
        other => return Err(format!("unknown link: {other}")),
    };
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        shared.app().link_failed(url, &e.to_string());
        shared.touch();
    }
    Ok(())
}
