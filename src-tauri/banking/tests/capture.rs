// Exercise the app's actual capture policy without linking Tauri/FFmpeg.
mod platform {
    pub mod activation {
        pub const VAULT_HELPER_BUNDLE_ID: &str = "com.racker.source-vault-helper";
    }
}

#[path = "../../src/core/banking_capture.rs"]
mod banking_capture;
#[path = "../../src/core/capture_exclusions.rs"]
mod capture_exclusions;

#[test]
fn bank_authorization_protects_external_browser_and_nested_surfaces() {
    let _guard = capture_exclusions::TEST_LOCK.lock().unwrap();
    let before = capture_exclusions::sensitive_surface_visible();
    {
        let _first = banking_capture::BankAuthorizationGuard::new();
        assert!(capture_exclusions::screen_capture_suppressed());
        assert!(!capture_exclusions::should_record_keystroke(
            false, false, false
        ));
        {
            let _second = banking_capture::BankAuthorizationGuard::new();
            assert!(banking_capture::bank_authorization_active());
        }
        assert!(banking_capture::bank_authorization_active());
    }
    assert!(!banking_capture::bank_authorization_active());
    assert_eq!(capture_exclusions::sensitive_surface_visible(), before);
}
