import Darwin
import Foundation

@main
struct SourceDesktopAudio {
    static func main() {
        do {
            let arguments = Array(CommandLine.arguments.dropFirst())
            guard arguments.count == 3, arguments[0] == "stream",
                  let parentPID = pid_t(arguments[2]) else {
                throw DesktopAudioError.invalidArguments
            }

            let runtime = try CoreAudioDesktopRuntime(
                spoolDirectory: URL(fileURLWithPath: arguments[1]),
                parentPID: parentPID
            )
            runtime.start()
            writeDesktopAudioLine("READY")
            RunLoop.main.run()
        } catch {
            writeDesktopAudioLine("ERROR \(error.localizedDescription)")
            exit(1)
        }
    }
}
