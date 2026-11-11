use gtk::glib::object::ObjectType;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;
use tauri::Manager;

extern "C" {
    fn t3_focus_grab_start(
        window: *mut c_void,
        cleared: extern "C" fn(*mut c_void),
        data: *mut c_void,
    ) -> *mut c_void;
    fn t3_focus_grab_stop(grab: *mut c_void);
}

struct Callback {
    app: tauri::AppHandle,
    generation: u64,
}

struct Grab {
    native: NonNull<c_void>,
    callback: Box<Callback>,
}

impl Drop for Grab {
    fn drop(&mut self) {
        unsafe { t3_focus_grab_stop(self.native.as_ptr()) };
    }
}

thread_local! {
    static ACTIVE: RefCell<Option<Grab>> = const { RefCell::new(None) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
    static MAPPING: Cell<u64> = const { Cell::new(0) };
}

extern "C" fn cleared(data: *mut c_void) {
    let callback = unsafe { &*(data as *const Callback) };
    let app = callback.app.clone();
    let generation = callback.generation;
    gtk::glib::idle_add_local_once(move || {
        let current = ACTIVE.with(|active| {
            active
                .borrow()
                .as_ref()
                .map(|grab| grab.callback.generation)
                == Some(generation)
        });
        if current {
            stop();
            if let Err(error) = super::hide_on_main(&app) {
                eprintln!("Linux focus grab hide: {error}");
            }
        }
    });
}

fn stop() {
    ACTIVE.with(|active| drop(active.borrow_mut().take()));
}

fn start(window: &gtk::ApplicationWindow, app: &tauri::AppHandle, replace: bool) {
    if !window.is_mapped() || super::UI.with(|ui| ui.borrow().state) != super::TrayState::Available
    {
        return;
    }
    if !replace && ACTIVE.with(|active| active.borrow().is_some()) {
        return;
    }
    stop();
    let Some(gdk_window) = window.window() else {
        return;
    };
    let generation = GENERATION.with(|counter| {
        let next = counter.get().wrapping_add(1);
        counter.set(next);
        next
    });
    let mut callback = Box::new(Callback {
        app: app.clone(),
        generation,
    });
    let native = unsafe {
        t3_focus_grab_start(
            gdk_window.as_ptr().cast(),
            cleared,
            (&mut *callback as *mut Callback).cast(),
        )
    };
    if let Some(native) = NonNull::new(native) {
        ACTIVE.with(|active| *active.borrow_mut() = Some(Grab { native, callback }));
    }
}

pub(super) fn initialize(app: &tauri::AppHandle) {
    let Some(window) = app
        .get_window("main")
        .and_then(|window| window.gtk_window().ok())
    else {
        return;
    };
    let handle = app.clone();
    window.connect_draw(move |window, _| {
        let window = window.downgrade();
        let handle = handle.clone();
        let mapping = MAPPING.with(Cell::get);
        gtk::glib::idle_add_local_once(move || {
            if MAPPING.with(Cell::get) == mapping {
                if let Some(window) = window.upgrade() {
                    start(&window, &handle, false);
                }
            }
        });
        gtk::glib::Propagation::Proceed
    });
    window.connect_unmap(|_| {
        MAPPING.with(|mapping| mapping.set(mapping.get().wrapping_add(1)));
        stop();
    });
    window.connect_destroy(|_| stop());
}

pub(super) fn presented(window: &tauri::Window, app: &tauri::AppHandle) {
    if let Ok(window) = window.gtk_window() {
        if ACTIVE.with(|active| active.borrow().is_some()) {
            start(&window, app, true);
        } else {
            window.queue_draw();
        }
    }
}
