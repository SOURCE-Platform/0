//! The §1.7 native secure panel: an `NSPanel` with `NSSecureTextField`s,
//! presented modally from the AppKit main queue. Runs only on the main
//! thread (the runner marshals requests here; see `panel/mod.rs`).
//!
//! Close semantics (§1.7): field buffers are cleared to "" before the
//! window closes, collected strings live in `Zeroizing<String>`, and the
//! panel has no close button — the only exits are OK/Cancel/Escape or a
//! server-initiated abort (`lock` preemption / §13.3 timeout).
//!
//! MP policy: minimum 8 chars, confirmation must match (create/change).
//! The spec fixes no MP strength floor; this is a documented Phase C
//! default.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{declare_class, msg_send_id, sel, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSResponder,
    NSSecureTextField, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use zeroize::Zeroizing;

/// All ObjC class references share the object-pointer layout; casting up
/// the class hierarchy is the standard objc2 idiom when the multi-hop
/// `Deref` chain doesn't apply at an argument position.
fn as_responder(view: &NSSecureTextField) -> &NSResponder {
    unsafe { &*(view as *const NSSecureTextField).cast::<NSResponder>() }
}

fn as_any(object: &NSObject) -> &AnyObject {
    unsafe { &*(object as *const NSObject).cast::<AnyObject>() }
}

use super::form::{field_plan, read_field, to_secret, validate};
use super::watchdog::Watchdog;
#[cfg(debug_assertions)]
use super::form::write_probe;
use crate::vault::{PanelOutcome, PanelRequest};


struct PanelIvars {
    panel: RefCell<Option<Retained<NSWindow>>>,
    fields: RefCell<Vec<Retained<NSSecureTextField>>>,
    error_label: RefCell<Option<Retained<NSTextField>>>,
    mode: Cell<u8>, // 0 = entry, 1 = create, 2 = change
    submitted: RefCell<Option<Vec<Zeroizing<String>>>>,
}

impl Default for PanelIvars {
    fn default() -> Self {
        PanelIvars {
            panel: RefCell::new(None),
            fields: RefCell::new(Vec::new()),
            error_label: RefCell::new(None),
            mode: Cell::new(0),
            submitted: RefCell::new(None),
        }
    }
}

declare_class!(
    struct VaultPanelDelegate;

    unsafe impl ClassType for VaultPanelDelegate {
        type Super = NSObject;
        type Mutability = objc2::mutability::InteriorMutable;
        const NAME: &'static str = "VaultPanelDelegate";
    }

    impl DeclaredClass for VaultPanelDelegate {
        type Ivars = PanelIvars;
    }

    unsafe impl VaultPanelDelegate {
        #[method(onOk:)]
        fn on_ok(&self, _sender: Option<&NSButton>) {
            self.finish(true);
        }

        #[method(onCancel:)]
        fn on_cancel(&self, _sender: Option<&NSButton>) {
            self.finish(false);
        }
    }
);

impl VaultPanelDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(PanelIvars::default());
        unsafe { msg_send_id![super(this), init] }
    }

    /// Read field values, enforce the MP policy, zeroize the field
    /// buffers, and stop the modal session. `ok=false` = cancel path.
    fn finish(&self, ok: bool) {
        if ok {
            let values: Vec<Zeroizing<String>> = self
                .ivars()
                .fields
                .borrow()
                .iter()
                .map(|f| read_field(f))
                .collect();
            match validate(self.ivars().mode.get(), &values) {
                Ok(()) => *self.ivars().submitted.borrow_mut() = Some(values),
                Err(msg) => {
                    if let Some(label) = self.ivars().error_label.borrow().as_ref() {
                        // SAFETY: called on the main thread with a live label.
                        unsafe { label.setStringValue(&NSString::from_str(msg)) };
                    }
                    // Do not zeroize on a failed submit — the user is
                    // still editing. No modal stop: panel stays up.
                    return;
                }
            }
        }
        self.zeroize_fields();
        let mtm = MainThreadMarker::new().expect("panel code runs on the main thread");
        unsafe { NSApplication::sharedApplication(mtm).stopModal() };
    }

    fn zeroize_fields(&self) {
        for field in self.ivars().fields.borrow().iter() {
            unsafe { field.setStringValue(&NSString::new()) };
        }
    }
}

/// Pointers to the live panel session so `abort_current` (called by the
/// modal-session watchdog) can dismiss it. Main-thread only.
struct SessionPtrs {
    panel: *const NSWindow,
    delegate: *const VaultPanelDelegate,
}

thread_local! {
    static CURRENT: RefCell<Option<SessionPtrs>> = const { RefCell::new(None) };
}

/// Dismiss the visible panel, zeroizing its fields (§1.7 close). Called
/// from the watchdog timer inside the modal session, where `abortModal`
/// (not `stopModal`) is the call that ends the loop. Main-thread only.
pub(super) fn abort_current() {
    CURRENT.with(|c| {
        if let Some(ptrs) = c.borrow().as_ref() {
            // SAFETY: both pointers were installed by `present` on this
            // same thread and are cleared before the objects are released.
            let (delegate, panel) = unsafe { (&*ptrs.delegate, &*ptrs.panel) };
            delegate.zeroize_fields();
            panel.orderOut(None);
            let mtm = MainThreadMarker::new().expect("panel code runs on the main thread");
            // SAFETY: main thread, inside the modal session being ended.
            unsafe { NSApplication::sharedApplication(mtm).abortModal() };
        }
    });
}

/// UI-02 evidence, sampled by the watchdog inside the running modal
/// session after the panel has had time to become key and hand its
/// secure field the field editor. Debug builds only.
#[cfg(debug_assertions)]
pub(super) fn probe_current() {
    CURRENT.with(|c| {
        if let Some(ptrs) = c.borrow().as_ref() {
            // SAFETY: installed by `present` on this thread; still live.
            let panel = unsafe { &*ptrs.panel };
            let mtm = MainThreadMarker::new().expect("panel code runs on the main thread");
            // SAFETY: plain property read on the shared application.
            let app_active = unsafe { NSApplication::sharedApplication(mtm).isActive() };
            write_probe(&panel.title().to_string(), app_active, panel.isKeyWindow());
        }
    });
}

/// Build, present, and run one modal panel. Main-thread only.
pub fn present(req: &PanelRequest, abort: Arc<AtomicBool>) -> PanelOutcome {
    let Some(mtm) = MainThreadMarker::new() else {
        return PanelOutcome::Cancelled;
    };
    // Preempted before the main queue reached us: never show the panel.
    if abort.load(Ordering::SeqCst) {
        return PanelOutcome::Cancelled;
    }
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let (labels, mode) = field_plan(req);
    let delegate = VaultPanelDelegate::new(mtm);
    delegate.ivars().mode.set(mode);

    let height = 120.0 + 44.0 * labels.len() as f64;
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(380.0, height));
    let panel = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc::<NSWindow>(),
            rect,
            NSWindowStyleMask::Titled,
            NSBackingStoreType::NSBackingStoreBuffered,
            false,
        )
    };
    panel.setTitle(&NSString::from_str(req.title()));

    // SAFETY (whole layout section): main thread, all views freshly
    // created and parented within this block; no reentrancy.
    unsafe {
        let content = NSView::initWithFrame(mtm.alloc::<NSView>(), rect);
        let mut y = height - 56.0;
        let mut fields = Vec::new();
        for label in labels {
            let text = NSTextField::labelWithString(&NSString::from_str(label), mtm);
            text.setFrame(NSRect::new(NSPoint::new(20.0, y + 20.0), NSSize::new(340.0, 18.0)));
            content.addSubview(&text);
            let field = NSSecureTextField::initWithFrame(
                mtm.alloc::<NSSecureTextField>(),
                NSRect::new(NSPoint::new(20.0, y - 2.0), NSSize::new(340.0, 22.0)),
            );
            content.addSubview(&field);
            fields.push(field);
            y -= 44.0;
        }
        let error = NSTextField::labelWithString(&NSString::new(), mtm);
        error.setFrame(NSRect::new(NSPoint::new(20.0, 40.0), NSSize::new(340.0, 18.0)));
        content.addSubview(&error);

        let ok = NSButton::buttonWithTitle_target_action(
            &NSString::from_str("OK"),
            Some(as_any(&delegate)),
            Some(sel!(onOk:)),
            mtm,
        );
        ok.setFrame(NSRect::new(NSPoint::new(280.0, 8.0), NSSize::new(80.0, 28.0)));
        ok.setKeyEquivalent(&NSString::from_str("\r"));
        let cancel = NSButton::buttonWithTitle_target_action(
            &NSString::from_str("Cancel"),
            Some(as_any(&delegate)),
            Some(sel!(onCancel:)),
            mtm,
        );
        cancel.setFrame(NSRect::new(NSPoint::new(192.0, 8.0), NSSize::new(80.0, 28.0)));
        cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        content.addSubview(&ok);
        content.addSubview(&cancel);

        *delegate.ivars().panel.borrow_mut() = Some(panel.clone());
        *delegate.ivars().fields.borrow_mut() = fields;
        *delegate.ivars().error_label.borrow_mut() = Some(error);
        panel.setContentView(Some(&content));
    }

    CURRENT.with(|c| {
        *c.borrow_mut() = Some(SessionPtrs {
            panel: Retained::as_ptr(&panel),
            delegate: Retained::as_ptr(&delegate),
        })
    });

    panel.center();
    unsafe { app.activate() };
    if let Some(field) = fields_first(&delegate) {
        panel.makeFirstResponder(Some(as_responder(&field)));
    }
    panel.makeKeyAndOrderFront(None);

    // Blocks until the delegate stops the modal session or the watchdog
    // aborts it; the watchdog is torn down before the session state is.
    let watchdog = Watchdog::start(abort);
    unsafe { app.runModalForWindow(&panel) };
    drop(watchdog);
    panel.orderOut(None);
    CURRENT.with(|c| *c.borrow_mut() = None);

    let submitted = delegate.ivars().submitted.borrow_mut().take();
    match (req, submitted) {
        (_, None) => PanelOutcome::Cancelled,
        (PanelRequest::MpChange, Some(mut v)) => {
            let new = v.pop().unwrap_or_default();
            let _confirm = v.pop().unwrap_or_default();
            let old = v.pop().unwrap_or_default();
            PanelOutcome::SubmittedChange(to_secret(old), to_secret(new))
        }
        (PanelRequest::MpCreate, Some(v)) | (PanelRequest::MpEntry, Some(v)) => {
            PanelOutcome::Submitted(to_secret(v.into_iter().next().unwrap_or_default()))
        }
    }
}

fn fields_first(delegate: &VaultPanelDelegate) -> Option<Retained<NSSecureTextField>> {
    delegate.ivars().fields.borrow().first().cloned()
}
