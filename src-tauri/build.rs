fn main() {
    #[cfg(target_os = "macos")]
    build_desktop_audio_helper();
    #[cfg(target_os = "macos")]
    build_dictation_helper();

    // Configure FFmpeg paths for macOS (Homebrew installation)
    #[cfg(target_os = "macos")]
    {
        // Check for Homebrew FFmpeg installation
        if std::path::Path::new("/opt/homebrew/Cellar/ffmpeg").exists() {
            println!("cargo:rustc-link-search=/opt/homebrew/lib");
            println!("cargo:rustc-link-lib=dylib=avcodec");
            println!("cargo:rustc-link-lib=dylib=avformat");
            println!("cargo:rustc-link-lib=dylib=avutil");
            println!("cargo:rustc-link-lib=dylib=swscale");

            // Set environment variables for ffmpeg-sys-next
            std::env::set_var("FFMPEG_DIR", "/opt/homebrew/Cellar/ffmpeg/8.0_1");
            std::env::set_var(
                "FFMPEG_INCLUDE_DIR",
                "/opt/homebrew/Cellar/ffmpeg/8.0_1/include",
            );
            std::env::set_var("FFMPEG_LIB_DIR", "/opt/homebrew/Cellar/ffmpeg/8.0_1/lib");
            std::env::set_var(
                "PKG_CONFIG_PATH",
                "/opt/homebrew/Cellar/ffmpeg/8.0_1/lib/pkgconfig",
            );
        }
    }

    tauri_build::build()
}

#[cfg(target_os = "macos")]
fn build_desktop_audio_helper() {
    let sources = [
        "src/native/source_desktop_audio.swift",
        "src/native/desktop_audio_core.swift",
    ];
    for source in sources {
        println!("cargo:rerun-if-changed={source}");
    }
    let output = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"))
        .join("source-desktop-audio");
    let status = std::process::Command::new("swiftc")
        .args(sources)
        .args(["-parse-as-library", "-O", "-o"])
        .arg(&output)
        .status()
        .expect("Swift is required to build the macOS desktop-audio helper");
    assert!(status.success(), "Failed to build the desktop-audio helper");
    println!(
        "cargo:rustc-env=SOURCE_DESKTOP_AUDIO_HELPER={}",
        output.display()
    );
}

#[cfg(target_os = "macos")]
fn build_dictation_helper() {
    // NOTE: sources live in the SwiftPM package dir so swiftc and
    // `swift build` compile one shared copy. engine_fluidaudio.swift is
    // SPM-only (needs the FluidAudio SDK); engine_stub.swift is swiftc-only.
    for source in [
        "native-pkg/Package.swift",
        "native-pkg/Package.resolved",
        "native-pkg/Sources/SourceDictation/source_dictation.swift",
        "native-pkg/Sources/SourceDictation/dictation_core.swift",
        "native-pkg/Sources/SourceDictation/right_option_hotkey.swift",
        "native-pkg/Sources/SourceDictation/focus_capture.swift",
        "native-pkg/Sources/SourceDictation/text_insertion.swift",
        "native-pkg/Sources/SourceDictation/mic_capture.swift",
        "native-pkg/Sources/SourceDictation/mic_level.swift",
        "native-pkg/Sources/SourceDictation/mic_partials.swift",
        "native-pkg/Sources/SourceDictation/input_device.swift",
        "native-pkg/Sources/SourceDictation/overlay.swift",
        "native-pkg/Sources/SourceDictation/engine_fluidaudio.swift",
        "native-pkg/Sources/SourceDictation/engine_stub.swift",
    ] {
        println!("cargo:rerun-if-changed={source}");
    }
    // Catch-all so a newly added helper file can never again silently
    // skip the rebuild the way overlay.swift did.
    println!("cargo:rerun-if-changed=native-pkg/Sources/SourceDictation");
    if let Some(binary) = build_spm_dictation_helper() {
        sign_dictation_helper(&binary);
        println!("cargo:rustc-env=SOURCE_DICTATION_HELPER={}", binary.display());
        return;
    }
    println!("cargo:warning=SwiftPM dictation build failed; falling back to stub helper");
    let output =
        std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"))
            .join("source-dictation");
    let sources = [
        "native-pkg/Sources/SourceDictation/source_dictation.swift",
        "native-pkg/Sources/SourceDictation/dictation_core.swift",
        "native-pkg/Sources/SourceDictation/right_option_hotkey.swift",
        "native-pkg/Sources/SourceDictation/focus_capture.swift",
        "native-pkg/Sources/SourceDictation/text_insertion.swift",
        "native-pkg/Sources/SourceDictation/mic_capture.swift",
        "native-pkg/Sources/SourceDictation/mic_level.swift",
        "native-pkg/Sources/SourceDictation/mic_partials.swift",
        "native-pkg/Sources/SourceDictation/input_device.swift",
        "native-pkg/Sources/SourceDictation/overlay.swift",
        "native-pkg/Sources/SourceDictation/engine_stub.swift",
    ];
    let status = std::process::Command::new("swiftc")
        .args(sources)
        .args([
            "-parse-as-library",
            "-O",
            "-framework",
            "AppKit",
            "-framework",
            "ApplicationServices",
            "-framework",
            "AVFoundation",
            "-o",
        ])
        .arg(&output)
        .status()
        .expect("Swift is required to build the macOS dictation helper");
    assert!(status.success(), "Failed to build the dictation helper");
    sign_dictation_helper(&output);
    println!(
        "cargo:rustc-env=SOURCE_DICTATION_HELPER={}",
        output.display()
    );
}

/// Release SwiftPM build of the full-engine helper. Returns the binary
/// path on success, None when the SDK cannot be resolved (offline).
#[cfg(target_os = "macos")]
fn build_spm_dictation_helper() -> Option<std::path::PathBuf> {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new("swift")
        .args(["build", "-c", "release", "--disable-automatic-resolution"])
        .current_dir(manifest_dir.join("native-pkg"))
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let binary = manifest_dir
        .join("native-pkg")
        .join(".build")
        .join("release")
        .join("SourceDictation");
    binary.exists().then_some(binary)
}

/// Give the helper a stable code-signing requirement so macOS Accessibility
/// approval survives rebuilds. Unsigned/ad-hoc helper builds are identified by
/// their changing CDHash, which makes an existing permission row look enabled
/// while AXIsProcessTrusted still returns false for the new binary.
#[cfg(target_os = "macos")]
fn sign_dictation_helper(binary: &std::path::Path) {
    let identity = std::env::var("SOURCE_CODESIGN_IDENTITY")
        .ok()
        .or_else(|| std::env::var("APPLE_SIGNING_IDENTITY").ok())
        .or_else(find_apple_development_identity);
    let Some(identity) = identity else {
        println!(
            "cargo:warning=No Apple Development signing identity found; \
             dictation Accessibility approval may not survive helper rebuilds"
        );
        return;
    };

    let status = std::process::Command::new("codesign")
        .args([
            "--force",
            "--sign",
            &identity,
            "--identifier",
            "com.racker.zero.dictation-helper",
            "--timestamp=none",
        ])
        .arg(binary)
        .status()
        .expect("codesign is required to sign the macOS dictation helper");
    assert!(
        status.success(),
        "Failed to sign the dictation helper with identity {identity}"
    );
    println!("cargo:warning=Signed dictation helper with {identity}");
}

#[cfg(target_os = "macos")]
fn find_apple_development_identity() -> Option<String> {
    let output = std::process::Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    text.lines().find_map(|line| {
        let start = line.find("\"Apple Development:")? + 1;
        let end = line[start..].find('"')? + start;
        Some(line[start..end].to_string())
    })
}
