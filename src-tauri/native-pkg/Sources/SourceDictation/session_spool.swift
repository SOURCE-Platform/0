import Foundation

/// Where Right Option session recordings live while they are transcribed.
///
/// A recording is only a means to a transcript, so it is deleted as soon as
/// its transcription completes, and anything a crash or quit left behind is
/// swept at startup. Before this, every session was kept forever (436 MB of
/// WAVs). Clips Source hands over with TRANSCRIBE_FILE, such as mobile
/// recordings, live outside this spool and are never touched here.
enum SessionSpool {
    static var directory: URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("source-dictation-sessions", isDirectory: true)
    }

    static func discard(_ path: String) {
        try? FileManager.default.removeItem(atPath: path)
    }

    /// Remove leftover session recordings and live-partial snapshots. Files
    /// written in the last minute are skipped, so a session still being
    /// recorded by a helper that has not exited yet is left alone.
    static func sweepLeftovers() {
        let fileManager = FileManager.default
        let keys: [URLResourceKey] = [.contentModificationDateKey]
        let cutoff = Date().addingTimeInterval(-60)
        let sessions = (try? fileManager.contentsOfDirectory(
            at: directory, includingPropertiesForKeys: keys)) ?? []
        let partials = ((try? fileManager.contentsOfDirectory(
            at: fileManager.temporaryDirectory, includingPropertiesForKeys: keys)) ?? [])
            .filter { $0.lastPathComponent.hasPrefix("partial-") }
        var removed = 0
        for url in sessions + partials where url.pathExtension == "wav" {
            let modified = (try? url.resourceValues(forKeys: Set(keys)))?
                .contentModificationDate ?? .distantPast
            if modified < cutoff, (try? fileManager.removeItem(at: url)) != nil {
                removed += 1
            }
        }
        if removed > 0 {
            writeDictationLine("DEBUG discarded \(removed) leftover dictation recordings")
        }
    }
}
