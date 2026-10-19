import FluidAudio
import Foundation

/// SwiftPM-only engine: Fluid Speech Mini (by FluidVoice, CC BY 4.0), loaded
/// as a drop-in Parakeet v3 model. Lives in the shared FluidAudio cache in
/// ~/Library/Application Support/FluidAudio, so every FluidAudio app on this
/// machine downloads models once.
///
/// The encoder runs on the GPU, not the Neural Engine. On the ANE, macOS
/// compiles the encoder for 1-2 minutes and keeps the result only minutes
/// (measured 2026-10-19 on an M2: a 12-minute-old compile was gone), so
/// nearly every Source launch left dictation unusable for that long, and
/// the compile starved the helper enough to freeze the pill and the hotkey.
/// GPU load: ~6 s cold, under 1 s warm; FluidAudio documents it as
/// WER-neutral and slightly faster.
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
                        model: "fluid-speech-mini (FluidAudio)"
                    )
                )
            } catch {
                writeDictationLine("ERROR {\"message\":\"transcription failed\"}")
                completion(
                    EngineTranscription(
                        text: "",
                        language: nil,
                        confidence: nil,
                        model: "fluid-speech-mini (FluidAudio)"
                    )
                )
            }
        }
    }

    /// Load Mini in the background at startup. A dictation that arrives
    /// while this is still running waits on the same load rather than
    /// starting another one. Utility priority keeps the load from competing
    /// with the hotkey, the mic and the pill on the main thread.
    func warmUp() {
        Task(priority: .utility) {
            let started = Date()
            do {
                _ = try await setupManager()
                let seconds = String(format: "%.1f", Date().timeIntervalSince(started))
                writeDictationLine("DEBUG speech model ready in \(seconds)s")
            } catch {
                writeDictationLine("DEBUG speech model warm-up failed; it will load on first use")
            }
        }
    }

    private func setupManager() async throws -> AsrManager {
        if let manager {
            return manager
        }
        return try await setup.run {
            let directory = MiniModel.defaultDirectory()
            try await MiniModel.ensureAvailable(at: directory)
            let models = try AsrModels.loadLocal(
                from: directory,
                version: .v3,
                encoderComputeUnits: .cpuAndGPU
            )
            let manager = AsrManager()
            try await manager.loadModels(models)
            self.manager = manager
            return manager
        }
    }
}

/// Runs the loader once, however many callers arrive at the same time.
/// Startup warm-up and an early dictation can both ask for the model, so the
/// check-and-start is locked: without it both could start their own load.
private final class SetupOnce: @unchecked Sendable {
    private let lock = NSLock()
    private var task: Task<AsrManager, Error>?

    func run(_ loader: @escaping @Sendable () async throws -> AsrManager) async throws -> AsrManager {
        let current: Task<AsrManager, Error> = lock.withLock {
            if let task {
                return task
            }
            let started = Task { try await loader() }
            task = started
            return started
        }
        do {
            return try await current.value
        } catch {
            // Let a later call retry, unless another caller already replaced it.
            lock.withLock {
                if task == current {
                    task = nil
                }
            }
            throw error
        }
    }
}

func makeDefaultEngine() -> TranscriptionEngine {
    FluidAudioTranscriptionEngine()
}
