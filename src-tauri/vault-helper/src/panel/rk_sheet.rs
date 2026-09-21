//! Recovery Key display + print window (spec §1.7): the 24 words and the
//! §11.7 freshness checkpoint, rendered by the helper and printed through
//! `NSPrintOperation` directly from an in-memory view — never via the
//! WebView, never through a PDF or any file the helper writes.
//!
//! - The window is excluded from screen capture by macOS itself
//!   (`NSWindowSharingNone`), in addition to Source's own suppression
//!   (`secure_panel_visible` is up for the whole window lifetime,
//!   including the print dialog).
//! - The only exit is "I've saved it" (or a lock/timeout abort through
//!   the watchdog). There is no cancel: callers commit nothing until the
//!   user acknowledged.
//! - v1 keeps the standard macOS print dialog (owner decision, Phase
//!   D.1): no custom print panel is built merely to remove "Save as
//!   PDF". The window carries the normative warning copy instead.
//! - Honest limits: AppKit copies the words into NSString/label storage
//!   the helper cannot zeroize (labels are cleared on close); the macOS
//!   print system may spool the job (CUPS) outside this process — the
//!   helper makes no erasure claim about spooled print data.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{declare_class, msg_send_id, sel, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSPrintOperation,
    NSTextField, NSView, NSWindow, NSWindowSharingType, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

use super::watchdog::{Hooks, Watchdog};
use crate::vault::{RecoverySheet, RK_SHEET_TITLE};

const W: f64 = 560.0;
const H: f64 = 470.0;

#[derive(Default)]
struct SheetIvars {
    acknowledged: Cell<bool>,
    print_view: RefCell<Option<Retained<NSView>>>,
    labels: RefCell<Vec<Retained<NSTextField>>>,
}

declare_class!(
    struct RkSheetDelegate;

    unsafe impl ClassType for RkSheetDelegate {
        type Super = NSObject;
        type Mutability = objc2::mutability::InteriorMutable;
        const NAME: &'static str = "RkSheetDelegate";
    }

    impl DeclaredClass for RkSheetDelegate {
        type Ivars = SheetIvars;
    }

    unsafe impl RkSheetDelegate {
        #[method(onPrint:)]
        fn on_print(&self, _sender: Option<&NSButton>) {
            if let Some(view) = self.ivars().print_view.borrow().as_ref() {
                print_view(view, true);
            }
        }

        #[method(onDone:)]
        fn on_done(&self, _sender: Option<&NSButton>) {
            self.ivars().acknowledged.set(true);
            let mtm = MainThreadMarker::new().expect("main thread");
            // SAFETY: main thread, inside the modal session being ended.
            unsafe { NSApplication::sharedApplication(mtm).stopModal() };
        }
    }
);

thread_local! {
    static CURRENT: RefCell<Option<(*const NSWindow, *const RkSheetDelegate)>> = const { RefCell::new(None) };
}

fn as_any(o: &NSObject) -> &AnyObject {
    unsafe { &*(o as *const NSObject).cast::<AnyObject>() }
}

/// Clear every label (drops our references to the word strings).
fn clear(delegate: &RkSheetDelegate) {
    for l in delegate.ivars().labels.borrow().iter() {
        unsafe { l.setStringValue(&NSString::new()) };
    }
}

fn abort_sheet() {
    CURRENT.with(|c| {
        if let Some((win, del)) = *c.borrow() {
            // SAFETY: installed by `present` on this thread; still live.
            let (win, del) = unsafe { (&*win, &*del) };
            clear(del);
            win.orderOut(None);
            super::appkit::set_on_screen(false);
            let mtm = MainThreadMarker::new().expect("main thread");
            unsafe { NSApplication::sharedApplication(mtm).abortModal() };
        }
    });
}

/// Build the sheet content (used for the window and the printed page).
fn content_view(sheet: &RecoverySheet, delegate: &RkSheetDelegate, mtm: MainThreadMarker) -> Retained<NSView> {
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(W, H));
    // SAFETY: main thread; views freshly created and parented here.
    unsafe {
        let view = NSView::initWithFrame(mtm.alloc::<NSView>(), rect);
        let add = |text: &str, x: f64, y: f64, w: f64| {
            let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
            l.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 18.0)));
            view.addSubview(&l);
            delegate.ivars().labels.borrow_mut().push(l);
        };
        add("Source Vault — Recovery Key", 24.0, H - 40.0, 500.0);
        // Why this window is here, before the words themselves (§1.7).
        add(sheet.reason.line(), 24.0, H - 62.0, 520.0);
        add("Write these 24 words down or print this page, and keep it offline.", 24.0, H - 82.0, 520.0);
        for (i, word) in sheet.words.split_whitespace().enumerate() {
            let (col, row) = ((i / 6) as f64, (i % 6) as f64);
            add(&format!("{:>2}. {word}", i + 1), 24.0 + col * 130.0, H - 122.0 - row * 26.0, 124.0);
        }
        add(&sheet.checkpoint, 24.0, 118.0, 520.0);
        add("Anyone with these words can open your vault.", 24.0, 92.0, 520.0);
        // Normative copy (spec §1.7, Phase D.1 owner decision).
        add("Print to paper. Saving as PDF creates an unencrypted copy of your", 24.0, 74.0, 520.0);
        add("Recovery Key. The print system may also keep a spooled copy.", 24.0, 56.0, 520.0);
        view
    }
}

/// Run a print operation for `view`. `interactive=false` is the debug
/// evidence path (no dialog, job cancelled after rendering).
fn print_view(view: &NSView, interactive: bool) -> bool {
    // SAFETY: main thread; `view` is live for the whole call.
    unsafe {
        let op: Retained<NSPrintOperation> = NSPrintOperation::printOperationWithView(view);
        op.setShowsPrintPanel(interactive);
        op.setShowsProgressPanel(interactive);
        if !interactive {
            op.printInfo().setJobDisposition(objc2_app_kit::NSPrintCancelJob);
        }
        op.runOperation()
    }
}

/// Present the window modally; `true` iff the user pressed "I've saved it".
pub fn present(sheet: &RecoverySheet, abort: Arc<AtomicBool>) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    if abort.load(Ordering::SeqCst) {
        return false;
    }
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate: Retained<RkSheetDelegate> = {
        let this = mtm.alloc::<RkSheetDelegate>().set_ivars(SheetIvars::default());
        unsafe { msg_send_id![super(this), init] }
    };
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(W, H + 50.0));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc::<NSWindow>(),
            rect,
            NSWindowStyleMask::Titled,
            NSBackingStoreType::NSBackingStoreBuffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str(RK_SHEET_TITLE));
    window.setSharingType(NSWindowSharingType::NSWindowSharingNone);
    let words = content_view(sheet, &delegate, mtm);
    let print = content_view(sheet, &delegate, mtm);
    *delegate.ivars().print_view.borrow_mut() = Some(print.clone());
    // SAFETY: main thread; layout of freshly created views.
    unsafe {
        let root = NSView::initWithFrame(mtm.alloc::<NSView>(), rect);
        words.setFrame(NSRect::new(NSPoint::new(0.0, 50.0), NSSize::new(W, H)));
        root.addSubview(&words);
        let print_btn = NSButton::buttonWithTitle_target_action(&NSString::from_str("Print…"), Some(as_any(&delegate)), Some(sel!(onPrint:)), mtm);
        print_btn.setFrame(NSRect::new(NSPoint::new(W - 290.0, 12.0), NSSize::new(110.0, 28.0)));
        let done = NSButton::buttonWithTitle_target_action(&NSString::from_str("I've saved it"), Some(as_any(&delegate)), Some(sel!(onDone:)), mtm);
        done.setFrame(NSRect::new(NSPoint::new(W - 170.0, 12.0), NSSize::new(146.0, 28.0)));
        done.setKeyEquivalent(&NSString::from_str("\r"));
        root.addSubview(&print_btn);
        root.addSubview(&done);
        window.setContentView(Some(&root));
    }
    CURRENT.with(|c| *c.borrow_mut() = Some((Retained::as_ptr(&window), Retained::as_ptr(&delegate))));
    window.center();
    unsafe { app.activate() };
    window.makeKeyAndOrderFront(None);
    super::appkit::set_on_screen(true);

    #[cfg(debug_assertions)]
    if let Ok(mode) = std::env::var("OV0_VAULT_SHEET_SCRIPT") {
        let attempt = mode == "autoshow-print";
        let ran = attempt && print_view(&print, false);
        super::form::write_sheet_probe(attempt, ran, super::appkit::panel_on_screen(), window.isVisible());
    }

    let watchdog = Watchdog::start(abort, Hooks { abort: abort_sheet, probe: || {} });
    unsafe { app.runModalForWindow(&window) };
    drop(watchdog);
    clear(&delegate);
    window.orderOut(None);
    super::appkit::set_on_screen(false);
    CURRENT.with(|c| *c.borrow_mut() = None);
    *delegate.ivars().print_view.borrow_mut() = None;
    delegate.ivars().acknowledged.get()
}
