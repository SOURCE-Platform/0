import Foundation

/// Fluid Speech Mini (by FluidVoice, CC BY 4.0) acquisition. Downloads the
/// Core ML build from HuggingFace into the shared FluidAudio model cache
/// and prepares it for `AsrModels.loadLocal(..., version: .v3)`.
///
/// The package is a drop-in parakeet-tdt-0.6b-v3 replacement: same 16 kHz
/// input, 15 s windows, 8192-piece vocabulary, blank id 8192. The only
/// shim it needs is the joint file name — the repo ships Parakeet Ultra's
/// joint as `JointDecision.mlmodelc`, while FluidAudio's v3 family loader
/// expects `JointDecisionv3.mlmodelc` (the name upstream publishes that
/// same network under).
enum MiniModel {
    /// HuggingFace repo id; the on-disk folder uses the repo slug.
    static let repo = "FluidVoice-Labs/fluid-speech-mini-coreml"

    private static let bundles = ["Preprocessor", "Encoder", "Decoder", "JointDecision"]
    private static let bundleFiles = [
        "analytics/coremldata.bin",
        "coremldata.bin",
        "metadata.json",
        "model.mil",
        "weights/weight.bin",
    ]
    private static let vocabulary = "parakeet_vocab.json"

    /// ~/Library/Application Support/FluidAudio/Models/fluid-speech-mini-coreml —
    /// the cache root every FluidAudio app on this machine already shares.
    static func defaultDirectory() -> URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("FluidAudio", isDirectory: true)
            .appendingPathComponent("Models", isDirectory: true)
            .appendingPathComponent("fluid-speech-mini-coreml", isDirectory: true)
    }

    /// Guarantee the complete package on disk: fetch whatever is missing
    /// (idempotent, so an interrupted download resumes on next launch),
    /// rename the joint, then verify the final layout.
    static func ensureAvailable(at directory: URL) async throws {
        for relative in remotePaths where !isComplete(directory, relative) && !isComplete(directory, aliased(relative)) {
            try await download(relative, into: directory)
        }
        try aliasJointIfNeeded(in: directory)
        for relative in remotePaths
        where !isComplete(directory, relative) && !isComplete(directory, aliased(relative)) {
            throw MiniModelError.incomplete(relative)
        }
    }

    private static var remotePaths: [String] {
        bundles.flatMap { bundle in
            bundleFiles.map { "\(bundle).mlmodelc/\($0)" }
        } + [vocabulary]
    }

    /// The joint's local name after aliasing (identity for every other file).
    private static func aliased(_ relative: String) -> String {
        relative.replacingOccurrences(of: "JointDecision.mlmodelc", with: "JointDecisionv3.mlmodelc")
    }

    private static func isComplete(_ directory: URL, _ relative: String) -> Bool {
        let path = directory.appendingPathComponent(relative).path
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path),
            let size = attributes[.size] as? Int
        else {
            return false
        }
        return size > 0
    }

    private static func aliasJointIfNeeded(in directory: URL) throws {
        let manager = FileManager.default
        let source = directory.appendingPathComponent("JointDecision.mlmodelc")
        let alias = directory.appendingPathComponent("JointDecisionv3.mlmodelc")
        guard manager.fileExists(atPath: source.path), !manager.fileExists(atPath: alias.path) else {
            return
        }
        try manager.moveItem(at: source, to: alias)
    }

    private static func download(_ relative: String, into directory: URL) async throws {
        guard let url = URL(string: "https://huggingface.co/\(repo)/resolve/main/\(relative)") else {
            throw MiniModelError.downloadFailed(relative)
        }
        let target = directory.appendingPathComponent(relative)
        try FileManager.default.createDirectory(
            at: target.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        writeDictationLine("DEBUG mini download \(relative)")
        let (temporary, response) = try await URLSession.shared.download(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw MiniModelError.downloadFailed(relative)
        }
        try? FileManager.default.removeItem(at: target)
        try FileManager.default.moveItem(at: temporary, to: target)
    }
}

enum MiniModelError: LocalizedError {
    case incomplete(String)
    case downloadFailed(String)

    var errorDescription: String? {
        switch self {
        case .incomplete(let path):
            return "Fluid Speech Mini file missing after download: \(path)"
        case .downloadFailed(let path):
            return "Fluid Speech Mini download failed: \(path)"
        }
    }
}
