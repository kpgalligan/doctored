import AppKit
import QuartzCore

// The canvas's EDR half (v1.x): its backing layer's extended-range configuration, and the
// draw check the jobs-probe reads. `ImageCanvasView.extendedRangeDisplay` is the one switch (its
// `didSet` calls `applyExtendedRange()`); the editor sets it when an extended-range view image
// lands in `proofImage` (`EditorViewController+HDRPreview.swift`) and clears it when the window
// shows SDR again. Nothing here touches `image`, `previewImage` or any readout.

/// What the canvas's last `draw(_:)` drew into while it was extended-range,
/// recorded only while `recording` is on (the jobs-probe's "hdr preview
/// toggle" case): the backing context's component depth and kind, whether
/// its colour space is extended-range, and the largest RGB sample its bytes
/// hold after the base image was drawn (nil when the context exposes no
/// bytes). On macOS 26.5.1 AppKit records a layer-backed view's draw as a
/// DISPLAY LIST (`kCGContextTypeDisplayList`: depth 0, no colour space, no
/// bytes) that Core Animation rasterizes into the RGBA16Float backing store
/// later, so the probe also renders the layer into a float extended-range
/// bitmap (`CALayer.render(in:)`, which calls `draw(_:)` with that bitmap)
/// and reads what was drawn there.
struct ExtendedDrawRecord {
    let bitsPerComponent: Int
    let floatComponents: Bool
    let extendedRange: Bool
    let drawnPeak: Float?

    /// Off in the app: `noteExtendedDraw` returns at once. Set only by the
    /// jobs-probe case.
    static var recording = false
}

extension ImageCanvasView {
    /// The backing layer's configuration for `extendedRangeDisplay`
    /// (`EDRDisplay.configure`).
    func applyExtendedRange() {
        guard let layer else { return }
        EDRDisplay.configure(layer, extended: extendedRangeDisplay)
        needsDisplay = true
    }

    /// True when the backing layer is EDR-configured (MCP `edr_layer`).
    var extendedRangeLayerConfigured: Bool {
        guard let layer else { return false }
        return layer.wantsExtendedDynamicRangeContent && layer.contentsFormat == .RGBA16Float
    }

    /// Called by `draw(_:)` right after the base image is drawn. Returns at
    /// once unless `ExtendedDrawRecord.recording` is on and the canvas is
    /// extended-range; then records what `context` is (the struct's doc).
    func noteExtendedDraw(_ context: CGContext) {
        guard ExtendedDrawRecord.recording, extendedRangeDisplay else { return }
        let floats = context.bitmapInfo.contains(.floatComponents)
        let extended = context.colorSpace.map { CGColorSpaceUsesExtendedRange($0) } ?? false
        lastExtendedDraw = ExtendedDrawRecord(
            bitsPerComponent: context.bitsPerComponent, floatComponents: floats,
            extendedRange: extended, drawnPeak: Self.largestRGBSample(in: context, floats: floats))
    }

    /// The largest RGB sample `context`'s bytes hold, as a number (an 8-bit
    /// code over 255, a binary16 or binary32 as itself); nil without bytes
    /// or for a layout this does not read.
    private static func largestRGBSample(in context: CGContext, floats: Bool) -> Float? {
        guard let data = context.data, context.width > 0, context.height > 0 else { return nil }
        let bits = context.bitsPerComponent
        guard context.bitsPerPixel == bits * 4, [8, 16, 32].contains(bits) else { return nil }
        let alphaFirst = [.first, .premultipliedFirst, .noneSkipFirst].contains(context.alphaInfo)
        let rgb = alphaFirst ? 1...3 : 0...2
        var peak = -Float.infinity
        for y in 0..<context.height {
            let row = data.advanced(by: y * context.bytesPerRow)
            for x in 0..<context.width {
                for c in rgb {
                    let at = x * 4 + c
                    let value: Float
                    switch bits {
                    case 8: value = Float(row.loadUnaligned(fromByteOffset: at, as: UInt8.self)) / 255
                    case 16 where floats:
                        value = Float(Float16(bitPattern: row.loadUnaligned(fromByteOffset: at * 2, as: UInt16.self)))
                    case 16: value = Float(row.loadUnaligned(fromByteOffset: at * 2, as: UInt16.self)) / 65535
                    default:
                        value = floats ? row.loadUnaligned(fromByteOffset: at * 4, as: Float.self)
                            : Float(row.loadUnaligned(fromByteOffset: at * 4, as: UInt32.self)) / Float(UInt32.max)
                    }
                    if value.isFinite { peak = max(peak, value) }
                }
            }
        }
        return peak.isFinite ? peak : nil
    }
}
