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

    /// Begin recording a session. Stops any previous session file first.
    func beginSession() -> Bool {
        endSessionFile()
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

    private func append(buffer: AVAudioPCMBuffer, target: AVAudioFormat) {
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
        }
    }
}
