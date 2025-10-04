# Cross-Platform Keyboard Monitoring

This module provides privacy-first keyboard event monitoring across macOS, Windows, and Linux platforms.

## Architecture

### Platform Abstraction
Each platform has its own implementation of a keyboard listener that provides a unified interface:

```rust
// macOS
MacOSKeyboardListener::new() -> (Listener, EventReceiver)

// Windows
WindowsKeyboardListener::new() -> (Listener, EventReceiver)

// Linux
LinuxKeyboardListener::new() -> (Listener, EventReceiver)
```

The `KeyboardRecorder` uses conditional compilation to select the appropriate platform listener:

```rust
#[cfg(target_os = "macos")]
use MacOSKeyboardListener as PlatformKeyboardListener;

#[cfg(target_os = "windows")]
use WindowsKeyboardListener as PlatformKeyboardListener;

#[cfg(target_os = "linux")]
use LinuxKeyboardListener as PlatformKeyboardListener;
```

## Platform-Specific Implementation

### macOS (`keyboard_macos.rs`)

**Technology:** Core Graphics Event Tap API

**Features:**
- Low-level keyboard event interception via `CGEventTap`
- App context detection using NSWorkspace
- Modifier state tracking (Shift, Ctrl, Alt, Cmd)
- Privacy filtering for password fields

**Permissions Required:**
- Accessibility permissions (via System Preferences → Security & Privacy → Privacy → Accessibility)

**Limitations:**
- Requires main thread integration for full functionality
- CGEventTap must be enabled on main CFRunLoop

### Windows (`keyboard_windows.rs`)

**Technology:** Windows Hook API (`SetWindowsHookExW`)

**Features:**
- Low-level keyboard hook (`WH_KEYBOARD_LL`)
- Virtual key code to character mapping
- Foreground window detection via `GetForegroundWindow`
- Process information extraction
- Modifier state tracking via `GetKeyState`

**Permissions Required:**
- None (standard user permissions)

**Limitations:**
- Hook callback runs in DLL context (requires careful thread handling)
- UI Automation API integration for element detection is complex (currently stubbed)

**Key Code Mapping:**
- `0x20`: Space
- `0x30-0x39`: Numbers (0-9) with shift variants (!@#$%^&*())
- `0x41-0x5A`: Letters (A-Z) with case sensitivity based on shift

### Linux (`keyboard_linux.rs`)

**Technology:** evdev (Event Device Interface)

**Features:**
- Direct reading from `/dev/input/event*` devices
- Multi-keyboard support (detects all connected keyboards)
- X11 integration for active window detection (optional)
- Async event processing with tokio

**Permissions Required:**
- User must be in the `input` group:
  ```bash
  sudo usermod -a -G input $USER
  # Log out and log back in for changes to take effect
  ```

**Dependencies:**
- `evdev = "0.12"` - Event device access
- `x11` - X11 display server integration (optional feature)

**Limitations:**
- Requires `input` group membership (checked at runtime with helpful error)
- Wayland support is limited (X11 fallback works for most cases)
- AT-SPI2 for UI element detection is not yet implemented
- Modifier state detection needs enhancement

**Device Detection:**
- Scans `/dev/input/` for event devices
- Identifies keyboards by checking for common keys (A, Enter, Space)
- Spawns async task per keyboard device

**X11 Integration:**
- Queries `_NET_ACTIVE_WINDOW` atom for focused window
- Extracts window title via `XGetWMName`
- Retrieves PID via `_NET_WM_PID` property
- Reads process name from `/proc/{pid}/comm`

## Privacy Architecture

All platforms implement multi-layer privacy filtering:

1. **Consent Check**: Requires `Feature::KeyboardRecording` consent before starting
2. **UI Element Detection**: Attempts to identify sensitive input fields
3. **Keyword Filtering**: Detects password-related labels (password, PIN, SSN, credit card, CVV)
4. **Sensitivity Flag**: Every `KeyboardEvent` includes `is_sensitive` boolean
5. **Database Filtering**: `KeyboardRecorder` only stores non-sensitive events

## Data Model

### KeyboardEvent
```rust
pub struct KeyboardEvent {
    pub timestamp: i64,
    pub event_type: KeyEventType,  // KeyDown | KeyUp
    pub key_code: u32,
    pub key_char: Option<char>,
    pub modifiers: ModifierState,
    pub app_context: AppContext,
    pub ui_element: Option<UiElement>,
    pub is_sensitive: bool,  // Privacy flag
}
```

### ModifierState
```rust
pub struct ModifierState {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,  // Cmd on macOS, Win on Windows, Super on Linux
}
```

### AppContext
```rust
pub struct AppContext {
    pub app_name: String,
    pub window_title: String,
    pub process_id: i32,
}
```

## Usage Example

```rust
use crate::platform::input::PlatformKeyboardListener;

// Create listener
let (mut listener, mut events) = PlatformKeyboardListener::new(consent_manager)?;

// Start listening
listener.start_listening().await?;

// Process events
tokio::spawn(async move {
    while let Some(event) = events.recv().await {
        if !event.is_sensitive {
            // Safe to log/store
            println!("Key: {:?}", event.key_char);
        }
    }
});

// Stop listening
listener.stop_listening().await?;
```

## Testing

### macOS Testing
- Test with TextEdit and secure input fields
- Verify Accessibility permission prompts
- Test modifier combinations (Cmd+C, etc.)

### Windows Testing
- Test with Notepad for basic input
- Verify hook installation succeeds
- Test modifiers (Ctrl+C, Win+D, etc.)
- Check foreground window detection

### Linux Testing
```bash
# Grant permissions
sudo usermod -a -G input $USER

# Verify device access
ls -l /dev/input/event*

# Test on X11
echo $DISPLAY  # Should show :0 or similar

# Run application
cargo run
```

**Test Cases:**
- Multiple keyboards connected
- X11 vs Wayland environments
- Permission denied scenario (helpful error message)
- Process name extraction from /proc

## Future Enhancements

### macOS
- [ ] Full CGEventTap activation on main thread
- [ ] Complete Accessibility API integration for UI element detection
- [ ] Window title extraction improvements

### Windows
- [ ] UI Automation API integration for focused element detection
- [ ] COM initialization for advanced features
- [ ] Extended virtual key code mappings

### Linux
- [ ] AT-SPI2 integration for accessibility features
- [ ] Wayland native support (currently relies on XWayland)
- [ ] Enhanced modifier state detection
- [ ] Input method (ibus/fcitx) integration for international keyboards

### All Platforms
- [ ] Configurable privacy filters
- [ ] Custom sensitive keyword lists
- [ ] Typing speed/rhythm analysis
- [ ] Keyboard shortcut pattern detection
- [ ] Multi-language keyboard layout support
