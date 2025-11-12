fn main() {
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
            std::env::set_var("FFMPEG_INCLUDE_DIR", "/opt/homebrew/Cellar/ffmpeg/8.0_1/include");
            std::env::set_var("FFMPEG_LIB_DIR", "/opt/homebrew/Cellar/ffmpeg/8.0_1/lib");
            std::env::set_var("PKG_CONFIG_PATH", "/opt/homebrew/Cellar/ffmpeg/8.0_1/lib/pkgconfig");
        }
    }

    tauri_build::build()
}
