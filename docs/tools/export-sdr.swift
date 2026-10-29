import AVFoundation
import Foundation
var done = false
Task {
 do {
  let asset = AVURLAsset(url: URL(fileURLWithPath: CommandLine.arguments[1]))
  let composition = try await AVMutableVideoComposition.videoComposition(withPropertiesOf: asset)
  composition.colorPrimaries = AVVideoColorPrimaries_ITU_R_709_2
  composition.colorTransferFunction = AVVideoTransferFunction_ITU_R_709_2
  composition.colorYCbCrMatrix = AVVideoYCbCrMatrix_ITU_R_709_2
  let export = AVAssetExportSession(asset: asset, presetName: AVAssetExportPreset1920x1080)!
  export.videoComposition = composition
  export.shouldOptimizeForNetworkUse = true
  try await export.export(to: URL(fileURLWithPath: CommandLine.arguments[2]), as: .mp4)
  print("Exported SDR fallback")
 } catch { print(error); exit(1) }
 done = true
}
while !done { RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1)) }
