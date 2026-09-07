import AVFoundation
import Foundation

/// Headless mic capture for dictation sessions.
/// Records the default input at 16 kHz mono into per-session WAV files
/// the transcription engine can read directly. Best-effort: emits ERROR
/// lines instead of crashing when permission or devices fail.
final class MicSessionRecorder {
    private var engine: AVAudioEngine?
    private var sessionFile: AVAudioFile?
    private var converter: AVAudioConverter?
    private var sessionURL: URL?

    var activeSessionPath: String? { sessionURL?.path }

    /// Live loudness (0..1 RMS-ish) for the overlay waveform. Called on the
    /// audio tap thread, throttled to ~20 Hz.
    var onLevel: ((Float) -> Void)?
    private var lastLevelAt = Date.distantPast

    /// Converted 16 kHz mono samples kept for display-only partial
    /// transcriptions (capped at ~60 s). Reading the open WAV file directly
    /// gives a broken header, so partials come from memory instead.
    /// Guarded by lock: the audio tap appends on a realtime thread while
    /// the main-thread timer snapshots.
    private var partialSamples: [Float] = []
    private let partialLock = NSLock()
    private static let maxPartialSamples = 960_000

    /// Begin recording a session. Stops any previous session file first.
    func beginSession() -> Bool {
        endSessionFile()
        partialLock.lock()
        partialSamples = []
        partialLock.unlock()
        let spool = FileManager.default.temporaryDirectory
            .appendingPathComponent("source-dictation-sessions", isDirectory: true)
        do {
            try FileManager.default.createDirectory(
                at: spool, withIntermediateDirectories: true
            )
        } catch {
            writeDictationLine("ERROR {\"message\":\"mic spool failed\"}")
            return false
        }
        let url = spool.appendingPathComponent("\(UUID().uuidString).wav")
        guard let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false) else {
            return false
        }
        do {
            sessionFile = try AVAudioFile(forWriting: url, settings: format.settings)
        } catch {
            writeDictationLine("ERROR {\"message\":\"mic file failed\"}")
            return false
        }
        sessionURL = url

        let audioEngine = AVAudioEngine()
        let input = audioEngine.inputNode
        guard input.inputFormat(forBus: 0).channelCount > 0 else {
            writeDictationLine("ERROR {\"message\":\"no mic input\"}")
            return false
        }
        converter = AVAudioConverter(from: input.outputFormat(forBus: 0), to: format)
        input.installTap(onBus: 0, bufferSize: 4096, format: input.outputFormat(forBus: 0)) {
            [weak self] buffer, _ in
            self?.append(buffer: buffer, target: format)
        }
        do {
            try audioEngine.start()
        } catch {
            writeDictationLine("ERROR {\"message\":\"mic start failed\"}")
            return false
        }
        engine = audioEngine
        return true
    }

    /// Stop recording; the finished WAV path stays available for transcription.
    func endSessionFile() {
        engine?.inputNode.removeTap(onBus: 0)
        engine?.stop()
        engine = nil
        sessionFile = nil
        converter = nil
    }

    func discardSession() {
        endSessionFile()
        if let url = sessionURL {
            try? FileManager.default.removeItem(at: url)
        }
        sessionURL = nil
    }

    private func reportLevel(buffer: AVAudioPCMBuffer) {
        let now = Date()
        guard now.timeIntervalSince(lastLevelAt) > 0.05 else { return }
        lastLevelAt = now
        let rms: Float
        if let channel = buffer.floatChannelData?[0] {
            rms = rmsOfFloats(channel, count: Int(buffer.frameLength))
        } else if let channel = buffer.int16ChannelData?[0] {
            rms = rmsOfInt16(channel, count: Int(buffer.frameLength))
        } else {
            return
        }
        onLevel?(min(1.0, rms * 6.0))
    }

    private func rmsOfFloats(_ channel: UnsafePointer<Float>, count: Int) -> Float {
        guard count > 0 else { return 0 }
        var sum: Float = 0
        for i in 0..<count {
            sum += channel[i] * channel[i]
        }
        return sqrt(sum / Float(count))
    }

    private func rmsOfInt16(_ channel: UnsafePointer<Int16>, count: Int) -> Float {
        guard count > 0 else { return 0 }
        var sum: Float = 0
        for i in 0..<count {
            let sample = Float(channel[i]) / 32768.0
            sum += sample * sample
        }
        return sqrt(sum / Float(count))
    }

    /// Valid standalone WAV of the recent session audio (last ~30 s),
    /// or nil when there is nothing to transcribe yet. Bounded so
    /// display-only partials stay fast no matter how long dictation runs.
    private static let partialTailSamples = 480_000

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

    private func append(buffer: AVAudioPCMBuffer, target: AVAudioFormat) {
        reportLevel(buffer: buffer)
        guard let converter, let file = sessionFile else { return }
        let ratio = target.sampleRate / buffer.format.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 16
        guard let output = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else {
            return
        }
        var error: NSError?
        converter.convert(to: output, error: &error) { _, outStatus in
            outStatus.pointee = .haveData
            return buffer
        }
        if error == nil, output.frameLength > 0 {
            try? file.write(from: output)
            if let channel = output.floatChannelData?[0] {
                let count = Int(output.frameLength)
                partialLock.lock()
                partialSamples.append(contentsOf: UnsafeBufferPointer(start: channel, count: count))
                if partialSamples.count > Self.maxPartialSamples {
                    partialSamples.removeFirst(partialSamples.count - Self.maxPartialSamples)
                }
                partialLock.unlock()
            }
        }
    }
}
