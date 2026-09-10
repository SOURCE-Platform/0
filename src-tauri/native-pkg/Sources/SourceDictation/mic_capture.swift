import AVFoundation
import Foundation

/// Headless mic capture for dictation sessions.
/// Records the default input at 16 kHz mono into per-session WAV files
/// the transcription engine can read directly. Best-effort: emits ERROR
/// lines instead of crashing when permission or devices fail.
///
/// Survives the input device being reconfigured mid-session. Another client
/// opening or closing the same microphone — Source's own ambient capture does
/// this every two seconds — makes AVAudioEngine stop delivering audio. Without
/// reconnecting, everything after that point was silently lost.
final class MicSessionRecorder: @unchecked Sendable {
    private var engine: AVAudioEngine?
    private var sessionFile: AVAudioFile?
    private var converter: AVAudioConverter?
    private var sessionURL: URL?
    private var configObserver: NSObjectProtocol?

    /// Serialises engine start, stop, and reconnect, so a device change that
    /// lands mid-teardown can't race a session ending.
    private let lifecycleQueue = DispatchQueue(label: "source-dictation.mic-lifecycle")
    /// Guards the converter and file: the realtime tap uses them while a
    /// reconnect or a session end may be swapping them out.
    private let ioLock = NSLock()

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
        let file: AVAudioFile
        do {
            file = try AVAudioFile(forWriting: url, settings: format.settings)
        } catch {
            writeDictationLine("ERROR {\"message\":\"mic file failed\"}")
            return false
        }
        ioLock.lock()
        sessionFile = file
        ioLock.unlock()
        sessionURL = url
        return lifecycleQueue.sync { startEngine(target: format) }
    }

    /// Stop recording; the finished WAV path stays available for transcription.
    func endSessionFile() {
        lifecycleQueue.sync { teardownEngine(engine) }
        ioLock.lock()
        sessionFile = nil
        converter = nil
        ioLock.unlock()
    }

    func discardSession() {
        endSessionFile()
        if let url = sessionURL {
            try? FileManager.default.removeItem(at: url)
        }
        sessionURL = nil
    }

    /// Open the current default input and feed the session file.
    /// Must run on `lifecycleQueue`.
    private func startEngine(target: AVAudioFormat) -> Bool {
        let audioEngine = AVAudioEngine()
        let input = audioEngine.inputNode
        let inputFormat = input.outputFormat(forBus: 0)
        guard input.inputFormat(forBus: 0).channelCount > 0, inputFormat.sampleRate > 0 else {
            writeDictationLine("ERROR {\"message\":\"no mic input\"}")
            return false
        }
        guard let newConverter = AVAudioConverter(from: inputFormat, to: target) else {
            writeDictationLine("ERROR {\"message\":\"mic format unsupported\"}")
            return false
        }
        ioLock.lock()
        converter = newConverter
        ioLock.unlock()
        input.installTap(onBus: 0, bufferSize: 4096, format: inputFormat) {
            [weak self] buffer, _ in
            self?.append(buffer: buffer, target: target)
        }
        configObserver = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange,
            object: audioEngine,
            queue: nil
        ) { [weak self] _ in
            self?.reconnectAfterConfigurationChange(target: target)
        }
        do {
            try audioEngine.start()
        } catch {
            teardownEngine(audioEngine)
            writeDictationLine("ERROR {\"message\":\"mic start failed\"}")
            return false
        }
        engine = audioEngine
        return true
    }

    /// Must run on `lifecycleQueue`.
    private func teardownEngine(_ audioEngine: AVAudioEngine?) {
        if let observer = configObserver {
            NotificationCenter.default.removeObserver(observer)
            configObserver = nil
        }
        audioEngine?.inputNode.removeTap(onBus: 0)
        audioEngine?.stop()
        if engine === audioEngine {
            engine = nil
        }
    }

    /// The device was reconfigured and the engine has stopped. Rebuild it on
    /// the new configuration and keep writing to the same session file.
    private func reconnectAfterConfigurationChange(target: AVAudioFormat) {
        lifecycleQueue.async { [weak self] in
            guard let self else { return }
            self.ioLock.lock()
            let sessionActive = self.sessionFile != nil
            self.ioLock.unlock()
            // A change can land just after the session ended: nothing to resume.
            guard sessionActive, let stale = self.engine else { return }
            self.teardownEngine(stale)
            if self.startEngine(target: target) {
                writeDictationLine("DEBUG mic reconfigured mid-session; reconnected")
            } else {
                writeDictationLine("ERROR {\"message\":\"mic lost mid-session; keeping what was recorded\"}")
            }
        }
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
        ioLock.lock()
        defer { ioLock.unlock() }
        guard let converter, let file = sessionFile else { return }
        let ratio = target.sampleRate / buffer.format.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 16
        guard let output = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else {
            return
        }
        var error: NSError?
        // Hand the buffer over exactly once. Returning it on every callback made
        // the converter re-read it whenever it wanted more input, duplicating audio.
        var supplied = false
        converter.convert(to: output, error: &error) { _, outStatus in
            if supplied {
                outStatus.pointee = .noDataNow
                return nil
            }
            supplied = true
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
