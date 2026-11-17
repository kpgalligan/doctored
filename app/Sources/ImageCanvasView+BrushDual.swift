import AppKit

/// Brush dynamics' dual brush on the canvas (`designs/brush-dynamics.md`, `BrushDual.swift`). A
/// dual stroke's coverage is the product of the two tips' accumulated fields, so no one dab can
/// be drawn on its own: after each nominal stamp (an airbrush tick included, whose first-tip dabs
/// change the first field), every box a dab laid by that stamp covers — first-tip and second-tip
/// alike, each dab's clipped integer box (`brush_coverage::bounds`) — is re-rasterized by the
/// CORE from the stroke so far (`dr_brush_overlay_rgba8_dual` over the box: the dabs whose boxes
/// meet it, in order, shifted by the box's origin, which is also the texture's) and drawn over the
/// overlay with blend mode `.copy`. The drag therefore shows what mouse-up commits.
///
/// THE CEILING. These redraws run on main, synchronous and uncancellable
/// (`NativeSelectionWork.context(nil)`), so one nominal stamp spends at most `dualPreviewWork`
/// box pixels: a box's figure (`dualPreviewFigure`, the sum over the stroke's dabs of each dab's
/// box intersected with it) is taken before its core call, boxes go in laid order, and a box whose
/// figure exceeds what is left of the stamp's budget is not redrawn. The commit is exact either
/// way, and a later stamp's box that fits redraws those pixels from the whole stroke. The figure is
/// a declared constant, not a measurement; the owner's hand check judges it.
extension ImageCanvasView {
    /// One nominal stamp's preview budget: 2²⁴ box pixels.
    static let dualPreviewWork: UInt64 = 16_777_216

    /// Sum over the dabs (4 doubles each, canvas coordinates) of each dab's clipped box ∩
    /// `box`; saturating.
    static func dualPreviewFigure(box: (x0: Int, y0: Int, x1: Int, y1: Int), dabs: [Double],
                                  dualDabs: [Double]) -> UInt64 {
        var total: UInt64 = 0
        for list in [dabs, dualDabs] {
            for k in stride(from: 0, to: list.count - list.count % 4, by: 4) {
                let area = overlap(box, x: list[k], y: list[k + 1], diameter: list[k + 2])
                let (sum, overflow) = total.addingReportingOverflow(area)
                total = overflow ? .max : sum
            }
        }
        return total
    }

    /// The pixels a dab's integer box (floor and ceil of the centre ∓ the radius) shares with
    /// `box`, 0 when they do not meet or the dab is not finite.
    private static func overlap(_ box: (x0: Int, y0: Int, x1: Int, y1: Int),
                                x: Double, y: Double, diameter: Double) -> UInt64 {
        let radius = diameter / 2
        let x0 = max((x - radius).rounded(.down), Double(box.x0))
        let y0 = max((y - radius).rounded(.down), Double(box.y0))
        let x1 = min((x + radius).rounded(.up), Double(box.x1))
        let y1 = min((y + radius).rounded(.up), Double(box.y1))
        guard x1 > x0, y1 > y0 else { return 0 }
        return UInt64(x1 - x0) * UInt64(y1 - y0)
    }

    /// One dab's box clipped to the canvas (`brush_coverage::bounds`).
    private func dualBox(x: Double, y: Double, diameter: Double) -> (x0: Int, y0: Int, x1: Int, y1: Int) {
        let radius = diameter / 2
        func edge(_ value: Double, _ limit: Int) -> Int {
            value.isFinite ? Int(min(max(value, 0), Double(limit))) : 0
        }
        return (edge((x - radius).rounded(.down), overlayWidth),
                edge((y - radius).rounded(.down), overlayHeight),
                edge((x + radius).rounded(.up), overlayWidth),
                edge((y + radius).rounded(.up), overlayHeight))
    }

    /// The preview of one nominal stamp of a dual stroke (the extension's doc): `laid` are the
    /// first-tip dabs it laid, and the second-tip dabs from index `dualFrom` on are the ones it
    /// laid. Each laid dab's box is redrawn from the stroke so far, within the stamp's budget.
    func stampDualRegions(_ laid: [ResolvedDab], dualFrom: Int, in context: CGContext) {
        guard let dual = nativeStrokeSession?.dual, let ink = strokeTextureInk else { return }
        let dabs = nativeStrokeSession?.dabs ?? []
        let second = nativeStrokeSession?.dualDabs ?? []
        let shapes = nativeStrokeSession?.dynamicsShaped == true ? nativeStrokeSession?.shapes : nil
        let texture = nativeStrokeSession?.texture
        var boxes = laid.map { dualBox(x: Double($0.point.x), y: Double($0.point.y),
                                       diameter: Double($0.diameter)) }
        for k in stride(from: 4 * max(dualFrom, 0), to: second.count, by: 4) {
            boxes.append(dualBox(x: second[k], y: second[k + 1], diameter: second[k + 2]))
        }
        var budget = Self.dualPreviewWork
        for box in boxes where box.x1 > box.x0 && box.y1 > box.y0 {
            let figure = Self.dualPreviewFigure(box: box, dabs: dabs, dualDabs: second)
            guard figure <= budget else { continue }
            budget -= figure
            drawDualBox(box, dabs: dabs, shapes: shapes, second: second, tip: dual.tip,
                        texture: texture, ink: ink, in: context)
        }
    }

    /// `box` of the overlay replaced by the core's image of the stroke so far over it: the dabs
    /// of either list whose boxes meet it, in order, shifted by its origin. A box that one list
    /// does not reach is the product's zero, so it is cleared. A stroke the core refuses is not
    /// drawn; the commit is exact either way.
    private func drawDualBox(_ box: (x0: Int, y0: Int, x1: Int, y1: Int), dabs: [Double],
                             shapes: [Double]?, second: [Double], tip: BrushTip,
                             texture: StrokeTexture?, ink: [UInt8], in context: CGContext) {
        let (ox, oy) = (Double(box.x0), Double(box.y0))
        func filtered(_ list: [Double], pairs: [Double]?) -> ([Double], [Double]) {
            var kept: [Double] = [], keptPairs: [Double] = []
            for k in stride(from: 0, to: list.count - list.count % 4, by: 4)
            where Self.overlap(box, x: list[k], y: list[k + 1], diameter: list[k + 2]) > 0 {
                kept.append(contentsOf: [list[k] - ox, list[k + 1] - oy, list[k + 2], list[k + 3]])
                if let pairs { keptPairs.append(contentsOf: [pairs[k / 2], pairs[k / 2 + 1]]) }
            }
            return (kept, keptPairs)
        }
        let (first, firstShapes) = filtered(dabs, pairs: shapes)
        let (secondDabs, _) = filtered(second, pairs: nil)
        let (width, height) = (box.x1 - box.x0, box.y1 - box.y0)
        let rect = CGRect(x: box.x0, y: box.y0, width: width, height: height)
        guard !first.isEmpty, !secondDabs.isEmpty else {
            context.clear(rect)
            return
        }
        guard let stroke = try? NativeBrushStroke(tip: strokeTip, pixel: false, dabs: first,
                  shapes: shapes == nil ? nil : firstShapes, texture: texture,
                  dual: StrokeDual(tip: tip, dabs: secondDabs)),
              let pixels = try? stroke.coreOverlay(width: width, height: height,
                                                   origin: (box.x0, box.y0), rgb: ink,
                                                   selection: nil, opacity: 1),
              let provider = CGDataProvider(data: Data(pixels) as CFData),
              let image = CGImage(
                width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: width * 4, space: documentColorSpace,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
        else { return }
        // The overlay context is top-left flipped and the image is not symmetric, so it is
        // drawn un-flipped locally, row 0 at the box's top, replacing what the box held.
        context.saveGState()
        context.setBlendMode(.copy)
        context.translateBy(x: CGFloat(box.x0), y: CGFloat(box.y1))
        context.scaleBy(x: 1, y: -1)
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        context.restoreGState()
    }
}
