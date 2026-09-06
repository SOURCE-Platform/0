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
        "native-pkg/Sources/SourceDictation/focus_capture.swift",
        "native-pkg/Sources/SourceDictation/text_insertion.swift",
        "native-pkg/Sources/SourceDictation/mic_capture.swift",
        "native-pkg/Sources/SourceDictation/engine_fluidaudio.swift",
        "native-pkg/Sources/SourceDictation/engine_stub.swift",
    ] {
        println!("cargo:rerun-if-changed={source}");
    }
    if let Some(binary) = build_spm_dictation_helper() {
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
        "native-pkg/Sources/SourceDictation/focus_capture.swift",
        "native-pkg/Sources/SourceDictation/text_insertion.swift",
        "native-pkg/Sources/SourceDictation/mic_capture.swift",
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
