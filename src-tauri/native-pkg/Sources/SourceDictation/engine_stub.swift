import Foundation

/// swiftc-only engine: keeps the TRANSCRIPT contract testable without the
/// FluidAudio SDK. The SwiftPM build excludes this file and uses
/// engine_fluidaudio.swift instead (see Package.swift).
struct StubTranscriptionEngine: TranscriptionEngine {
    func transcribe(audioPath: String, completion: @escaping @Sendable (EngineTranscription) -> Void) {
        completion(
            EngineTranscription(
                text: "",
                language: nil,
                confidence: nil,
                model: "parakeet-tdt-v3 (stub)"
            )
        )
    }
}

func makeDefaultEngine() -> TranscriptionEngine {
    StubTranscriptionEngine()
}
