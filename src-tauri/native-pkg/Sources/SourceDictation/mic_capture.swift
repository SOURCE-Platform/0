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
    /// When the tap last delivered audio. Guarded by `ioLock`.
    private var lastBufferAt = DispatchTime(uptimeNanoseconds: 0)
    /// When the engine last rebuilt after a configuration change.
    /// `lifecycleQueue` only.
    private var lastRebuildAt = DispatchTime(uptimeNanoseconds: 0)
    /// How much audio this session captured, and whether any of it was
    /// non-zero. Guarded by `ioLock`. A session that ends with no frames, or
    /// with nothing but exact zeros, is a failure that used to look exactly
    /// like success: the file existed, the engine reported no error, and the
    /// transcript came back empty. It is reported loudly now.
    private var sessionFrames: AVAudioFrameCount = 0
    private var sessionPeak: Float = 0

    var activeSessionPath: String? { sessionURL?.path }

    /// Live loudness (0..1 RMS-ish) for the overlay waveform. Called on the
    /// audio tap thread, throttled to ~20 Hz.
    var onLevel: ((Float) -> Void)?
    var lastLevelAt = Date.distantPast

    /// Converted 16 kHz mono samples kept for display-only partial
    /// transcriptions (capped at ~60 s). Reading the open WAV file directly
    /// gives a broken header, so partials come from memory instead.
    /// Guarded by lock: the audio tap appends on a realtime thread while
    /// the main-thread timer snapshots.
    var partialSamples: [Float] = []
    let partialLock = NSLock()
    static let maxPartialSamples = 960_000

    /// Microphone denial arrives as silence, not as an error: the engine starts
    /// happily and the tap delivers zero-filled buffers. Say so out loud, so a
    /// revoked grant can never again be mistaken for a capture bug.
    private func reportMicrophoneAccess() {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            return
        case .notDetermined:
            writeDictationLine(
                "ERROR {\"message\":\"microphone access has never been granted to the dictation helper\"}"
            )
        default:
            writeDictationLine(
                "ERROR {\"message\":\"microphone access denied; enable Source under Privacy & Security > Microphone\"}"
            )
        }
    }

    /// Begin recording a session. Stops any previous session file first.
    func beginSession() -> Bool {
        endSessionFile()
        reportMicrophoneAccess()
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
        sessionFrames = 0
        sessionPeak = 0
        ioLock.unlock()
        sessionURL = url
        return lifecycleQueue.sync { startEngine(target: format) }
    }

    /// Stop recording; the finished WAV path stays available for transcription.
    func endSessionFile() {
        lifecycleQueue.sync { teardownEngine(engine) }
        ioLock.lock()
        let hadSession = sessionFile != nil
        let frames = sessionFrames
        let peak = sessionPeak
        sessionFile = nil
        converter = nil
        ioLock.unlock()
        guard hadSession else { return }
        if frames == 0 {
            writeDictationLine(
                "ERROR {\"message\":\"microphone delivered no audio this session\"}"
            )
        } else if peak == 0 {
            writeDictationLine(
                "ERROR {\"message\":\"microphone recorded only silence this session\"}"
            )
        }
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
        // Record from the microphone chosen in Source, not just the macOS default.
        // Chosen before any format is read, since formats follow the device.
        let source = DictationInput.apply(to: audioEngine)
        let input = audioEngine.inputNode
        // Tap the HARDWARE format, never `outputFormat(forBus:)`. Selecting a
        // device on the AUHAL leaves the node's output format reporting the
        // PREVIOUS device's sample rate, and it never catches up while the
        // engine lives. Installing a tap at that stale rate makes AUHAL deliver
        // zero buffers — sessions that record digital silence with no error
        // anywhere — or, when the mismatch is visible by the time the tap is
        // created, throw an uncatchable format-mismatch exception. Only
        // `inputFormat(forBus:)` tracks the device that was actually selected.
        let inputFormat = input.inputFormat(forBus: 0)
        guard inputFormat.channelCount > 0, inputFormat.sampleRate > 0 else {
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
        writeDictationLine("DEBUG dictation mic: \(source)")
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

    /// The device was reconfigured and the engine may have stopped. Rebuild
    /// only when audio actually stops arriving: Source's own ambient capture
    /// opens and closes this same microphone every couple of seconds, and
    /// each of those posts this notification while the stream is perfectly
    /// healthy. Rebuilding for every one of them chopped sessions into
    /// silence, so notifications while audio flows are ignored, and rebuilds
    /// are rate-limited to one every few seconds.
    private func reconnectAfterConfigurationChange(target: AVAudioFormat) {
        let noticedAt = DispatchTime.now()
        lifecycleQueue.asyncAfter(deadline: .now() + .milliseconds(800)) { [weak self] in
            guard let self else { return }
            self.ioLock.lock()
            let flowing = self.lastBufferAt > noticedAt
            self.ioLock.unlock()
            guard !flowing else { return }
            let now = DispatchTime.now()
            guard now > self.lastRebuildAt + .seconds(3) else { return }
            self.lastRebuildAt = now
            self.rebuildEngine(target: target)
        }
    }

    /// Must run on `lifecycleQueue`.
    private func rebuildEngine(target: AVAudioFormat) {
        ioLock.lock()
        let sessionActive = sessionFile != nil
        ioLock.unlock()
        // A change can land just after the session ended: nothing to resume.
        guard sessionActive, let stale = engine else { return }
        teardownEngine(stale)
        if startEngine(target: target) {
            writeDictationLine("DEBUG mic reconfigured mid-session; reconnected")
        } else {
            writeDictationLine("ERROR {\"message\":\"mic lost mid-session; keeping what was recorded\"}")
        }
    }

    private func append(buffer: AVAudioPCMBuffer, target: AVAudioFormat) {
        reportLevel(buffer: buffer)
        ioLock.lock()
        defer { ioLock.unlock() }
        guard let converter, let file = sessionFile else { return }
        lastBufferAt = .now()
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
            sessionFrames += output.frameLength
            if let channel = output.floatChannelData?[0] {
                let count = Int(output.frameLength)
                for i in 0..<count {
                    sessionPeak = max(sessionPeak, abs(channel[i]))
                }
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
