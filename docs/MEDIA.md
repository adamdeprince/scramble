# Gameplay captures

The [HTML guide](../index.html#watch) includes a roughly 40-second recording
of the actual game running in Silkie, plus HDR window captures. The built-in
`--autoplay` player drives the same simulation as keyboard play. No artwork
or highlights were painted into the captures.

The stills were captured on macOS with ScreenCaptureKit, Silkie's
`hdr=always`, and seed 42. Morning Meadow supplies the delivery screenshot
and its close-up. Lily Pond supplies the level-clear celebration. The fence
image is a crop of Dusk gameplay: a keyboard-placed hurdle, gathering hearts,
and delivery sparkles are visible together.

## Formats and checks

- HDR stills: 12-bit RGB AVIF, BT.2020 primaries, ST 2084 (PQ) transfer,
  identity matrix, full range (CICP `9/16/0`). The source was extended linear
  sRGB RGBA half-float, never an 8-bit screenshot. Core Image converts the
  captured pixels to BT.2020 PQ before AVIF encoding; this is a colour
  conversion, not just a metadata tag. AV1 quantization is disabled; the
  16-bit PQ intermediate is reduced to 12-bit precision.
- Captured highlights peak at approximately **3.002 times SDR white**.
  The exported AVIF files are decoded back into extended linear sRGB to
  check that values above SDR white survive encoding. The delivery and
  celebration captures contain 138 and 510 such pixels respectively;
  the uncropped Dusk capture contains 174. Counts here refer to the
  source captures, before encoding and colour conversion.
- HDR video: HEVC Main 10, BT.2020/PQ, 1200 × 792, approximately 30 fps,
  39.73 seconds. The file includes HDR mastering/light-level metadata
  supplied by ScreenCaptureKit.
- Standard video: H.264, BT.709, matching dimensions and duration. This is
  an SDR conversion of the same recording. Both versions are silent.
- PNG stills are SDR fallbacks of the same captured frames. They cannot
  reproduce the HDR brightness. The older title and hawk images used only
  in the README remain SDR; the HTML guide uses the new HDR captures.

The HTML uses AVIF sources with PNG fallbacks. Its video selects HEVC on
HDR displays when the browser supports the codec, with H.264 as the default
fallback. Native controls, inline playback, metadata-only preloading, and
explicit download links are provided. Nothing autoplays on page load.
HDR appearance depends on the browser, display, available headroom, and OS
settings; an SDR screenshot of the page cannot validate perceived HDR
brightness.

## Validation

All four HDR AVIF assets decode successfully in Chrome. Decoding them back
into linear sRGB measures peaks of 2.998–3.002 times SDR white. Both the HEVC
HDR recording and H.264 fallback play in Chrome without media errors; their
colour metadata was also inspected through AVFoundation. The page was
checked at desktop and 390-pixel mobile widths, with no horizontal page
overflow, and all local links and media references resolve. These checks
verify encoding and playback, not the brightness perceived on every display.

## Capture tools

These helpers require macOS 26 or newer, permission to capture the screen,
Swift, and `avifenc`/`avifdec` from libavif. ScreenCaptureKit captures only the
specified window. Temporary float and PQ intermediates can be kept outside
the repository. See Apple's [HDR capture presentation](https://developer.apple.com/videos/play/wwdc2024/10088/)
for the capture API background.

Launch a dedicated Silkie instance with `hdr=always`, run
`target/release/silkie-scramble --seed 42 --autoplay`, and press Enter.
Use its kitty remote-control `ls` response to get `platform_window_id`.
For Lily Pond, add `--level 4`. The sample count and interval are explicit
capture choices, not a storage limit; only the brightest frame is retained
as samples arrive.

```sh
swiftc -O -module-cache-path /tmp/scramble-swift-cache \
  docs/tools/capture.swift -o /tmp/scramble-capture

# Replace WINDOW_ID with the dedicated game window ID.
/tmp/scramble-capture shot WINDOW_ID 100 0.12 /tmp/brood
/tmp/scramble-capture video WINDOW_ID 40 /tmp/gameplay-hdr.mp4 hdr

# Optional crop: x, y, width, height; origin is at the bottom left.
swift docs/tools/convert-still.swift /tmp/brood-linear.tiff /tmp/glints \
  110 580 500 430

avifenc --lossless --depth 12 --yuv 444 --cicp 9/16/0 \
  --ignore-icc --ignore-exif --ignore-xmp \
  /tmp/brood-pq.png /tmp/brood-hdr.avif
swift docs/tools/inspect-hdr.swift /tmp/brood-hdr.avif
swift docs/tools/export-sdr.swift /tmp/gameplay-hdr.mp4 /tmp/gameplay.mp4
```

`capture.swift` saves a linear half-float TIFF, a colour-converted 16-bit PQ
PNG, an SDR PNG, and a text report for the selected frame. The HDR inspector
fails if no decoded pixel exceeds SDR white. Cropping uses the float source;
never use the SDR fallback as the source for an HDR export.
