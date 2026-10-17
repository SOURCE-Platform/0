//! Move a file to the Trash (Phase I, spec §10.6): the export the user
//! just imported, on their explicit click. The Finder's own Trash, so the
//! user can still put it back; this is not secure deletion and the UI
//! says so.

use std::path::Path;

#[cfg(target_os = "macos")]
pub fn move_to_trash(path: &Path) -> bool {
    use cocoa::base::{id, nil, BOOL, YES};
    use cocoa::foundation::NSString;
    use objc::{class, msg_send, sel, sel_impl};

    let Some(text) = path.to_str() else {
        return false;
    };
    // SAFETY: Foundation messages with nil-checked objects; the two out
    // parameters are passed as nil (not wanted), which the API allows.
    unsafe {
        let ns_path = NSString::alloc(nil).init_str(text);
        let url: id = msg_send![class!(NSURL), fileURLWithPath: ns_path];
        let _: () = msg_send![ns_path, release];
        if url == nil {
            return false;
        }
        let fm: id = msg_send![class!(NSFileManager), defaultManager];
        let ok: BOOL = msg_send![fm, trashItemAtURL: url resultingItemURL: nil error: nil];
        ok == YES
    }
}

#[cfg(not(target_os = "macos"))]
pub fn move_to_trash(_path: &Path) -> bool {
    false
}
