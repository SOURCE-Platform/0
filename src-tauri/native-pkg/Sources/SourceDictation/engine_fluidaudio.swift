import FluidAudio
import Foundation

/// SwiftPM-only engine: real Parakeet transcription on the Apple Neural
/// Engine. Shares the model cache in ~/Library/Application Support/FluidAudio
/// with every other FluidAudio app, so models download once.
final class FluidAudioTranscriptionEngine: TranscriptionEngine, @unchecked Sendable {
    private var manager: AsrManager?
    private let setup = SetupOnce()

    func transcribe(audioPath: String, completion: @escaping @Sendable (EngineTranscription) -> Void) {
        Task {
            do {
                let manager = try await setupManager()
                var decoderState = try TdtDecoderState()
                let url = URL(fileURLWithPath: audioPath)
                let result = try await manager.transcribe(url, decoderState: &decoderState)
                completion(
                    EngineTranscription(
                        text: result.text,
                        language: nil,
                        confidence: result.confidence,
                        model: "parakeet-tdt (FluidAudio)"
                    )
                )
            } catch {
                writeDictationLine("ERROR {\"message\":\"transcription failed\"}")
                completion(
                    EngineTranscription(
                        text: "",
                        language: nil,
                        confidence: nil,
                        model: "parakeet-tdt (FluidAudio)"
                    )
                )
            }
        }
    }

    private func setupManager() async throws -> AsrManager {
        if let manager {
            return manager
        }
        return try await setup.run {
            let models = try await AsrModels.downloadAndLoad()
            let manager = AsrManager()
            try await manager.loadModels(models)
            self.manager = manager
            return manager
        }
    }
}

/// Runs the loader once even under concurrent transcribe calls.
private final class SetupOnce: @unchecked Sendable {
    private var task: Task<AsrManager, Error>?

    func run(_ loader: @escaping @Sendable () async throws -> AsrManager) async throws -> AsrManager {
        if let task {
            return try await task.value
        }
        let task = Task { try await loader() }
        self.task = task
        do {
            return try await task.value
        } catch {
            self.task = nil
            throw error
        }
    }
}

func makeDefaultEngine() -> TranscriptionEngine {
    FluidAudioTranscriptionEngine()
}
