import AVFoundation
import Foundation

/// Live loudness metering for the dictation overlay waveform.
/// Runs on the realtime audio tap thread, so it only reads the buffer it is
/// handed and does no allocation.
extension MicSessionRecorder {
    func reportLevel(buffer: AVAudioPCMBuffer) {
        let now = Date()
        guard now.timeIntervalSince(lastLevelAt) > 0.05 else { return }
        lastLevelAt = now
        let rms: Float
        if let channel = buffer.floatChannelData?[0] {
            rms = rmsOfFloats(channel, count: Int(buffer.frameLength))
        } else if let channel = buffer.int16ChannelData?[0] {
            rms = rmsOfInt16(channel, count: Int(buffer.frameLength))
        } else {
            return
        }
        onLevel?(min(1.0, rms * 6.0))
    }

    func rmsOfFloats(_ channel: UnsafePointer<Float>, count: Int) -> Float {
        guard count > 0 else { return 0 }
        var sum: Float = 0
        for i in 0..<count {
            sum += channel[i] * channel[i]
        }
        return sqrt(sum / Float(count))
    }

    func rmsOfInt16(_ channel: UnsafePointer<Int16>, count: Int) -> Float {
        guard count > 0 else { return 0 }
        var sum: Float = 0
        for i in 0..<count {
            let sample = Float(channel[i]) / 32768.0
            sum += sample * sample
        }
        return sqrt(sum / Float(count))
    }
}
