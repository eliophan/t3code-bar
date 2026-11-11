use gtk::glib::object::ObjectType;
use gtk::prelude::*;
use std::cell::RefCell;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

#[path = "focus_grab.rs"]
mod focus_grab;

extern "C" {
    fn t3_layer_bar_init(window: *mut std::ffi::c_void) -> std::ffi::c_int;
}

pub fn initialize_surface(app: &tauri::App) -> Result<(), std::io::Error> {
    let window = app
        .get_window("main")
        .ok_or_else(|| std::io::Error::other("Bar window missing"))?;
    let gtk = window.gtk_window().map_err(std::io::Error::other)?;
    let result = unsafe { t3_layer_bar_init(gtk.as_ptr().cast()) };
    let error = match result {
        0 => return Ok(()),
        1 => "GTK panel was realized before layer-shell initialization",
        2 => "Wayland compositor does not support layer-shell keyboard on-demand (version 4)",
        _ => "Could not initialize the GTK layer surface",
    };
    Err(std::io::Error::other(error))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrayState {
    Waiting,
    Available,
    Unavailable,
}

struct LinuxUi {
    state: TrayState,
    tray_created: bool,
    diagnostic: Option<gtk::Window>,
    retry: Option<mpsc::SyncSender<()>>,
}

thread_local! {
    static UI: RefCell<LinuxUi> = const { RefCell::new(LinuxUi {
        state: TrayState::Waiting,
        tray_created: false,
        diagnostic: None,
        retry: None,
    }) };
}

#[derive(Clone, Copy)]
struct Observation {
    registered: bool,
    grace_elapsed: bool,
}

#[derive(Default)]
struct Mailbox {
    latest: Option<Observation>,
    scheduled: bool,
}

fn next_state(state: TrayState, registered: bool, grace_elapsed: bool) -> TrayState {
    if registered {
        TrayState::Available
    } else if state == TrayState::Waiting && !grace_elapsed {
        TrayState::Waiting
    } else {
        TrayState::Unavailable
    }
}

// Must run before GTK/WebKit is initialized. Respect even an explicit empty override.
pub fn prepare_runtime() {
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

async fn host_registered() -> zbus::Result<bool> {
    let connection = zbus::Connection::session().await?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
    )
    .await?;
    if !proxy
        .get_property::<bool>("IsStatusNotifierHostRegistered")
        .await?
    {
        return Ok(false);
    }
    let items = proxy
        .get_property::<Vec<String>>("RegisteredStatusNotifierItems")
        .await?;
    let bus = zbus::Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await?;
    for item in items {
        let service = item.split('/').next().unwrap_or("");
        if bus
            .call::<_, _, u32>("GetConnectionUnixProcessID", &(service,))
            .await
            .is_ok_and(|pid| pid == std::process::id())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn apply(window: &tauri::Window) -> Result<(), String> {
    let height = crate::PANEL_HEIGHT.load(std::sync::atomic::Ordering::Acquire) as i32;
    let gtk = window.gtk_window().map_err(|e| e.to_string())?;
    gtk.set_size_request(320, height);
    gtk.set_default_size(320, height);
    gtk.set_decorated(false);
    gtk.set_title("T3 Code Bar — Panel");
    let gtk_observed = gtk.clone();
    gtk::glib::timeout_add_local_once(Duration::from_millis(1000), move || {
        crate::record(
            "linux_gtk_state",
            serde_json::json!({"decorated":gtk_observed.is_decorated(),"mapped":gtk_observed.is_mapped(),"realized":gtk_observed.is_realized(),"titlebar":gtk_observed.titlebar().is_some()}),
        );
    });
    window.set_skip_taskbar(true).map_err(|e| e.to_string())?;
    window.set_always_on_top(true).map_err(|e| e.to_string())?;
    window.set_resizable(true).map_err(|e| e.to_string())?;
    window
        .set_min_size(Some(tauri::LogicalSize::new(320.0, height as f64)))
        .map_err(|e| e.to_string())?;
    window
        .set_max_size(Some(tauri::LogicalSize::new(320.0, height as f64)))
        .map_err(|e| e.to_string())?;
    window
        .set_size(tauri::LogicalSize::new(320.0, height as f64))
        .map_err(|e| e.to_string())?;
    let size = window.inner_size().ok();
    crate::record(
        "linux_variant",
        serde_json::json!({"requestedPanel":true,"effectivePanel":true,"host":true,"tray":true,"requestedLogicalSize":[320,height],"actualPhysicalSize":size.map(|s| [s.width,s.height]),"scaleFactor":window.scale_factor().ok()}),
    );
    Ok(())
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Show T3 Code Bar", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide T3 Code Bar", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit T3 Code Bar", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &hide, &settings, &quit])?;
    let mut rgba = vec![0u8; 16 * 16 * 4];
    for pixel in rgba.chunks_mut(4) {
        pixel.copy_from_slice(&[96, 165, 250, 255]);
    }
    TrayIconBuilder::with_id("bar-tray")
        .icon(tauri::image::Image::new_owned(rgba, 16, 16))
        .tooltip("T3 Code Bar")
        .menu(&menu)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                if let Err(error) = show(tray.app_handle()) {
                    eprintln!("Linux tray action: {error}");
                }
            }
        })
        .on_menu_event(|app, event| {
            let result = match event.id.as_ref() {
                "show" => show(app),
                "settings" => dispatch(app, |app| present(app, true)),
                "hide" => dispatch(app, hide_on_main),
                "quit" => {
                    app.exit(0);
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(error) = result {
                eprintln!("Linux tray action: {error}");
            }
        })
        .build(app)?;
    Ok(())
}

fn dispatch(
    app: &tauri::AppHandle,
    action: impl FnOnce(&tauri::AppHandle) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Err(error) = action(&handle) {
            eprintln!("Linux window action: {error}");
            crate::record("linux_error", serde_json::json!({"error":error}));
        }
    })
    .map_err(|e| e.to_string())
}

fn present_diagnostic(app: &tauri::AppHandle) {
    let existing = UI.with(|ui| ui.borrow().diagnostic.clone());
    if let Some(window) = existing {
        window.present();
        return;
    }
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("T3 Code Bar - Tray unavailable");
    window.set_default_size(420, 160);
    window.set_border_width(20);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let message = gtk::Label::new(Some(
        "T3 Code Bar needs a system tray. Start a tray host, then retry, or quit.",
    ));
    message.set_line_wrap(true);
    content.pack_start(&message, true, true, 0);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let retry = gtk::Button::with_label("Retry");
    let quit = gtk::Button::with_label("Quit");
    buttons.pack_end(&quit, false, false, 0);
    buttons.pack_end(&retry, false, false, 0);
    content.pack_start(&buttons, false, false, 0);
    window.add(&content);
    let handle = app.clone();
    retry.connect_clicked(move |_| retry_on_main(&handle));
    let handle = app.clone();
    quit.connect_clicked(move |_| handle.exit(0));
    let handle = app.clone();
    window.connect_delete_event(move |_, _| {
        handle.exit(0);
        gtk::glib::Propagation::Stop
    });
    UI.with(|ui| ui.borrow_mut().diagnostic = Some(window.clone()));
    window.show_all();
    window.present();
}

fn rebuild_missing_tray(app: &tauri::AppHandle) {
    let needs_tray = UI.with(|ui| !ui.borrow().tray_created);
    if needs_tray {
        match build_tray(app) {
            Ok(()) => UI.with(|ui| ui.borrow_mut().tray_created = true),
            Err(error) => eprintln!("Linux tray retry: {error}"),
        }
    }
}

fn retry_on_main(app: &tauri::AppHandle) {
    rebuild_missing_tray(app);
    let retry = UI.with(|ui| ui.borrow().retry.clone());
    if let Some(retry) = retry {
        let _ = retry.try_send(());
    }
}

fn present(app: &tauri::AppHandle, settings: bool) -> Result<(), String> {
    let state = UI.with(|ui| ui.borrow().state);
    match state {
        TrayState::Waiting => Ok(()),
        TrayState::Unavailable => {
            present_diagnostic(app);
            Ok(())
        }
        TrayState::Available => {
            let window = app.get_window("main").ok_or("Bar window missing")?;
            apply(&window)?;
            window.show().map_err(|e| e.to_string())?;
            focus_grab::presented(&window, app);
            crate::panel_opened(app);
            if settings {
                app.emit("bar-settings", ()).map_err(|e| e.to_string())?;
            }
            window.set_focus().map_err(|e| e.to_string())
        }
    }
}

fn hide_on_main(app: &tauri::AppHandle) -> Result<(), String> {
    if UI.with(|ui| ui.borrow().state) == TrayState::Available {
        let window = app.get_window("main").ok_or("Bar window missing")?;
        window.hide().map_err(|e| e.to_string())?;
        crate::panel_closed(app);
    }
    Ok(())
}

fn observe_on_main(app: &tauri::AppHandle, observation: Observation) {
    if observation.grace_elapsed {
        rebuild_missing_tray(app);
    }
    let (previous, next) = UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let previous = ui.state;
        let next = next_state(
            previous,
            ui.tray_created && observation.registered,
            observation.grace_elapsed || !ui.tray_created,
        );
        ui.state = next;
        (previous, next)
    });
    if previous == next {
        return;
    }
    crate::record(
        "linux_host_changed",
        serde_json::json!({"state":format!("{next:?}")}),
    );
    if next == TrayState::Unavailable {
        let result = app
            .get_window("main")
            .ok_or_else(|| "Bar window missing".to_owned())
            .and_then(|window| window.hide().map_err(|e| e.to_string()));
        if let Err(error) = result {
            eprintln!("Linux window hide: {error}");
            app.exit(1);
            return;
        }
        crate::panel_closed(app);
        present_diagnostic(app);
    } else if next == TrayState::Available {
        let diagnostic = UI.with(|ui| ui.borrow().diagnostic.clone());
        if let Some(window) = diagnostic {
            window.hide();
        }
        if let Err(error) = present(app, false) {
            eprintln!("Linux window show: {error}");
        }
    }
}

fn publish(
    app: &tauri::AppHandle,
    mailbox: &Arc<Mutex<Mailbox>>,
    observation: Observation,
) -> Result<(), String> {
    let mut pending = mailbox.lock().map_err(|e| e.to_string())?;
    pending.latest = Some(observation);
    if pending.scheduled {
        return Ok(());
    }
    pending.scheduled = true;
    let inbox = Arc::clone(mailbox);
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        let observation = {
            let mut pending = inbox.lock().unwrap();
            pending.scheduled = false;
            pending.latest.take()
        };
        if let Some(observation) = observation {
            observe_on_main(&handle, observation);
        }
    }) {
        pending.scheduled = false;
        return Err(error.to_string());
    }
    Ok(())
}

pub fn initialize(app: &mut tauri::App) -> tauri::Result<()> {
    focus_grab::initialize(app.handle());
    let (retry, wake) = mpsc::sync_channel(1);
    let result = build_tray(app.handle());
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.tray_created = result.is_ok();
        ui.retry = Some(retry);
    });
    if let Err(error) = result {
        eprintln!("Linux tray creation: {error}");
        observe_on_main(
            app.handle(),
            Observation {
                registered: false,
                grace_elapsed: true,
            },
        );
    }
    let app = app.handle().clone();
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("Linux tray poller: {error}");
                app.exit(1);
                return;
            }
        };
        let start = Instant::now();
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        loop {
            let registered = runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(2), host_registered())
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or(false)
            });
            let grace_elapsed = start.elapsed() >= Duration::from_secs(2);
            if let Err(error) = publish(
                &app,
                &mailbox,
                Observation {
                    registered,
                    grace_elapsed,
                },
            ) {
                eprintln!("Linux tray observation: {error}");
                app.exit(1);
                break;
            }
            match wake.recv_timeout(Duration::from_millis(if grace_elapsed { 500 } else { 100 })) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    Ok(())
}

pub fn hide(window: &tauri::Window) -> Result<(), String> {
    dispatch(window.app_handle(), hide_on_main)
}

pub fn toggle(app: &tauri::AppHandle) {
    let _ = dispatch(app, |app| {
        let state = UI.with(|ui| ui.borrow().state);
        if state == TrayState::Available
            && app
                .get_window("main")
                .and_then(|window| window.is_visible().ok())
                .unwrap_or(false)
        {
            hide_on_main(app)
        } else {
            present(app, false)
        }
    });
}

pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    // Hyprland can report a transient Focused(false) while the tray activation
    // hands pointer focus to the panel. Hiding here races the first pointer
    // move into an input and makes the panel disappear before the user can type.
    // Linux exposes explicit tray Hide and window-close actions instead.
    let _ = (window, event);
}

pub fn show(app: &tauri::AppHandle) -> Result<(), String> {
    dispatch(app, |app| present(app, false))
}

pub fn resize(window: &tauri::Window, _height: u32) -> Result<(), String> {
    dispatch(window.app_handle(), |app| {
        if UI.with(|ui| ui.borrow().state) == TrayState::Available {
            let window = app.get_window("main").ok_or("Bar window missing")?;
            apply(&window)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::{next_state, TrayState::*};

    #[test]
    fn startup_loss_and_recovery_follow_tray_observations() {
        assert_eq!(next_state(Waiting, false, false), Waiting);
        assert_eq!(next_state(Waiting, false, true), Unavailable);
        assert_eq!(next_state(Waiting, true, false), Available);
        assert_eq!(next_state(Available, false, false), Unavailable);
        assert_eq!(next_state(Unavailable, false, true), Unavailable);
        assert_eq!(next_state(Unavailable, true, true), Available);
        assert_eq!(next_state(Available, true, true), Available);
    }
}
