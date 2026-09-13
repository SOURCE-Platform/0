import AudioToolbox
import CoreAudio
import Foundation

/// Tell Source when audio input devices come or go.
///
/// Core Audio posts a notification the moment a device is added, removed, or
/// promoted to system default, so nothing has to sit in a loop asking. Source
/// resolves the microphone chosen in Settings against this list and follows it.
final class InputDeviceWatch {
    private let queue = DispatchQueue(label: "com.racker.zero.input-device-watch")
    private var listeners: [(AudioObjectPropertyAddress, AudioObjectPropertyListenerBlock)] = []
    private var pending: DispatchWorkItem?

    private static let watched: [AudioObjectPropertySelector] = [
        kAudioHardwarePropertyDevices,
        kAudioHardwarePropertyDefaultInputDevice,
    ]

    func start() {
        for selector in Self.watched {
            var address = AudioObjectPropertyAddress(
                mSelector: selector,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain
            )
            let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in
                self?.scheduleReport()
            }
            let status = AudioObjectAddPropertyListenerBlock(
                AudioObjectID(kAudioObjectSystemObject), &address, queue, block
            )
            if status == noErr {
                listeners.append((address, block))
            } else {
                writeDictationLine("DEBUG input device listener failed (status \(status))")
            }
        }
        report()
    }

    func stop() {
        for (address, block) in listeners {
            var address = address
            AudioObjectRemovePropertyListenerBlock(
                AudioObjectID(kAudioObjectSystemObject), &address, queue, block
            )
        }
        listeners.removeAll()
    }

    /// One plug-in event fires several Core Audio notifications, and a device
    /// needs a moment to publish its channels, so settle before reporting.
    private func scheduleReport() {
        pending?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.report() }
        pending = work
        queue.asyncAfter(deadline: .now() + 0.4, execute: work)
    }

    private func report() {
        let devices = DictationInput.inputDevices().map { $0.name }
        let payload: [String: Any] = [
            "default": Self.defaultInputName() as Any,
            "devices": devices,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
            let json = String(data: data, encoding: .utf8)
        else { return }
        writeDictationLine("INPUT_DEVICES \(json)")
    }

    private static func defaultInputName() -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultInputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var id = AudioDeviceID(0)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        guard
            AudioObjectGetPropertyData(
                AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &id
            ) == noErr
        else { return nil }
        return DictationInput.inputDevices().first { $0.id == id }?.name
    }
}
