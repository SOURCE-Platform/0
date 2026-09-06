import CoreAudio
import Darwin
import Foundation

private let levelInterval: TimeInterval = 1.0 / 30.0
private let chunkInterval: TimeInterval = 1.0

enum DesktopAudioError: LocalizedError {
    case unavailable
    case invalidArguments
    case osStatus(String, OSStatus)

    var errorDescription: String? {
        switch self {
        case .unavailable:
            return "Core Audio Process Taps require macOS 14.2 or later."
        case .invalidArguments:
            return "Invalid desktop audio helper arguments."
        case let .osStatus(operation, status):
            return "\(operation) failed with Core Audio status \(status)."
        }
    }
}

final class CoreAudioDesktopRuntime: @unchecked Sendable {
    private let collector = DesktopSampleCollector()
    private let spoolDirectory: URL
    private let parentPID: pid_t
    private var tapID = AudioObjectID(kAudioObjectUnknown)
    private var deviceID = AudioObjectID(kAudioObjectUnknown)
    private var ioProcID: AudioDeviceIOProcID?
    private var levelTimer: DispatchSourceTimer?
    private var chunkTimer: DispatchSourceTimer?

    init(spoolDirectory: URL, parentPID: pid_t) throws {
        guard #available(macOS 14.2, *) else {
            throw DesktopAudioError.unavailable
        }
        self.spoolDirectory = spoolDirectory
        self.parentPID = parentPID
        try FileManager.default.createDirectory(
            at: spoolDirectory,
            withIntermediateDirectories: true
        )
        try createTapAndAggregateDevice()
        watchInput()
    }

    deinit {
        stop()
    }

    func start() {
        guard let ioProcID else { return }
        _ = AudioDeviceStart(deviceID, ioProcID)
        startLevelTimer()
        startChunkTimer()
        startParentWatchdog()
    }

    func stop() {
        levelTimer?.cancel()
        chunkTimer?.cancel()
        levelTimer = nil
        chunkTimer = nil
        if let ioProcID {
            _ = AudioDeviceStop(deviceID, ioProcID)
            _ = AudioDeviceDestroyIOProcID(deviceID, ioProcID)
            self.ioProcID = nil
        }
        if deviceID != kAudioObjectUnknown {
            _ = AudioHardwareDestroyAggregateDevice(deviceID)
            deviceID = kAudioObjectUnknown
        }
        if tapID != kAudioObjectUnknown {
            if #available(macOS 14.2, *) {
                _ = AudioHardwareDestroyProcessTap(tapID)
            }
            tapID = kAudioObjectUnknown
        }
    }

    private func createTapAndAggregateDevice() throws {
        guard #available(macOS 14.2, *) else {
            throw DesktopAudioError.unavailable
        }

        let description = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
        description.muteBehavior = .unmuted
        try check(
            AudioHardwareCreateProcessTap(description, &tapID),
            operation: "Creating desktop audio tap"
        )

        let aggregateUID = "com.source.desktop-audio.\(UUID().uuidString)"
        let aggregate: [String: Any] = [
            kAudioAggregateDeviceNameKey: "SOURCE Desktop Audio",
            kAudioAggregateDeviceUIDKey: aggregateUID,
            kAudioAggregateDeviceIsPrivateKey: 1,
            kAudioAggregateDeviceTapListKey: [[
                kAudioSubTapUIDKey: description.uuid.uuidString,
                kAudioSubTapDriftCompensationKey: 1,
            ]],
        ]
        try check(
            AudioHardwareCreateAggregateDevice(aggregate as CFDictionary, &deviceID),
            operation: "Creating desktop audio device"
        )
        try check(
            AudioDeviceCreateIOProcID(
                deviceID,
                desktopAudioCallback,
                Unmanaged.passUnretained(collector).toOpaque(),
                &ioProcID
            ),
            operation: "Registering desktop audio callback"
        )
    }

    private func watchInput() {
        FileHandle.standardInput.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty,
                  let command = String(data: data, encoding: .utf8) else { return }
            for line in command.split(whereSeparator: \ .isNewline) {
                self?.collector.setCaptureEnabled(line == "CAPTURE_START")
            }
        }
    }

    private func startLevelTimer() {
        let timer = DispatchSource.makeTimerSource(queue: .global(qos: .userInteractive))
        timer.schedule(deadline: .now(), repeating: levelInterval)
        timer.setEventHandler { [weak self] in
            guard let self else { return }
            writeDesktopAudioLine("LEVEL \(self.collector.latestLevel())")
        }
        levelTimer = timer
        timer.resume()
    }

    private func startChunkTimer() {
        let timer = DispatchSource.makeTimerSource(queue: .global(qos: .utility))
        timer.schedule(deadline: .now() + chunkInterval, repeating: chunkInterval)
        timer.setEventHandler { [weak self] in
            self?.emitChunk()
        }
        chunkTimer = timer
        timer.resume()
    }

    private func emitChunk() {
        let samples = collector.drainCapturedSamples()
        guard !samples.isEmpty else { return }
        let path = spoolDirectory.appendingPathComponent("\(UUID().uuidString).f32")
        do {
            try writeRawFloat(samples, to: path)
            writeDesktopAudioLine("CHUNK \(path.path)")
        } catch {
            writeDesktopAudioLine("ERROR \(error.localizedDescription)")
        }
    }

    private func startParentWatchdog() {
        DispatchQueue.global(qos: .utility).async { [parentPID] in
            while getppid() == parentPID {
                sleep(1)
            }
            exit(0)
        }
    }

    private func check(_ status: OSStatus, operation: String) throws {
        guard status == noErr else {
            throw DesktopAudioError.osStatus(operation, status)
        }
    }
}

private final class DesktopSampleCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var captureEnabled = false
    private var samples: [Float] = []
    private var level: Float = 0

    func append(_ input: UnsafePointer<AudioBufferList>) {
        let buffers = UnsafeMutableAudioBufferListPointer(
            UnsafeMutablePointer(mutating: input)
        )
        let decoded = buffers.flatMap(decodeBuffer)
        guard !decoded.isEmpty else { return }

        lock.lock()
        level = rms(decoded)
        if captureEnabled {
            samples.append(contentsOf: decoded)
        }
        lock.unlock()
    }

    func latestLevel() -> Float {
        lock.lock()
        defer { lock.unlock() }
        return level
    }

    func setCaptureEnabled(_ enabled: Bool) {
        lock.lock()
        captureEnabled = enabled
        if !enabled {
            samples.removeAll(keepingCapacity: true)
        }
        lock.unlock()
    }

    func drainCapturedSamples() -> [Float] {
        lock.lock()
        defer { lock.unlock() }
        guard captureEnabled else { return [] }
        let drained = samples
        samples.removeAll(keepingCapacity: true)
        return drained
    }
}

private func desktopAudioCallback(
    _ inDevice: AudioObjectID,
    _ inNow: UnsafePointer<AudioTimeStamp>,
    _ inInputData: UnsafePointer<AudioBufferList>,
    _ inInputTime: UnsafePointer<AudioTimeStamp>,
    _ outOutputData: UnsafeMutablePointer<AudioBufferList>,
    _ inOutputTime: UnsafePointer<AudioTimeStamp>,
    _ inClientData: UnsafeMutableRawPointer?
) -> OSStatus {
    guard let inClientData else { return noErr }
    let collector = Unmanaged<DesktopSampleCollector>
        .fromOpaque(inClientData)
        .takeUnretainedValue()
    collector.append(inInputData)
    return noErr
}

private func decodeBuffer(_ buffer: AudioBuffer) -> [Float] {
    guard let data = buffer.mData else { return [] }
    let count = Int(buffer.mDataByteSize) / MemoryLayout<Float>.size
    guard count > 0 else { return [] }
    let values = data.assumingMemoryBound(to: Float.self)
    let channels = max(Int(buffer.mNumberChannels), 1)
    let frames = UnsafeBufferPointer(start: values, count: count)
    guard channels > 1 else { return Array(frames) }

    return stride(from: 0, to: count, by: channels).map { start in
        let end = min(start + channels, count)
        return frames[start..<end].reduce(0, +) / Float(end - start)
    }
}

private func rms(_ values: [Float]) -> Float {
    let sum = values.reduce(0) { $0 + $1 * $1 }
    return min(max(sqrt(sum / Float(values.count)), 0), 1)
}

private func writeRawFloat(_ samples: [Float], to path: URL) throws {
    var output = Data(capacity: samples.count * MemoryLayout<Float>.size)
    for sample in samples {
        var littleEndian = sample.bitPattern.littleEndian
        withUnsafeBytes(of: &littleEndian) { output.append(contentsOf: $0) }
    }
    try output.write(to: path, options: .atomic)
}

func writeDesktopAudioLine(_ value: String) {
    let line = "\(value)\n"
    FileHandle.standardOutput.write(line.data(using: .utf8)!)
}
