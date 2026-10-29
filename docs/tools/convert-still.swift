// Convert a captured linear half-float TIFF to PQ PNG and an SDR fallback.
// Optional crop coordinates use Core Image's bottom-left origin.
// swift convert-still.swift INPUT.tiff OUTPUT_PREFIX [X Y WIDTH HEIGHT]
import Foundation
import CoreImage
import ImageIO
let args = CommandLine.arguments
let linear = CGColorSpace(name: CGColorSpace.extendedLinearSRGB)!
let context = CIContext(options: [.workingColorSpace: linear])
guard args.count == 3 || args.count == 7,
      var image = CIImage(contentsOf: URL(fileURLWithPath: args[1])) else {
    fatalError("Usage: INPUT.tiff OUTPUT_PREFIX [X Y WIDTH HEIGHT]")
}
if args.count == 7 {
    let values = args[3...6].compactMap(Double.init)
    guard values.count == 4, values.allSatisfy({ $0.isFinite }), values[2] > 0, values[3] > 0 else {
        fatalError("Invalid crop")
    }
    let crop = CGRect(x: values[0], y: values[1], width: values[2], height: values[3])
    guard image.extent.contains(crop) else { fatalError("Crop outside image") }
    image = image.cropped(to: crop).transformed(by: CGAffineTransform(translationX: -crop.minX, y: -crop.minY))
}
try context.writePNGRepresentation(of: image, to: URL(fileURLWithPath: args[2] + "-pq.png"), format: .RGBA16, colorSpace: CGColorSpace(name: CGColorSpace.itur_2100_PQ)!)
try context.writePNGRepresentation(of: image, to: URL(fileURLWithPath: args[2] + ".png"), format: .RGBA8, colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!)
