// API doubles. check.py inserts actual product types and functions below.
extern crate self as serde_json;
#[macro_export]
macro_rules! json { ($($tokens:tt)*) => { () }; }
use std::cell::RefCell;
use std::sync::{mpsc, Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
static PANEL_HEIGHT: AtomicU32 = AtomicU32::new(600);
fn record(_: &str, _: ()) {}
fn panel_closed(_: &tauri::AppHandle) {}
fn panel_opened(_: &tauri::AppHandle) {}

#[derive(Default)]
struct Effects {
    content: bool,
    exited: bool,
    settings: bool,
    apply: bool,
    build_ok: bool,
    builds: usize,
    fail: &'static str,
}
thread_local! {
    static EFFECTS: RefCell<Effects> = RefCell::new(Effects::default());
    static JOBS: RefCell<Vec<Box<dyn FnOnce()>>> = RefCell::new(vec![]);
    static DIAGNOSTIC: RefCell<Option<gtk::Window>> = const { RefCell::new(None) };
    static DELETE: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
    static BUTTONS: RefCell<Vec<(&'static str, Box<dyn Fn()>)>> = RefCell::new(vec![]);
    static WAKE: RefCell<Option<mpsc::Receiver<()>>> = const { RefCell::new(None) };
}
fn failure(name: &str) -> Result<(), String> {
    if EFFECTS.with(|s| s.borrow().fail == name) { Err(name.into()) } else { Ok(()) }
}
fn apply(_: &tauri::Window) -> Result<(), String> {
    EFFECTS.with(|s| s.borrow_mut().apply = true);
    failure("apply")
}
fn build_tray(_: &tauri::AppHandle) -> Result<(), String> {
    EFFECTS.with(|s| s.borrow_mut().builds += 1);
    if EFFECTS.with(|s| s.borrow().build_ok) { Ok(()) } else { Err("build failed".into()) }
}
#[derive(Clone, Copy)]
enum MouseButton { Left, Right }
#[derive(Clone, Copy)]
enum MouseButtonState { Up, Down }
enum TrayIconEvent { Click { button: MouseButton, button_state: MouseButtonState }, Other }
struct ProbeTray;
impl ProbeTray { fn app_handle(&self) -> &tauri::AppHandle { static APP: tauri::AppHandle = tauri::AppHandle; &APP } }
// Native focus-grab lifecycle and compositor delivery are an API boundary.
mod focus_grab {
    pub fn presented(_: &super::tauri::Window, _: &super::tauri::AppHandle) {}
}
mod tauri {
    #[derive(Clone)]
    pub struct AppHandle;
    pub struct Window;
    impl AppHandle {
        pub fn get_window(&self, _: &str) -> Option<Window> { Some(Window) }
        pub fn run_on_main_thread(&self, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
            super::failure("dispatch")?;
            super::JOBS.with(|q| q.borrow_mut().push(Box::new(f)));
            Ok(())
        }
        pub fn exit(&self, _: i32) { super::EFFECTS.with(|s| s.borrow_mut().exited = true); }
        pub fn emit(&self, _: &str, _: ()) -> Result<(), String> {
            super::EFFECTS.with(|s| s.borrow_mut().settings = true); Ok(())
        }
    }
    impl Window {
        pub fn app_handle(&self) -> &AppHandle { static APP: AppHandle = AppHandle; &APP }
        pub fn hide(&self) -> Result<(), String> {
            super::failure("hide")?;
            super::EFFECTS.with(|s| s.borrow_mut().content = false); Ok(())
        }
        pub fn show(&self) -> Result<(), String> {
            super::failure("show")?;
            super::EFFECTS.with(|s| s.borrow_mut().content = true); Ok(())
        }
        pub fn set_focus(&self) -> Result<(), String> { super::failure("focus") }
        pub fn is_visible(&self) -> Result<bool, String> { Ok(super::EFFECTS.with(|s| s.borrow().content)) }
    }
    pub enum WindowEvent { Focused(bool), Other }
}
mod gtk {
    use std::{cell::Cell, rc::Rc};
    pub mod glib { pub enum Propagation { Stop } }
    pub enum WindowType { Toplevel }
    pub enum Orientation { Vertical, Horizontal }
    #[derive(Clone)]
    pub struct Window { pub visible: Rc<Cell<bool>> }
    impl Window {
        pub fn new(_: WindowType) -> Self {
            let window = Self { visible: Rc::new(Cell::new(false)) };
            super::DIAGNOSTIC.with(|d| *d.borrow_mut() = Some(window.clone())); window
        }
        pub fn set_title(&self, _: &str) {}
        pub fn set_default_size(&self, _: i32, _: i32) {}
        pub fn set_border_width(&self, _: u32) {}
        pub fn add<T>(&self, _: &T) {}
        pub fn show_all(&self) { self.visible.set(true); }
        pub fn present(&self) { self.visible.set(true); }
        pub fn hide(&self) { self.visible.set(false); }
        pub fn connect_delete_event(&self, f: impl Fn(&Window, ()) -> glib::Propagation + 'static) {
            let window = self.clone();
            super::DELETE.with(|d| *d.borrow_mut() = Some(std::boxed::Box::new(move || { f(&window, ()); })));
        }
    }
    pub struct Box;
    impl Box {
        pub fn new(_: Orientation, _: i32) -> Self { Self }
        pub fn pack_start<T>(&self, _: &T, _: bool, _: bool, _: u32) {}
        pub fn pack_end<T>(&self, _: &T, _: bool, _: bool, _: u32) {}
    }
    pub struct Label;
    impl Label {
        pub fn new(_: Option<&str>) -> Self { Self }
        pub fn set_line_wrap(&self, _: bool) {}
    }
    #[derive(Clone)]
    pub struct Button { label: &'static str }
    impl Button {
        pub fn with_label(label: &'static str) -> Self { Self { label } }
        pub fn connect_clicked(&self, f: impl Fn(&Button) + 'static) {
            let button = self.clone();
            super::BUTTONS.with(|b| b.borrow_mut().push((self.label, std::boxed::Box::new(move || f(&button)))));
        }
    }
}

// @SOURCE_TYPES@
// @SOURCE_FUNCTIONS@

fn drain_one() {
    let job = JOBS.with(|q| if q.borrow().is_empty() { None } else { Some(q.borrow_mut().remove(0)) });
    if let Some(job) = job { job(); }
}
fn state_number(state: TrayState) -> usize {
    match state { TrayState::Waiting => 0, TrayState::Available => 1, TrayState::Unavailable => 2 }
}
fn state_from(n: usize) -> TrayState {
    match n { 0 => TrayState::Waiting, 1 => TrayState::Available, _ => TrayState::Unavailable }
}
fn reset(state: usize, content: bool, diagnostic: bool, created: bool, fail: &'static str) {
    EFFECTS.with(|s| *s.borrow_mut() = Effects { content, fail, build_ok: true, ..Effects::default() });
    JOBS.with(|q| q.borrow_mut().clear());
    DIAGNOSTIC.with(|d| *d.borrow_mut() = None);
    DELETE.with(|d| *d.borrow_mut() = None);
    BUTTONS.with(|b| b.borrow_mut().clear());
    let (retry, wake) = mpsc::sync_channel(1);
    WAKE.with(|w| *w.borrow_mut() = Some(wake));
    UI.with(|ui| *ui.borrow_mut() = LinuxUi { state: state_from(state), tray_created: created, diagnostic: None, retry: Some(retry) });
    if diagnostic { present_diagnostic(&tauri::AppHandle); }
}
fn bits() -> (usize, bool, bool, bool, bool, bool, bool) {
    let (state, created) = UI.with(|ui| (state_number(ui.borrow().state), ui.borrow().tray_created));
    let diagnostic = DIAGNOSTIC.with(|d| d.borrow().as_ref().is_some_and(|w| w.visible.get()));
    EFFECTS.with(|s| { let s = s.borrow(); (state, s.content, diagnostic, created, s.exited, s.settings, s.apply) })
}
fn perform(action: &str) {
    let app = &tauri::AppHandle;
    match action {
        "observe00" => observe_on_main(app, Observation { registered: false, grace_elapsed: false }),
        "observe01" => observe_on_main(app, Observation { registered: false, grace_elapsed: true }),
        "observe10" => observe_on_main(app, Observation { registered: true, grace_elapsed: false }),
        "observe11" => observe_on_main(app, Observation { registered: true, grace_elapsed: true }),
        "observe01_fail" | "observe11_fail" => {
            EFFECTS.with(|s| s.borrow_mut().build_ok = false);
            observe_on_main(app, Observation { registered: action == "observe11_fail", grace_elapsed: true });
        }
        "activate" => {
            tray_event(&ProbeTray, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up });
            drain_one();
        }
        "show" => { let _ = show(app); drain_one(); }
        "settings" => { let _ = dispatch(app, |app| present(app, true)); drain_one(); }
        "hide" => { let _ = hide(&tauri::Window); drain_one(); }
        "resize" => { let _ = resize(&tauri::Window, 500); drain_one(); }
        "retry_ok" => retry_on_main(app),
        "retry_fail" => { EFFECTS.with(|s| s.borrow_mut().build_ok = false); retry_on_main(app); }
        "close" => {
            // A delete event queued while visible retains its callback after recovery hides it.
            if DELETE.with(|d| d.borrow().is_none()) {
                present_diagnostic(app);
                DIAGNOSTIC.with(|d| d.borrow().as_ref().unwrap().hide());
            }
            DELETE.with(|d| d.borrow().as_ref().unwrap()());
        }
        "quit" => {
            if BUTTONS.with(|b| b.borrow().is_empty()) {
                present_diagnostic(app);
                DIAGNOSTIC.with(|d| d.borrow().as_ref().unwrap().hide());
            }
            BUTTONS.with(|b| b.borrow().iter().find(|(label, _)| *label == "Quit").unwrap().1());
        }
        _ => panic!("unknown action"),
    }
}
fn focus_checks() {
    // Exercise every nonempty focus sequence through length six for visible
    // content, hidden content, and the unavailable native diagnostic.
    for length in 1..=6 {
        for sequence in 0..(1 << length) {
            for (state, content, diagnostic) in [(1, true, false), (1, false, false), (2, false, true)] {
                reset(state, content, diagnostic, true, "");
                let before = bits();
                for step in 0..length {
                    let focused = sequence & (1 << step) != 0;
                    on_window_event(&tauri::Window, &tauri::WindowEvent::Focused(focused));
                    while JOBS.with(|q| !q.borrow().is_empty()) { drain_one(); }
                    assert_eq!(bits(), before, "FocusPreservesVisiblePanel: focus event changed window state");
                    assert!(JOBS.with(|q| q.borrow().is_empty()));
                }
                hide(&tauri::Window).unwrap();
                drain_one();
                if state == 1 {
                    assert!(!bits().1 && !bits().2, "ExplicitHideStillWorks");
                } else {
                    assert_eq!(bits(), before, "FocusCannotDismissUnavailableDiagnostic");
                }
            }
        }
    }
    // A tray-opened editable panel survives loss of focus and queues no Hide,
    // including a loss delivered before an already queued activation is applied.
    reset(1, false, false, true, "");
    tray_event(&ProbeTray, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up });
    on_window_event(&tauri::Window, &tauri::WindowEvent::Focused(false));
    assert_eq!(JOBS.with(|q| q.borrow().len()), 1, "FocusMustNotQueueHide");
    drain_one();
    let opened = bits();
    assert!(opened.1);
    on_window_event(&tauri::Window, &tauri::WindowEvent::Focused(false));
    assert!(JOBS.with(|q| q.borrow().is_empty()), "FocusMustNotQueueHide");
    assert_eq!(bits(), opened, "FocusPreservesVisiblePanel");
    hide(&tauri::Window).unwrap();
    drain_one();
    assert!(!bits().1, "ExplicitHideStillWorks");
    println!("FOCUS_CHECKS_PASS: 126 focus sequences across three window states; tray activation; explicit Hide; unavailable diagnostic");
}
fn self_checks() {
    focus_checks();
    // The actual click callback queues the same state-gated Show intent.
    reset(1, true, false, true, "");
    tray_event(&ProbeTray, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up });
    observe_on_main(&tauri::AppHandle, Observation { registered: false, grace_elapsed: true });
    drain_one();
    assert_eq!(bits(), (2, false, true, true, false, false, false));
    for event in [TrayIconEvent::Other,
                  TrayIconEvent::Click { button: MouseButton::Right, button_state: MouseButtonState::Up },
                  TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Down }] {
        reset(1, false, false, true, "");
        tray_event(&ProbeTray, event);
        assert!(JOBS.with(|q| q.borrow().is_empty()));
        assert!(!bits().1);
    }
    // Failed construction remains diagnostic-only. Automatic attempts neither
    // publish observations nor wake the single worker. Later construction and
    // a subsequent own-registration sample recover without pressing Retry.
    reset(0, false, false, false, "");
    EFFECTS.with(|s| s.borrow_mut().build_ok = false);
    assert!(build_tray(&tauri::AppHandle).is_err());
    observe_on_main(&tauri::AppHandle, Observation { registered: false, grace_elapsed: true });
    assert_eq!(bits(), (2, false, true, false, false, false, false));
    assert_eq!(EFFECTS.with(|s| s.borrow().builds), 2);
    EFFECTS.with(|s| s.borrow_mut().build_ok = true);
    observe_on_main(&tauri::AppHandle, Observation { registered: false, grace_elapsed: true });
    assert_eq!(bits(), (2, false, true, true, false, false, false));
    assert_eq!(EFFECTS.with(|s| s.borrow().builds), 3);
    assert!(JOBS.with(|q| q.borrow().is_empty()));
    WAKE.with(|w| assert!(w.borrow().as_ref().unwrap().try_recv().is_err()));
    observe_on_main(&tauri::AppHandle, Observation { registered: true, grace_elapsed: true });
    assert_eq!(bits(), (1, true, false, true, false, false, true));
    assert_eq!(EFFECTS.with(|s| s.borrow().builds), 3);
    println!("AUTOMATIC_REBUILD_CHECK_PASS: initial failure; failed automatic attempt; later construction; own registration; no wake");
    // A queued Show reads the state at execution, after loss was applied.
    reset(1, true, false, true, "");
    show(&tauri::AppHandle).unwrap();
    observe_on_main(&tauri::AppHandle, Observation { registered: false, grace_elapsed: true });
    drain_one();
    assert_eq!(bits(), (2, false, true, true, false, false, false));
    // The real mailbox coalesces a positive then a negative observation.
    reset(0, false, false, true, "");
    let mailbox = Arc::new(Mutex::new(Mailbox::default()));
    publish(&tauri::AppHandle, &mailbox, Observation { registered: true, grace_elapsed: true }).unwrap();
    publish(&tauri::AppHandle, &mailbox, Observation { registered: false, grace_elapsed: true }).unwrap();
    assert_eq!(JOBS.with(|q| q.borrow().len()), 1);
    drain_one();
    assert_eq!(bits().0, 2);
    assert!(!bits().1 && bits().2);
    // Hiding failure during loss terminates instead of exposing working content.
    reset(1, true, false, true, "hide");
    observe_on_main(&tauri::AppHandle, Observation { registered: false, grace_elapsed: true });
    assert!(bits().4);
    // Failed styling/show never opens working content. Focus failure may follow a successful show.
    for fail in ["apply", "show"] {
        reset(2, false, true, true, fail);
        observe_on_main(&tauri::AppHandle, Observation { registered: true, grace_elapsed: true });
        assert!(!bits().1 && !bits().4);
    }
    reset(2, false, true, true, "focus");
    observe_on_main(&tauri::AppHandle, Observation { registered: true, grace_elapsed: true });
    assert!(bits().1 && !bits().4);
    // Scheduling failure is reported. A failed mailbox schedule can be retried.
    reset(2, false, true, true, "dispatch");
    assert!(show(&tauri::AppHandle).is_err());
    let mailbox = Arc::new(Mutex::new(Mailbox::default()));
    assert!(publish(&tauri::AppHandle, &mailbox, Observation { registered: true, grace_elapsed: true }).is_err());
    assert!(!mailbox.lock().unwrap().scheduled);
    // Retry during a pending probe coalesces at capacity one. It does not dismiss the diagnostic.
    reset(2, false, true, false, "");
    BUTTONS.with(|b| b.borrow().iter().find(|(label, _)| *label == "Retry").unwrap().1());
    BUTTONS.with(|b| b.borrow().iter().find(|(label, _)| *label == "Retry").unwrap().1());
    assert!(bits().3 && bits().2 && !bits().1);
    WAKE.with(|w| {
        let w = w.borrow(); let w = w.as_ref().unwrap();
        assert!(w.try_recv().is_ok()); assert!(w.try_recv().is_err());
    });
    // Programmatic recovery hide never triggers the real native delete handler.
    observe_on_main(&tauri::AppHandle, Observation { registered: true, grace_elapsed: true });
    assert!(bits().1 && !bits().2 && !bits().4);
    // A previously queued delete event still exits after recovery.
    DELETE.with(|d| d.borrow().as_ref().unwrap()());
    assert!(bits().4);
    println!("SOURCE_CHECKS_PASS");
}
fn main() {
    if std::env::args().any(|a| a == "--focus-checks") { focus_checks(); return; }
    if std::env::args().any(|a| a == "--checks") { self_checks(); return; }
    let actions = ["observe00", "observe01", "observe10", "observe11", "observe01_fail", "observe11_fail", "activate", "show", "settings", "hide", "resize", "retry_ok", "retry_fail", "close", "quit"];
    for state in 0..3 { for content in [false, true] { for diagnostic in [false, true] { for created in [false, true] {
        for action in actions {
            reset(state, content, diagnostic, created, "");
            perform(action);
            let (s, c, d, t, e, settings, apply) = bits();
            println!("{} {} {} {} {} {} {} {} {} {} {} {}", state, u8::from(content), u8::from(diagnostic), u8::from(created), action, s, u8::from(c), u8::from(d), u8::from(t), u8::from(e), u8::from(settings), u8::from(apply));
        }
    }}}}
}
