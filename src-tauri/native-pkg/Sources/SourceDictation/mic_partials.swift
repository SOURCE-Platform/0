import AVFoundation
import Foundation

/// Display-only partial transcripts. Reading the session WAV while it is still
/// open gives a broken header, so partials are rendered from the samples kept
/// in memory instead.
extension MicSessionRecorder {
    /// Valid standalone WAV of the recent session audio (last ~30 s),
    /// or nil when there is nothing to transcribe yet. Bounded so
    /// display-only partials stay fast no matter how long dictation runs.
    static let partialTailSamples = 480_000

    func partialSnapshotURL() -> URL? {
        partialLock.lock()
        let samples = partialSamples
        partialLock.unlock()
        guard !samples.isEmpty else { return nil }
        let tailStart = max(0, samples.count - Self.partialTailSamples)
        let tail = Array(samples[tailStart...])
        guard let format = AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false
        ) else {
            return nil
        }
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("partial-\(UUID().uuidString).wav")
        do {
            let file = try AVAudioFile(forWriting: url, settings: format.settings)
            let frames = AVAudioFrameCount(tail.count)
            guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames) else {
                return nil
            }
            buffer.frameLength = frames
            tail.withUnsafeBufferPointer { source in
                buffer.floatChannelData?[0].update(from: source.baseAddress!, count: source.count)
            }
            try file.write(from: buffer)
            return url
        } catch {
            return nil
        }
    }
}
