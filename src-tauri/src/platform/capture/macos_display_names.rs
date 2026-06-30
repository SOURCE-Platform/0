use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use objc::{class, msg_send, sel, sel_impl};
use std::collections::HashMap;
use std::ffi::CStr;

fn nsstring_to_string(value: id) -> Option<String> {
    if value == nil {
        return None;
    }

    unsafe {
        let c_str: *const i8 = msg_send![value, UTF8String];
        if c_str.is_null() {
            None
        } else {
            Some(CStr::from_ptr(c_str).to_string_lossy().into_owned())
        }
    }
}

pub(super) fn get_display_names() -> HashMap<u32, String> {
    unsafe {
        let mut names = HashMap::new();
        let screens: id = msg_send![class!(NSScreen), screens];
        if screens == nil {
            return names;
        }

        let count: usize = msg_send![screens, count];
        let screen_number_key = NSString::alloc(nil).init_str("NSScreenNumber");

        for index in 0..count {
            let screen: id = msg_send![screens, objectAtIndex: index];
            if screen == nil {
                continue;
            }

            let description: id = msg_send![screen, deviceDescription];
            if description == nil {
                continue;
            }

            let screen_number: id = msg_send![description, objectForKey: screen_number_key];
            if screen_number == nil {
                continue;
            }

            let display_id: u32 = msg_send![screen_number, unsignedIntValue];
            let has_localized_name: bool =
                msg_send![screen, respondsToSelector: sel!(localizedName)];
            if !has_localized_name {
                continue;
            }

            let localized_name: id = msg_send![screen, localizedName];
            if let Some(name) = nsstring_to_string(localized_name) {
                names.insert(display_id, name);
            }
        }

        names
    }
}
