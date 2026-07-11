import CoreMedia
import Darwin
import Foundation
@preconcurrency import ScreenCaptureKit

private let sampleRate = 16_000

private final class DesktopAudioCollector: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
    private let retainSamples: Bool
    private let gain: Float
    private let lock = NSLock()
    private var samples: [Float] = []
    private var stopped = false
    private var stopContinuation: CheckedContinuation<String, Never>?

    init(retainSamples: Bool, gainDb: Double) {
        self.retainSamples = retainSamples
        self.gain = pow(10, Float(min(max(gainDb, 0), 24)) / 20)
    }

    func stream(
        _ stream: SCStream,
        didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
        of outputType: SCStreamOutputType
    ) {
        guard outputType == .audio else { return }
        let values = decodeSamples(sampleBuffer).map { sample in
            min(max(sample * gain, -1), 1)
        }
        guard !values.isEmpty else { return }

        let level = rms(values)
        writeLine("LEVEL \(level)")
        guard retainSamples else { return }

        lock.lock()
        samples.append(contentsOf: values)
        lock.unlock()
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        let message = error.localizedDescription
        lock.lock()
        stopped = true
        let continuation = stopContinuation
        stopContinuation = nil
        lock.unlock()
        continuation?.resume(returning: message)
    }

    func retainedSamples() -> [Float] {
        lock.lock()
        defer { lock.unlock() }
        return samples
    }

    func waitForStop() async -> String {
        await withCheckedContinuation { continuation in
            lock.lock()
            if stopped {
                lock.unlock()
                continuation.resume(returning: "The desktop audio stream stopped.")
            } else {
                stopContinuation = continuation
                lock.unlock()
            }
        }
    }
}

private func decodeSamples(_ sampleBuffer: CMSampleBuffer) -> [Float] {
    guard let description = CMSampleBufferGetFormatDescription(sampleBuffer),
          let format = CMAudioFormatDescriptionGetStreamBasicDescription(description)
    else { return [] }

    let channels = max(Int(format.pointee.mChannelsPerFrame), 1)
    let bitsPerChannel = Int(format.pointee.mBitsPerChannel)
    let isFloat = format.pointee.mFormatFlags & kAudioFormatFlagIsFloat != 0
    let isSigned = format.pointee.mFormatFlags & kAudioFormatFlagIsSignedInteger != 0

    var buffer = AudioBufferList(
        mNumberBuffers: 1,
        mBuffers: AudioBuffer(mNumberChannels: 1, mDataByteSize: 0, mData: nil)
    )
    var retainedBlock: CMBlockBuffer?
    let status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
        sampleBuffer,
        bufferListSizeNeededOut: nil,
        bufferListOut: &buffer,
        bufferListSize: MemoryLayout<AudioBufferList>.size,
        blockBufferAllocator: kCFAllocatorDefault,
        blockBufferMemoryAllocator: kCFAllocatorDefault,
        flags: kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
        blockBufferOut: &retainedBlock
    )
    guard status == noErr else { return [] }

    let audioBuffers = UnsafeMutableAudioBufferListPointer(&buffer)
    var decoded: [Float] = []
    for audioBuffer in audioBuffers {
        guard let data = audioBuffer.mData else { continue }
        if isFloat && bitsPerChannel == 32 {
            let values = data.assumingMemoryBound(to: Float.self)
            let count = Int(audioBuffer.mDataByteSize) / MemoryLayout<Float>.size
            decoded.append(contentsOf: UnsafeBufferPointer(start: values, count: count))
        } else if isSigned && bitsPerChannel == 16 {
            let values = data.assumingMemoryBound(to: Int16.self)
            let count = Int(audioBuffer.mDataByteSize) / MemoryLayout<Int16>.size
            decoded.append(contentsOf: UnsafeBufferPointer(start: values, count: count).map {
                Float($0) / Float(Int16.max)
            })
        }
    }

    guard channels > 1, !decoded.isEmpty else { return decoded }
    return stride(from: 0, to: decoded.count, by: channels).map { start in
        let end = min(start + channels, decoded.count)
        return decoded[start..<end].reduce(0, +) / Float(end - start)
    }
}

private func rms(_ values: [Float]) -> Float {
    let sum = values.reduce(0) { $0 + $1 * $1 }
    return min(max(sqrt(sum / Float(values.count)), 0), 1)
}

private func writeLine(_ value: String) {
    let line = "\(value)\n"
    FileHandle.standardOutput.write(line.data(using: .utf8)!)
}

private func writeRawFloat(_ samples: [Float], to path: String) throws {
    var output = Data(capacity: samples.count * MemoryLayout<Float>.size)
    for sample in samples {
        var littleEndian = sample.bitPattern.littleEndian
        withUnsafeBytes(of: &littleEndian) { output.append(contentsOf: $0) }
    }
    try output.write(to: URL(fileURLWithPath: path), options: .atomic)
}

private func makeStream(_ collector: DesktopAudioCollector) async throws -> SCStream {
    let content = try await SCShareableContent.excludingDesktopWindows(
        false,
        onScreenWindowsOnly: false
    )
    guard let display = content.displays.first else {
        throw NSError(domain: "SOURCE", code: 1, userInfo: [
            NSLocalizedDescriptionKey: "No display is available for desktop audio capture."
        ])
    }

    let filter = SCContentFilter(
        display: display,
        excludingApplications: [],
        exceptingWindows: []
    )
    let configuration = SCStreamConfiguration()
    configuration.width = 2
    configuration.height = 2
    configuration.capturesAudio = true
    configuration.sampleRate = sampleRate
    configuration.channelCount = 1
    configuration.excludesCurrentProcessAudio = true

    let stream = SCStream(filter: filter, configuration: configuration, delegate: collector)
    try stream.addStreamOutput(
        collector,
        type: .audio,
        sampleHandlerQueue: DispatchQueue(label: "com.source.desktop-audio")
    )
    return stream
}

private func runMeter(gainDb: Double, parentPid: pid_t) async throws {
    let watchdog = Task.detached {
        while getppid() == parentPid {
            try? await Task.sleep(for: .seconds(1))
        }
        exit(0)
    }
    defer { watchdog.cancel() }
    var hasStarted = false
    while true {
        let collector = DesktopAudioCollector(retainSamples: false, gainDb: gainDb)
        do {
            let stream = try await makeStream(collector)
            try await stream.startCapture()
            if hasStarted {
                writeLine("RECONNECTED")
            } else {
                writeLine("READY")
                hasStarted = true
            }
            let reason = await collector.waitForStop()
            writeLine("RECONNECTING \(reason)")
        } catch {
            guard hasStarted else { throw error }
            writeLine("RECONNECTING \(error.localizedDescription)")
        }
        try await Task.sleep(for: .seconds(1))
    }
}

private func runCapture(outputPath: String, duration: Double, gainDb: Double) async throws {
    let collector = DesktopAudioCollector(retainSamples: true, gainDb: gainDb)
    let stream = try await makeStream(collector)
    try await stream.startCapture()
    try await Task.sleep(for: .seconds(max(duration, 0.2)))
    try await stream.stopCapture()
    try writeRawFloat(collector.retainedSamples(), to: outputPath)
    writeLine("COMPLETE")
}

@main
struct SourceDesktopAudio {
    static func main() async {
        do {
            let arguments = Array(CommandLine.arguments.dropFirst())
            guard let mode = arguments.first else {
                throw NSError(domain: "SOURCE", code: 2, userInfo: [
                    NSLocalizedDescriptionKey: "Missing desktop audio mode."
                ])
            }
            if mode == "meter", arguments.count == 3,
                      let gainDb = Double(arguments[1]),
                      let parentPid = pid_t(arguments[2]) {
                try await runMeter(gainDb: gainDb, parentPid: parentPid)
            } else if mode == "capture", arguments.count == 4,
                      let duration = Double(arguments[2]),
                      let gainDb = Double(arguments[3]) {
                try await runCapture(outputPath: arguments[1], duration: duration, gainDb: gainDb)
            } else {
                throw NSError(domain: "SOURCE", code: 3, userInfo: [
                    NSLocalizedDescriptionKey: "Invalid desktop audio arguments."
                ])
            }
        } catch {
            writeLine("ERROR \(error.localizedDescription)")
            exit(1)
        }
    }
}
