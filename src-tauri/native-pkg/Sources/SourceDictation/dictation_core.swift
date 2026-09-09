import AppKit
import Darwin
import Foundation

enum DictationError: Error {
    case invalidArguments
}

func writeDictationLine(_ line: String) {
    if let data = (line + "\n").data(using: .utf8) {
        FileHandle.standardOutput.write(data)
    }
}

/// Phase 1 skeleton: lifecycle + line protocol only.
/// Phase 2 plugs in: CGEvent Right Option tap, Core Audio capture,
/// FluidAudio Parakeet (v2/v3), overlay + TypingService-equivalent.
/// Session state is mutated on the main runloop only: the hotkey tap and
/// timers already run there, and stdin commands hop there explicitly.
final class DictationRuntime: @unchecked Sendable {
    private let parentPID: pid_t
    private var parentTimer: Timer?
    private var hotkey: RightOptionHotkey?
    private var engine: TranscriptionEngine = makeDefaultEngine()
    private var activeSessionID: String?
    private var sessionAudioPath: String?
    private var sessionStartedAtMs: Int64?
    private var focusTarget: CapturedFocusTarget?
    private let inserter = TextInserter()
    private let mic = MicSessionRecorder()
    private var partialTimer: Timer?
    private var partialBusy = false

    init(parentPID: pid_t) {
        self.parentPID = parentPID
    }

    func start() {
        setupPresentation()
        mic.onLevel = { level in ListeningIndicator.shared.setLevel(level) }
        ListeningIndicator.shared.onOpenSettings = {
            writeDictationLine("OPEN_SETTINGS")
        }
        watchParent()
        hotkey = RightOptionHotkey(onToggle: { [weak self] in self?.toggleSession() })
        hotkey?.start()
        watchStdin()
    }

    /// Stay out of the Dock while allowing AppKit to deliver mouse events
    /// to the floating listening panel.
    private func setupPresentation() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
    }

    /// First Right Option press opens a session, second closes it.
    /// Manual START/STOP over stdin drive the same path (testing + fallback).
    func toggleSession() {
        if let id = activeSessionID {
            finishSession(id: id)
        } else {
            let id = UUID().uuidString
            activeSessionID = id
            sessionAudioPath = nil
            sessionStartedAtMs = Int64(Date().timeIntervalSince1970 * 1000)
            focusTarget = captureFocusedTarget()
            if mic.beginSession() {
                sessionAudioPath = mic.activeSessionPath
            }
            ListeningIndicator.shared.show()
            startPartialTimer(id: id)
            writeDictationLine("SESSION_STARTED \(id)")
        }
    }

    /// Display-only live words: re-transcribe a snapshot of the growing
    /// session file every few seconds for the pill label. The authoritative
    /// transcript still comes from the final TRANSCRIPT event.
    private func startPartialTimer(id: String) {
        partialTimer?.invalidate()
        partialTimer = Timer.scheduledTimer(withTimeInterval: 2.5, repeats: true) { [weak self] _ in
            guard let self, self.activeSessionID == id, !self.partialBusy,
                let snapshot = self.mic.partialSnapshotURL()
            else {
                return
            }
            self.partialBusy = true
            self.engine.transcribe(audioPath: snapshot.path) { result in
                try? FileManager.default.removeItem(at: snapshot)
                self.partialBusy = false
                if self.activeSessionID == id, !result.text.isEmpty {
                    ListeningIndicator.shared.setTranscript(result.text)
                }
            }
        }
    }

    func stopSession() {
        if let id = activeSessionID {
            finishSession(id: id)
        }
    }

    private func finishSession(id: String) {
        activeSessionID = nil
        partialTimer?.invalidate()
        partialTimer = nil
        partialBusy = false
        ListeningIndicator.shared.hide()
        mic.endSessionFile()
        sessionAudioPath = mic.activeSessionPath
        let startedAtMs = sessionStartedAtMs
        sessionStartedAtMs = nil
        writeDictationLine("SESSION_STOPPED \(id)")
        if let path = sessionAudioPath {
            sessionAudioPath = nil
            transcribeSessionAudio(id: id, path: path, startedAtMs: startedAtMs)
        }
    }

    private func transcribeSessionAudio(id: String, path: String, startedAtMs: Int64? = nil) {
        let startedMs = startedAtMs ?? Int64(Date().timeIntervalSince1970 * 1000)
        let endedMs = Int64(Date().timeIntervalSince1970 * 1000)
        engine.transcribe(audioPath: path) { result in
            let payload: [String: Any] = [
                "id": id,
                "text": result.text,
                "language": result.language as Any,
                "confidence": result.confidence as Any,
                "provider": "native-helper",
                "model": result.model,
                "startedAtMs": startedMs,
                "endedAtMs": endedMs,
                "source": "fluid-voice-prompt",
                "isFinal": true,
            ]
            if let data = try? JSONSerialization.data(withJSONObject: payload),
                let json = String(data: data, encoding: .utf8)
            {
                writeDictationLine("TRANSCRIPT \(json)")
            }
        }
    }

    private func watchParent() {
        parentTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            guard let self else { return }
            if kill(self.parentPID, 0) != 0 {
                exit(0)
            }
        }
    }

    private func watchStdin() {
        FileHandle.standardInput.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty, let line = String(data: data, encoding: .utf8) else { return }
            let commands = line.components(separatedBy: .newlines)
            DispatchQueue.main.async {
                for command in commands {
                    self?.handleCommand(command.trimmingCharacters(in: .whitespaces))
                }
            }
        }
    }

    private func handleCommand(_ command: String) {
        if command == "SHUTDOWN" {
            exit(0)
        }
        if command.hasPrefix("START") {
            if activeSessionID == nil {
                toggleSession()
            }
            return
        }
        if command == "STOP" {
            stopSession()
            return
        }
        if command.hasPrefix("TRANSCRIBE_FILE ") {
            let path = String(command.dropFirst("TRANSCRIBE_FILE ".count)).trimmingCharacters(in: .whitespaces)
            if !path.isEmpty {
                let id = UUID().uuidString
                transcribeSessionAudio(id: id, path: path)
            }
            return
        }
        if command.hasPrefix("INSERT ") {
            let payload = String(command.dropFirst("INSERT ".count))
            if let data = payload.data(using: .utf8),
                let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                let id = json["id"] as? String,
                let text = json["text"] as? String
            {
                let outcome = inserter.insert(text: text, target: focusTarget)
                if outcome.ok {
                    writeDictationLine("INSERTED \(id)")
                } else {
                    writeDictationLine("ERROR {\"message\":\"insertion failed: \(outcome.message)\"}")
                }
            } else {
                writeDictationLine("ERROR {\"message\":\"bad INSERT payload\"}")
            }
            return
        }
        // TODO(phase-4): FluidAudio Parakeet replaces StubTranscriptionEngine.
    }
}

/// Engine seam: Phase 4 backs this with FluidAudio Parakeet v2/v3 on the
/// Apple Neural Engine. The stub keeps the TRANSCRIPT contract testable now.
struct EngineTranscription {
    let text: String
    let language: String?
    let confidence: Float?
    let model: String
}

protocol TranscriptionEngine {
    func transcribe(audioPath: String, completion: @escaping @Sendable (EngineTranscription) -> Void)
}
