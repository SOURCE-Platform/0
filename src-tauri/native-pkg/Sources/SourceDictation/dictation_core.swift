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
    private var focusTarget: CapturedFocusTarget?
    private let inserter = TextInserter()
    private let mic = MicSessionRecorder()

    init(parentPID: pid_t) {
        self.parentPID = parentPID
    }

    func start() {
        watchParent()
        hotkey = RightOptionHotkey(onToggle: { [weak self] in self?.toggleSession() })
        hotkey?.start()
        watchStdin()
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
            focusTarget = captureFocusedTarget()
            if mic.beginSession() {
                sessionAudioPath = mic.activeSessionPath
            }
            writeDictationLine("SESSION_STARTED \(id)")
        }
    }

    func stopSession() {
        if let id = activeSessionID {
            finishSession(id: id)
        }
    }

    private func finishSession(id: String) {
        activeSessionID = nil
        mic.endSessionFile()
        sessionAudioPath = mic.activeSessionPath
        writeDictationLine("SESSION_STOPPED \(id)")
        if let path = sessionAudioPath {
            sessionAudioPath = nil
            transcribeSessionAudio(id: id, path: path)
        }
    }

    private func transcribeSessionAudio(id: String, path: String) {
        let startedMs = Int64(Date().timeIntervalSince1970 * 1000)
        engine.transcribe(audioPath: path) { result in
            let endedMs = Int64(Date().timeIntervalSince1970 * 1000)
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

/// Modifier-only Right Option toggle via a CGEvent tap.
/// keyCode 61 == Right Option. Fires on release after a clean press
/// (no other key/mouse activity in between), mirroring FluidVoice's
/// `ModifierOnlyShortcutFlagsDecision` behavior in simplified form.
final class RightOptionHotkey {
    private let onToggle: () -> Void
    private var eventTap: CFMachPort?
    private var runLoopSource: CFRunLoopSource?
    private var rightOptionDown = false
    private var interrupted = false
    private var retryTimer: Timer?

    init(onToggle: @escaping () -> Void) {
        self.onToggle = onToggle
    }

    func start() {
        installTap()
        retryTimer = Timer.scheduledTimer(withTimeInterval: 5.0, repeats: true) { [weak self] _ in
            guard let self else { return }
            if self.eventTap == nil {
                self.installTap()
            }
        }
    }

    private func installTap() {
        let mask = (1 << CGEventType.flagsChanged.rawValue)
            | (1 << CGEventType.keyDown.rawValue)
            | (1 << CGEventType.leftMouseDown.rawValue)
            | (1 << CGEventType.rightMouseDown.rawValue)
        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .defaultTap,
            eventsOfInterest: CGEventMask(mask),
            callback: { _, _, event, refcon in
                guard let refcon else { return Unmanaged.passRetained(event) }
                let hotkey = Unmanaged<RightOptionHotkey>.fromOpaque(refcon).takeUnretainedValue()
                hotkey.handle(event: event)
                return Unmanaged.passRetained(event)
            },
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else {
            // No Accessibility permission (or headless session): stdin
            // START/STOP remain available; retry on the timer.
            writeDictationLine("ERROR {\"message\":\"event tap unavailable; grant Accessibility permission\"}")
            return
        }
        eventTap = tap
        runLoopSource = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        if let source = runLoopSource {
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, .commonModes)
        }
        CGEvent.tapEnable(tap: tap, enable: true)
    }

    private func handle(event: CGEvent) {
        // Ignore keystrokes the inserter synthesized itself.
        if event.getIntegerValueField(.eventSourceUserData) == synthesizedEventTag {
            return
        }
        let type = event.type
        if type == .keyDown || type == .leftMouseDown || type == .rightMouseDown {
            interrupted = true
            return
        }
        guard type == .flagsChanged else { return }
        let keyCode = event.getIntegerValueField(.keyboardEventKeycode)
        let flags = event.flags
        if keyCode == 61 {
            if flags.contains(.maskAlternate) {
                rightOptionDown = true
                interrupted = false
            } else if rightOptionDown {
                rightOptionDown = false
                if !interrupted {
                    onToggle()
                }
                interrupted = false
            }
        } else if rightOptionDown {
            interrupted = true
        }
    }
}
