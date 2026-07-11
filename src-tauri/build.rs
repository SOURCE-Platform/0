fn main() {
    #[cfg(target_os = "macos")]
    build_desktop_audio_helper();

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
    let source = "src/native/source_desktop_audio.swift";
    println!("cargo:rerun-if-changed={source}");
    let output = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"))
        .join("source-desktop-audio");
    let status = std::process::Command::new("swiftc")
        .args([source, "-parse-as-library", "-O", "-o"])
        .arg(&output)
        .status()
        .expect("Swift is required to build the macOS desktop-audio helper");
    assert!(status.success(), "Failed to build the desktop-audio helper");
    println!(
        "cargo:rustc-env=SOURCE_DESKTOP_AUDIO_HELPER={}",
        output.display()
    );
}
