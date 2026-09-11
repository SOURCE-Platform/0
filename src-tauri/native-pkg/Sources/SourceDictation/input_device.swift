import AVFoundation
import AudioToolbox
import CoreAudio
import Foundation

/// The microphone chosen in Source, resolved to a Core Audio device.
///
/// Right Option dictation used to record from whatever macOS considered the
/// default input, ignoring the microphone picked in Source.
enum DictationInput {
    struct Device {
        let id: AudioDeviceID
        let name: String
    }

    /// Name of the microphone chosen in Source, or nil to follow the macOS default.
    /// Read fresh for every session, so a new choice applies to the next dictation.
    static func chosenName() -> String? {
        if let override = ProcessInfo.processInfo.environment["SOURCE_DICTATION_INPUT"],
            !override.isEmpty
        {
            return override
        }
        let settings = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".observer_data/config/settings.json")
        guard let data = try? Data(contentsOf: settings),
            let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let selected = json["selected_audio_input_id"] as? String
        else { return nil }
        let prefix = "microphone-name:"
        guard selected.hasPrefix(prefix) else { return nil }
        let name = String(selected.dropFirst(prefix.count))
        return name.isEmpty ? nil : name
    }

    /// Point `engine`'s input at the chosen microphone before its formats are read.
    /// Returns a description of what will be recorded, for the log.
    static func apply(to engine: AVAudioEngine) -> String {
        guard let chosen = chosenName() else { return "macOS default input" }
        guard let device = find(named: chosen) else {
            return "chosen mic \"\(chosen)\" is not connected; using macOS default input"
        }
        guard let unit = engine.inputNode.audioUnit else {
            return "macOS default input (input has no audio unit)"
        }
        // Already on it, as when the chosen mic is the macOS default. Selecting
        // it anyway would only post a configuration change.
        if currentDevice(of: unit) == device.id {
            return device.name
        }
        var status = select(device.id, on: unit)
        if status != noErr {
            // Core Audio refused once right after a burst of reconnects
            // (kAudioHardwareIllegalOperationError). One retry costs little.
            usleep(150_000)
            status = select(device.id, on: unit)
        }
        return status == noErr
            ? device.name
            : "could not select \"\(device.name)\" (status \(status)); using macOS default input"
    }

    private static func select(_ id: AudioDeviceID, on unit: AudioUnit) -> OSStatus {
        var id = id
        return AudioUnitSetProperty(
            unit,
            kAudioOutputUnitProperty_CurrentDevice,
            kAudioUnitScope_Global,
            0,
            &id,
            UInt32(MemoryLayout<AudioDeviceID>.size)
        )
    }

    private static func currentDevice(of unit: AudioUnit) -> AudioDeviceID? {
        var id = AudioDeviceID(0)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        let status = AudioUnitGetProperty(
            unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, &id, &size
        )
        return status == noErr ? id : nil
    }

    static func find(named name: String) -> Device? {
        let target = normalized(name)
        return inputDevices().first { normalized($0.name) == target }
    }

    /// Match loosely, as Source does: different macOS APIs spell the same device
    /// differently, for example with a curly or a straight apostrophe.
    static func normalized(_ name: String) -> String {
        name.lowercased().filter { $0.isLetter || $0.isNumber }
    }

    /// Every connected device with at least one input channel.
    static func inputDevices() -> [Device] {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        let system = AudioObjectID(kAudioObjectSystemObject)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(system, &address, 0, nil, &size) == noErr else { return [] }
        var ids = [AudioDeviceID](repeating: 0, count: Int(size) / MemoryLayout<AudioDeviceID>.size)
        guard AudioObjectGetPropertyData(system, &address, 0, nil, &size, &ids) == noErr else { return [] }
        return ids.compactMap { id in
            guard inputChannelCount(id) > 0, let name = deviceName(id) else { return nil }
            return Device(id: id, name: name)
        }
    }

    private static func inputChannelCount(_ id: AudioDeviceID) -> Int {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: kAudioObjectPropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain
        )
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(id, &address, 0, nil, &size) == noErr, size > 0 else {
            return 0
        }
        let raw = UnsafeMutableRawPointer.allocate(
            byteCount: Int(size),
            alignment: MemoryLayout<AudioBufferList>.alignment
        )
        defer { raw.deallocate() }
        guard AudioObjectGetPropertyData(id, &address, 0, nil, &size, raw) == noErr else { return 0 }
        let buffers = UnsafeMutableAudioBufferListPointer(raw.assumingMemoryBound(to: AudioBufferList.self))
        return buffers.reduce(0) { $0 + Int($1.mNumberChannels) }
    }

    private static func deviceName(_ id: AudioDeviceID) -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioObjectPropertyName,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var name: Unmanaged<CFString>?
        var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
        guard AudioObjectGetPropertyData(id, &address, 0, nil, &size, &name) == noErr,
            let value = name?.takeRetainedValue()
        else { return nil }
        return value as String
    }
}
