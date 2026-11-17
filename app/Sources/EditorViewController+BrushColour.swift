import AppKit

/// Colour dynamics on the canvas (`BrushColour.swift`, `designs/brush-dynamics.md` slice 5): at
/// mouse-down the canvas asks for the stroke's colour handle (`ImageCanvasView
/// .onStrokeColourHandle`) beside its dynamics handle, with the stroke's seed.
extension EditorViewController {
    /// The stroke's colour handle, or nil, in which case the stroke paints its one ink (it is
    /// never refused): nil unless the tool is the Brush, the target is a layer outside Quick
    /// Mask, the mode is Normal, the document is neither CMYK, Indexed nor Bitmap, neither swatch
    /// is above white on a float document (an above-white foreground keeps its one linear ink),
    /// and the amounts the document takes (`reduced(for:)`) are not all 0. The foreground and
    /// the background are the two swatches through the stroke's own ink route (`LabPaint.color`,
    /// then `PrecisionInk` in the document's space); a Lab document's handle reads them as its
    /// codes (`lab`).
    func strokeColourHandle(_ colour: BrushDynamics.ColorDynamics,
                            seed: UInt32) -> RasterBrushColour? {
        guard currentTool == .brush, let document, let doc = document.doc,
              strokeTarget == .layer, !canvas.strokeOnQuickMask, !canvas.strokeOnMask,
              precisionStroke.map({ $0.capture.target == .layer && !$0.capture.quickMask }) ?? true,
              paintStrokeBlendMode() == DR_BLEND_NORMAL,
              !doc.isCmyk, !doc.isIndexed, !doc.isBitmap else { return nil }
        let shared = ToolOptionsStore.shared.sharedState
        if doc.isFloat, ExtendedRangeColor.aboveWhite(canvas.paintColor, in: doc) != nil
            || ExtendedRangeColor.aboveWhite(backgroundColor, in: doc) != nil {
            return nil
        }
        let amounts = colour.reduced(for: doc)
        guard !amounts.isZero else { return nil }
        do {
            let foreground = try PrecisionInk(color: LabPaint.color(canvas.paintColor,
                stored: shared.foregroundLab, in: doc), space: document.nsColorSpace).rgba
            let background = try PrecisionInk(color: LabPaint.color(backgroundColor,
                stored: shared.backgroundLab, in: doc), space: document.nsColorSpace).rgba
            return try BrushDynamics.colourHandle(amounts, seed: seed, foreground: foreground,
                                                  background: background, lab: doc.isLab)
        } catch {
            return nil
        }
    }
}
