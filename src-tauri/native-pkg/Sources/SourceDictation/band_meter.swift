import Accelerate
import AVFoundation
import Foundation

/// FFT spectrum metering for the pill's graphic equalizer: each bar is a
/// real frequency range of the live mic signal. Every buffer preallocates
/// at init, so `bands(for:)` does no allocation on the realtime audio tap
/// thread. Owned by MicSessionRecorder and only touched on the tap thread.
final class BandMeter {
    static let bandCount = 6
    /// Band edges in Hz — speech-shaped, roughly log-spaced.
    private static let edges: [Double] = [80, 250, 500, 1000, 2000, 4000, 8000]
    /// Below this bin power (dB) a band counts as silence and decays to 0,
    /// so room tone never reads as speech.
    private static let silenceFloorDB: Float = -55

    private let order = vDSP_Length(10)
    private let size = 1024
    private let setup: FFTSetup?
    private var window: [Float]
    private var samples: [Float]
    private var real: [Float]
    private var imag: [Float]
    private var mags: [Float]
    /// Per-band adaptive peak: instant attack, slow decay (~0.995 per push,
    /// half-life ~7 s at 20 Hz), so the meter self-calibrates to any mic
    /// gain instead of needing tuned absolute constants.
    private var peaks = [Float](repeating: 0, count: bandCount)
    /// Display smoothing: fast attack, slow release, for pleasing motion.
    private var smoothed = [Float](repeating: 0, count: bandCount)
    /// Reused output buffer. Callers that hand the bands to another thread
    /// must value-copy first (Swift array copy-on-write makes `let copy =
    /// bands` sufficient).
    private var bandOut = [Float](repeating: 0, count: bandCount)

    init() {
        setup = vDSP_create_fftsetup(order, FFTRadix(kFFTRadix2))
        var window = [Float](repeating: 0, count: size)
        vDSP_hann_window(&window, vDSP_Length(size), Int32(vDSP_HANN_NORM))
        self.window = window
        samples = [Float](repeating: 0, count: size)
        real = [Float](repeating: 0, count: size / 2)
        imag = [Float](repeating: 0, count: size / 2)
        mags = [Float](repeating: 0, count: size / 2)
    }

    deinit {
        if let setup {
            vDSP_destroy_fftsetup(setup)
        }
    }

    /// Latest band levels (0..1), or nil for a channel layout the meter
    /// cannot read. Newest samples land at the end of the window; short
    /// buffers zero-pad at the front.
    func bands(for buffer: AVAudioPCMBuffer) -> [Float]? {
        guard let setup else { return nil }
        let frames = Int(buffer.frameLength)
        guard frames > 0 else { return nil }
        let copied = min(frames, size)
        let pad = size - copied
        if let channel = buffer.floatChannelData?[0] {
            samples.withUnsafeMutableBufferPointer { dst in
                if pad > 0 {
                    dst.baseAddress!.update(repeating: 0, count: pad)
                }
                dst.baseAddress!.advanced(by: pad).update(
                    from: channel.advanced(by: frames - copied),
                    count: copied
                )
            }
        } else if let channel = buffer.int16ChannelData?[0] {
            samples.withUnsafeMutableBufferPointer { dst in
                if pad > 0 {
                    dst.baseAddress!.update(repeating: 0, count: pad)
                }
                let source = channel.advanced(by: frames - copied)
                for index in 0..<copied {
                    dst[pad + index] = Float(source[index]) / 32768.0
                }
            }
        } else {
            return nil
        }

        vDSP_vmul(samples, 1, window, 1, &samples, 1, vDSP_Length(size))
        samples.withUnsafeBufferPointer { pointer in
            pointer.baseAddress!.withMemoryRebound(to: DSPComplex.self, capacity: size / 2) { complex in
                real.withUnsafeMutableBufferPointer { realp in
                    imag.withUnsafeMutableBufferPointer { imagp in
                        var split = DSPSplitComplex(realp: realp.baseAddress!, imagp: imagp.baseAddress!)
                        vDSP_ctoz(complex, 2, &split, 1, vDSP_Length(size / 2))
                        vDSP_fft_zrip(setup, &split, 1, order, FFTDirection(kFFTDirection_Forward))
                        vDSP_zvmags(&split, 1, &mags, 1, vDSP_Length(size / 2))
                    }
                }
            }
        }

        let hzPerBin = buffer.format.sampleRate / Double(size)
        for band in 0..<Self.bandCount {
            let low = max(1, Int(Self.edges[band] / hzPerBin))
            let high = min(Int(Self.edges[band + 1] / hzPerBin), size / 2 - 1)
            var peak: Float = 0
            if high > low {
                mags.withUnsafeBufferPointer { pointer in
                    vDSP_maxv(pointer.baseAddress! + low, 1, &peak, vDSP_Length(high - low))
                }
            }
            let db = 10 * log10(max(peak, 1e-12))
            let target: Float
            if db < Self.silenceFloorDB {
                target = 0
            } else {
                peaks[band] = max(peak, peaks[band] * 0.995, 1e-9)
                // Square root compresses the range perceptually, the way
                // loudness actually lands on the ear.
                target = min(1, sqrt(peak / peaks[band]))
            }
            smoothed[band] = max(target, smoothed[band] * 0.72)
            bandOut[band] = smoothed[band]
        }
        return bandOut
    }
}
