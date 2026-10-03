// The pairing screens (spec §5.3 copy): scan the Mac's code, compare the
// two codes, wait for the Mac, finish; and the camera scanner itself.

import AVFoundation
import SwiftUI

struct PairingView: View {
    @StateObject var flow: PairingFlow

    var body: some View {
        VStack(spacing: 20) {
            switch flow.step {
            case .scan:
                Text("On your Mac: Settings → Security → Add Device. Then scan the code it shows.")
                    .multilineTextAlignment(.center)
                QRScanner { flow.scanned($0) }
                    .frame(height: 320)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
            case .contacting:
                ProgressView("Contacting your Mac…")
            case .compare(let code):
                Text("Does the Source Vault window on your Mac show this code?")
                    .multilineTextAlignment(.center)
                Text(code).font(.system(.largeTitle, design: .monospaced)).bold()
                HStack {
                    Button("Codes don't match", role: .destructive) { flow.codesMatch(false) }
                    Button("They match") { flow.codesMatch(true) }.buttonStyle(.borderedProminent)
                }
            case .waiting:
                ProgressView("On your Mac, click Continue. The Source Vault window shows the code and asks for your master password.")
            case .finishing:
                ProgressView("Bringing your vault to this iPhone…")
            case .done:
                Label("Paired", systemImage: "checkmark.seal")
            case .failed(let message):
                Text(message).multilineTextAlignment(.center)
                Button("Start over") { flow.restart() }
            }
        }
        .padding()
    }
}

/// The camera, reading QR codes only.
struct QRScanner: UIViewRepresentable {
    let onCode: (String) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(onCode: onCode) }

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        let session = context.coordinator.session
        if let camera = AVCaptureDevice.default(for: .video), let input = try? AVCaptureDeviceInput(device: camera), session.canAddInput(input) {
            session.addInput(input)
            let output = AVCaptureMetadataOutput()
            if session.canAddOutput(output) {
                session.addOutput(output)
                output.setMetadataObjectsDelegate(context.coordinator, queue: .main)
                output.metadataObjectTypes = [.qr]
            }
        }
        view.layer.session = session
        view.layer.videoGravity = .resizeAspectFill
        DispatchQueue.global(qos: .userInitiated).async { session.startRunning() }
        return view
    }

    func updateUIView(_ view: PreviewView, context: Context) {}

    static func dismantleUIView(_ view: PreviewView, coordinator: Coordinator) {
        coordinator.session.stopRunning()
    }

    final class Coordinator: NSObject, AVCaptureMetadataOutputObjectsDelegate {
        let session = AVCaptureSession()
        let onCode: (String) -> Void

        init(onCode: @escaping (String) -> Void) { self.onCode = onCode }

        func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput objects: [AVMetadataObject], from connection: AVCaptureConnection) {
            if let code = (objects.first as? AVMetadataMachineReadableCodeObject)?.stringValue {
                onCode(code)
            }
        }
    }

    final class PreviewView: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
        override var layer: AVCaptureVideoPreviewLayer { super.layer as! AVCaptureVideoPreviewLayer }
    }
}
