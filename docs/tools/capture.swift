// macOS 26+: capture only the specified game window, preserving HDR pixels.
// swiftc -O -module-cache-path /tmp/scramble-swift-cache capture.swift -o /tmp/scramble-capture
// scramble-capture shot WINDOW_ID SAMPLE_COUNT INTERVAL_SECONDS OUTPUT_PREFIX
// scramble-capture video WINDOW_ID DURATION_SECONDS OUTPUT.mp4 [hdr]
import AppKit
import ScreenCaptureKit
import CoreImage
import AVFoundation

struct CaptureError: Error { let message: String }
final class RecordingDelegate: NSObject, SCRecordingOutputDelegate {
    let completion = AsyncStream<Result<Void, Error>>.makeStream()
    func recordingOutput(_ output: SCRecordingOutput, didFailWithError error: Error) {
        completion.continuation.yield(.failure(error))
        completion.continuation.finish()
    }
    func recordingOutputDidFinishRecording(_ output: SCRecordingOutput) {
        completion.continuation.yield(.success(()))
        completion.continuation.finish()
    }
}
_ = NSApplication.shared
let args = CommandLine.arguments
var complete = false
var failed = false
Task {
    do {
        guard args.count >= 5, let id = UInt32(args[2]) else {
            throw CaptureError(message: "Usage: shot WINDOW COUNT INTERVAL PREFIX | video WINDOW SECONDS OUTPUT [hdr]")
        }
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let window = content.windows.first(where: { $0.windowID == id }) else {
            throw CaptureError(message: "Window not found")
        }
        let filter = SCContentFilter(desktopIndependentWindow: window)
        if args[1] == "shot" {
            guard args.count == 6, let count = Int(args[3]), count > 0,
                  let interval = Double(args[4]), interval >= 0, interval.isFinite else {
                throw CaptureError(message: "Invalid screenshot arguments")
            }
            let cfg = SCStreamConfiguration()
            cfg.captureDynamicRange = .hdrLocalDisplay
            cfg.pixelFormat = kCVPixelFormatType_64RGBAHalf
            cfg.colorSpaceName = CGColorSpace.extendedLinearSRGB
            cfg.width = Int(filter.contentRect.width * CGFloat(filter.pointPixelScale))
            cfg.height = Int(filter.contentRect.height * CGFloat(filter.pointPixelScale))
            cfg.showsCursor = false
            cfg.ignoreShadowsSingleWindow = true
            let linear = CGColorSpace(name: CGColorSpace.extendedLinearSRGB)!
            let pq = CGColorSpace(name: CGColorSpace.itur_2100_PQ)!
            let srgb = CGColorSpace(name: CGColorSpace.sRGB)!
            let context = CIContext(options: [.workingColorSpace: linear])
            var best = -1
            for i in 0..<count {
                let sample = try await SCScreenshotManager.captureSampleBuffer(contentFilter: filter, configuration: cfg)
                guard let buffer = CMSampleBufferGetImageBuffer(sample) else {
                    throw CaptureError(message: "No image buffer")
                }
                CVPixelBufferLockBaseAddress(buffer, .readOnly)
                let width = CVPixelBufferGetWidth(buffer), height = CVPixelBufferGetHeight(buffer)
                let stride = CVPixelBufferGetBytesPerRow(buffer)
                let base = CVPixelBufferGetBaseAddress(buffer)!
                var bright = 0
                var peak: Float = 0
                for y in 0..<height {
                    let row = base.advanced(by: y * stride).assumingMemoryBound(to: UInt16.self)
                    for x in 0..<width {
                        let value = (0..<3).map { Float(Float16(bitPattern: row[x * 4 + $0])) }.max()!
                        if value > 1.02 { bright += 1 }
                        peak = max(peak, value)
                    }
                }
                CVPixelBufferUnlockBaseAddress(buffer, .readOnly)
                if bright > best {
                    best = bright
                    let image = CIImage(cvPixelBuffer: buffer, options: [.colorSpace: linear])
                    let prefix = args[5]
                    try context.writeTIFFRepresentation(of: image, to: URL(fileURLWithPath: prefix + "-linear.tiff"), format: .RGBAh, colorSpace: linear)
                    try context.writePNGRepresentation(of: image, to: URL(fileURLWithPath: prefix + "-pq.png"), format: .RGBA16, colorSpace: pq)
                    try context.writePNGRepresentation(of: image, to: URL(fileURLWithPath: prefix + ".png"), format: .RGBA8, colorSpace: srgb)
                    let report = "sample=\(i) width=\(width) height=\(height) above_sdr_white=\(bright) peak_linear_srgb=\(peak)\n"
                    try report.write(toFile: prefix + ".txt", atomically: true, encoding: .utf8)
                    print(report, terminator: "")
                }
                if i + 1 < count { try await Task.sleep(for: .seconds(interval)) }
            }
        } else if args[1] == "video" {
            guard let duration = Double(args[3]), duration > 0, duration.isFinite else {
                throw CaptureError(message: "Invalid duration")
            }
            let hdr = args.count > 5 && args[5] == "hdr"
            let cfg = hdr ? SCStreamConfiguration(preset: .captureHDRRecordingPreservedSDRHDR10) : SCStreamConfiguration()
            cfg.width = Int(filter.contentRect.width)
            cfg.height = Int(filter.contentRect.height)
            // H.264/HEVC 4:2:0 requires even dimensions.
            cfg.width -= cfg.width % 2
            cfg.height -= cfg.height % 2
            cfg.minimumFrameInterval = CMTime(value: 1, timescale: 30)
            cfg.showsCursor = false
            cfg.ignoreShadowsSingleWindow = true
            let recording = SCRecordingOutputConfiguration()
            recording.outputURL = URL(fileURLWithPath: args[4])
            recording.outputFileType = .mp4
            recording.videoCodecType = hdr ? .hevc : .h264
            let delegate = RecordingDelegate()
            let output = SCRecordingOutput(configuration: recording, delegate: delegate)
            let stream = SCStream(filter: filter, configuration: cfg, delegate: nil)
            try stream.addRecordingOutput(output)
            try await stream.startCapture()
            print("Recording \(duration) seconds")
            try await Task.sleep(for: .seconds(duration))
            try await stream.stopCapture()
            for await result in delegate.completion.stream { try result.get() }
            print("Saved \(args[4])")
        } else { throw CaptureError(message: "Unknown mode") }
    } catch {
        FileHandle.standardError.write(Data("\(error)\n".utf8))
        failed = true
    }
    complete = true
}
while !complete { RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1)) }
exit(failed ? 1 : 0)
