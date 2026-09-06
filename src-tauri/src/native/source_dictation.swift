import Darwin
import Foundation

/// Entry point for the bundled dictation helper (Phase 1 skeleton).
/// Owns: Right Option hotkey, mic stream, FluidAudio inference,
/// overlay signalling, focus restore + text insertion.
/// Protocol (stdout lines): READY | TRANSCRIPT {...} | ERROR {...}
/// Protocol (stdin lines): START foregroundId | STOP | SHUTDOWN
@main
struct SourceDictation {
    static func main() {
        do {
            let arguments = Array(CommandLine.arguments.dropFirst())
            guard arguments.count == 2, arguments[0] == "serve",
                let parentPID = pid_t(arguments[1])
            else {
                throw DictationError.invalidArguments
            }
            let runtime = DictationRuntime(parentPID: parentPID)
            runtime.start()
            writeDictationLine("READY")
            RunLoop.main.run()
        } catch {
            writeDictationLine("ERROR \(error.localizedDescription)")
            exit(1)
        }
    }
}
