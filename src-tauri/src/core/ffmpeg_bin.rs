use std::path::PathBuf;

/// Resolve the `ffmpeg` CLI binary.
///
/// Callers used to rely on `PATH`, but the bundled `.app` launched from
/// Spotlight/Finder gets a minimal `PATH` (`/usr/bin:/bin/...`) that does
/// not include Homebrew, so spawning `ffmpeg` failed with
/// `No such file or directory (os error 2)` and microphone/camera
/// enumeration broke. Resolution order:
/// 1. `SOURCE_FFMPEG_PATH` / `FFMPEG_PATH` env override (must exist).
/// 2. The copy bundled next to the app binary (`Contents/MacOS/ffmpeg`,
///    placed by scripts/bundle-macos-dylibs.mjs) so a Homebrew upgrade or
///    cleanup cannot break capture in the installed app.
/// 3. Well-known Homebrew install locations (dev builds).
/// 4. Plain `ffmpeg` PATH lookup (dev shells).
pub fn ffmpeg_program() -> PathBuf {
    for key in ["SOURCE_FFMPEG_PATH", "FFMPEG_PATH"] {
        if let Ok(candidate) = std::env::var(key) {
            let path = PathBuf::from(candidate.trim());
            if path.is_file() {
                return path;
            }
        }
    }
    if let Some(bundled) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("ffmpeg")))
        .filter(|path| path.is_file())
    {
        return bundled;
    }
    for candidate in [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return path;
        }
    }
    PathBuf::from("ffmpeg")
}
