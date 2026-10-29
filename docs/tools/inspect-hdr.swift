// Decode an image into extended linear sRGB and report actual HDR pixels.
// swift inspect-hdr.swift IMAGE
import CoreImage
import Foundation
let context = CIContext(options: [.workingColorSpace: CGColorSpace(name: CGColorSpace.extendedLinearSRGB)!])
guard let image = CIImage(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])) else { fatalError("Cannot decode image") }
let width = Int(image.extent.width), height = Int(image.extent.height)
let (rowBytes, overflow1) = width.multipliedReportingOverflow(by: 8)
let (byteCount, overflow2) = rowBytes.multipliedReportingOverflow(by: height)
guard !overflow1 && !overflow2 else { fatalError("Image dimensions overflow") }
guard let storage = malloc(byteCount) else { fatalError("Cannot allocate image buffer") }
defer { free(storage) }
context.render(image, toBitmap: storage, rowBytes: rowBytes, bounds: image.extent, format: .RGBAh, colorSpace: CGColorSpace(name: CGColorSpace.extendedLinearSRGB)!)
let pixels = storage.assumingMemoryBound(to: UInt16.self)
var peak: Float = 0
var bright = 0
for i in 0..<(byteCount / 8) {
    let value = max(Float(Float16(bitPattern: pixels[i*4])), Float(Float16(bitPattern: pixels[i*4+1])), Float(Float16(bitPattern: pixels[i*4+2])))
    peak = max(peak, value)
    if value > 1.02 { bright += 1 }
}
print("\(CommandLine.arguments[1]): \(width)x\(height), above SDR white: \(bright), peak linear sRGB: \(peak)")
if bright == 0 { exit(1) }
