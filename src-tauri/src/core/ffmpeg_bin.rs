use std::path::PathBuf;

/// Resolve the `ffmpeg` CLI binary.
///
/// Callers used to rely on `PATH`, but the bundled `.app` launched from
/// Spotlight/Finder gets a minimal `PATH` (`/usr/bin:/bin/...`) that does
/// not include Homebrew, so spawning `ffmpeg` failed with
/// `No such file or directory (os error 2)` and microphone/camera
/// enumeration broke. Resolution order:
/// 1. `SOURCE_FFMPEG_PATH` / `FFMPEG_PATH` env override (must exist).
/// 2. Well-known Homebrew install locations.
/// 3. Plain `ffmpeg` PATH lookup (dev shells, future bundled sidecar on PATH).
pub fn ffmpeg_program() -> PathBuf {
    for key in ["SOURCE_FFMPEG_PATH", "FFMPEG_PATH"] {
        if let Ok(candidate) = std::env::var(key) {
            let path = PathBuf::from(candidate.trim());
            if path.is_file() {
                return path;
            }
        }
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
