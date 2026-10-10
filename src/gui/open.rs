//! Opening a folder in the file manager or a link in the browser.
//!
//! On Wayland a program can't bring another one to the front: the
//! compositor only lets a window take the focus with an activation token
//! (the xdg-activation protocol). Without one, KDE opened a link in a
//! browser window that stayed behind and only flashed its taskbar entry. So
//! a token is asked for while the click is fresh, through the window's own
//! Wayland connection, and handed to `xdg-open`, which passes it on
//! (`XDG_ACTIVATION_TOKEN`). Without Wayland (X11, Windows) there is no
//! token and the target is opened as before.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use iced::{Task, window};

/// Opens a folder or a web address. The result is the error to show, if
/// any.
pub fn open(target: impl Into<OsString>) -> Task<Result<(), String>> {
    let target = target.into();
    window::latest()
        .then(|id| match id {
            Some(id) => window::run(id, activation_token),
            None => Task::done(None),
        })
        .map(move |token| spawn(&target, token).map_err(|e| e.to_string()))
}

/// Opens the folder containing `path`, with that file selected where the
/// platform can do it.
#[cfg(windows)]
pub fn reveal(path: &Path) -> Task<Result<(), String>> {
    let mut arg = OsString::from("/select,");
    arg.push(path);
    Task::done(spawn(&arg, None).map_err(|e| e.to_string()))
}

#[cfg(unix)]
pub fn reveal(path: &Path) -> Task<Result<(), String>> {
    open(path.parent().unwrap_or(path))
}

/// `explorer.exe` is known to return a non-zero exit status even on
/// success, so only the spawn itself is checked, never the exit code.
#[cfg(windows)]
fn spawn(target: &OsStr, _token: Option<String>) -> std::io::Result<()> {
    std::process::Command::new("explorer").arg(target).spawn()?;
    Ok(())
}

/// Waited for on a thread of its own: a child that is never waited for
/// stays behind as a zombie until the app exits, one per click.
#[cfg(unix)]
fn spawn(target: &OsStr, token: Option<String>) -> std::io::Result<()> {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(target);
    if let Some(token) = token {
        command.env("XDG_ACTIVATION_TOKEN", token);
    }
    let mut child = command.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(target_os = "linux")]
fn activation_token(window: &dyn window::Window) -> Option<String> {
    wayland::token(window)
}

#[cfg(not(target_os = "linux"))]
fn activation_token(_window: &dyn window::Window) -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
mod wayland {
    use iced::window::raw_window_handle::{RawDisplayHandle, RawWindowHandle};
    use wayland_client::backend::{Backend, ObjectId};
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
    use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
    use wayland_protocols::xdg::activation::v1::client::xdg_activation_token_v1::{
        self, XdgActivationTokenV1,
    };
    use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;

    /// Asks the compositor for a token on the window's surface, on a queue
    /// of its own over the window's connection. `None` on X11, or when the
    /// compositor doesn't offer xdg-activation or doesn't answer.
    pub fn token(window: &dyn iced::window::Window) -> Option<String> {
        let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
            return None;
        };
        let RawWindowHandle::Wayland(surface) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: both pointers come from the live window this callback
        // was handed, and everything made from them is dropped before it
        // returns. A foreign backend never closes the connection.
        let connection = Connection::from_backend(unsafe {
            Backend::from_foreign_display(display.display.as_ptr().cast())
        });
        let surface_id =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.surface.as_ptr().cast()) }
                .ok()?;
        let surface = WlSurface::from_id(&connection, surface_id).ok()?;

        let (globals, mut queue) = registry_queue_init::<State>(&connection).ok()?;
        let handle = queue.handle();
        let activation: XdgActivationV1 = globals.bind(&handle, 1..=1, ()).ok()?;
        let request = activation.get_activation_token(&handle, ());
        request.set_app_id(crate::desktop::APP_ID.to_string());
        request.set_surface(&surface);
        request.commit();
        let mut state = State(None);
        // The answer comes right away; a few round trips, not a wait.
        for _ in 0..3 {
            if state.0.is_some() || queue.roundtrip(&mut state).is_err() {
                break;
            }
        }
        request.destroy();
        activation.destroy();
        state.0
    }

    struct State(Option<String>);

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<XdgActivationV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &XdgActivationV1,
            _: <XdgActivationV1 as Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<XdgActivationTokenV1, ()> for State {
        fn event(
            state: &mut Self,
            _: &XdgActivationTokenV1,
            event: xdg_activation_token_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let xdg_activation_token_v1::Event::Done { token } = event {
                state.0 = Some(token);
            }
        }
    }
}
