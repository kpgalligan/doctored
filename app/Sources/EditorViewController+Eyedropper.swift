import AppKit

// The Eyedropper's options: Sample from (the composite, or the active
// layer alone), the sampling ring and the recent swatches.
//
// Sample from — Current layer reads the layer's own pixels on a transparent
// canvas (`layerCanvasImage`, at 8 and 16 bits and in float), through the
// same samplers the composite goes through, so Sample Size and the readout
// behave alike. An adjustment layer has no pixels, so it reads the composite.
//
// The ring and the recents follow one press: the canvas shows the ring from
// the press to the release (`ImageCanvasView+EyedropperRing.swift`), and the
// colour the release leaves in the foreground joins the recents.

/// The last eight colours the Eyedropper picked, newest first. App-wide and
/// in memory only, like the foreground colour they came from.
enum RecentSwatches {
    static let limit = 8
    private(set) static var colors: [NSColor] = []

    /// The popup's rows: each colour's sRGB hex.
    static var titles: [String] { colors.map { TextLayer.hex($0) } }

    static func record(_ color: NSColor) {
        let hex = TextLayer.hex(color)
        colors.removeAll { TextLayer.hex($0) == hex }
        colors.insert(color, at: 0)
        if colors.count > limit { colors.removeLast(colors.count - limit) }
    }
}

/// The active layer's canvas image, kept for the ticks of one eyedropper
/// press so a drag does not rebuild it per tick. Main thread only; cleared
/// at the release.
private enum EyedropperLayerSource {
    static weak var doc: RasterDocument?
    static var layer = -1
    static var bytes: RasterImage?
    static var typed: RasterPrecisionImage?
    /// Whether the press in flight has picked a colour yet.
    static var picked = false
    /// Where the press in flight last picked (#1: a CMYK document's ink is read there once,
    /// when the press ends, not on every drag tick).
    static var point: CGPoint?
    /// The L*a*b* the press in flight last picked on a Lab document (#1 Lab phase 2e), set as
    /// the foreground's Lab ink when the press ends.
    static var lab: LabInk?

    static func clear() {
        doc = nil; layer = -1; bytes = nil; typed = nil
    }

    static func matches(_ current: RasterDocument, _ index: Int) -> Bool {
        doc === current && layer == index
    }
}

extension EditorViewController {
    /// The layer Sample from — Current layer reads, or nil for the composite:
    /// the option says All layers, or the active entry has no pixels of its
    /// own (an adjustment layer).
    private func eyedropperLayer(_ document: ImageDocument, _ doc: RasterDocument) -> Int? {
        guard ToolOptionsStore.shared.sample.fromIndex == 0 else { return nil }
        let index = document.activeLayerIndex
        guard index >= 0, index < doc.layerCount, !doc.layerIsAdjustment(index) else { return nil }
        return index
    }

    /// The 8-bit image the eyedropper samples: `byteComposite(of:)`, or the
    /// active layer alone with Sample from — Current layer. nil when there
    /// is no document or the render failed, after
    /// `eyedropperReadFailed()`.
    func eyedropperComposite() -> RasterImage? {
        guard let document, let doc = document.doc else { return nil }
        if let layer = eyedropperLayer(document, doc) {
            if EyedropperLayerSource.matches(doc, layer), let image = EyedropperLayerSource.bytes {
                return image
            }
            if let image = doc.layerCanvasImage(layer) {
                EyedropperLayerSource.clear()
                EyedropperLayerSource.doc = doc
                EyedropperLayerSource.layer = layer
                EyedropperLayerSource.bytes = image
                return image
            }
        }
        do {
            return try document.byteComposite(of: doc)
        } catch {
            eyedropperReadFailed()
            return nil
        }
    }

    /// The typed image the 16-bit and float eyedropper samples: the current
    /// projection, or the active layer alone with Sample from — Current layer.
    func eyedropperProjection(_ document: ImageDocument) throws -> RasterPrecisionImage {
        guard let doc = document.doc, let precision = doc.precision,
              let layer = eyedropperLayer(document, doc) else {
            return try document.currentPrecisionProjection()
        }
        if EyedropperLayerSource.matches(doc, layer), let image = EyedropperLayerSource.typed {
            return image
        }
        let context = try PrecisionWorkContext(source: precision)
        let image = try precision.layerCanvasImage(
            layer, workingLimit: context.limit, otherRetained: context.otherRetained)
        EyedropperLayerSource.clear()
        EyedropperLayerSource.doc = doc
        EyedropperLayerSource.layer = layer
        EyedropperLayerSource.typed = image
        return image
    }

    /// An eyedropper read failed. It never presents an alert —
    /// an alert in the press would swallow its mouse-up and leave the
    /// canvas's eyedropper drag latched — so it beeps on the press only and
    /// drag ticks stay silent; the status bar already shows "Preview failed"
    /// with the sentence as its tooltip.
    func eyedropperReadFailed() {
        if NSApp.currentEvent?.type == .leftMouseDown { NSSound.beep() }
    }

    /// One sample tick's colour into the foreground, with its text (copied
    /// when Copy on Pick is on). The first tick of a press opens the sampling
    /// ring around `point` with the foreground as it was; later ticks move it.
    func pickSample(_ color: NSColor, text: String, at point: CGPoint) {
        let previous = canvas.eyedropperRing?.previous ?? paintColor
        setPaintColor(color)
        EyedropperLayerSource.point = point
        lastSampleColor = paintColor
        lastSampleText = text
        EyedropperLayerSource.picked = true
        if ToolOptionsStore.shared.sample.showRing {
            canvas.eyedropperRing = EyedropperRing(point: point, picked: color, previous: previous)
        }
        if ToolOptionsStore.shared.sample.copyOnPick {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
        }
        optionsBar.refreshValues()
    }

    /// One Lab document sample tick (#1 Lab phase 2e): the swatch shows the picked L*a*b*'s
    /// clipped sRGB (`LabPaint.display`), the text reads L*, a*, b*, and the press latches the
    /// L*a*b* itself, which becomes the foreground's Lab ink when the press ends.
    func pickLabSample(_ readout: PrecisionReadout, at point: CGPoint) {
        let ink = LabInk(normalized: readout.normalized)
        pickSample(LabPaint.display(ink), text: LabDocument.readoutText(readout.normalized), at: point)
        EyedropperLayerSource.lab = ink
    }

    /// The CMYK composite's ink under `point` at the Sample Size (#1); nil for an RGB
    /// document, Sample from — Current layer, or a read that failed.
    private func cmykSampleInk(at point: CGPoint) -> CmykInk? {
        let options = ToolOptionsStore.shared.sample
        guard let doc = document?.doc, doc.isCmyk, options.fromIndex == 1, let precision = doc.precision,
              let context = try? PrecisionWorkContext(source: precision),
              let sample = try? precision.cmykSample(x: Int(floor(point.x)), y: Int(floor(point.y)),
                                                     reach: options.sampleSizeIndex,
                                                     workingLimit: context.limit,
                                                     otherRetained: context.otherRetained),
              let percent = sample["ink_percent"] as? [NSNumber] else { return nil }
        return CmykInk(percent: percent.map(\.doubleValue))
    }

    /// The press ended: the ring goes, the layer image is released, and the
    /// colour the press left in the foreground joins the recent swatches.
    func eyedropperPressEnded() {
        canvas.eyedropperRing = nil
        let pickedThisPress = EyedropperLayerSource.picked
        let point = EyedropperLayerSource.point
        EyedropperLayerSource.picked = false
        EyedropperLayerSource.point = nil
        EyedropperLayerSource.clear()
        let lab = EyedropperLayerSource.lab
        EyedropperLayerSource.lab = nil
        guard pickedThisPress, let picked = lastSampleColor else { return }
        // #1: in a CMYK document the composite's own ink where the press
        // ended is the foreground's ink, kept exactly (Sample from — Current
        // layer keeps the colour, which converts back through the profile).
        if let point, let ink = cmykSampleInk(at: point) { setPaintInk(ink, color: paintColor) }
        // #1 Lab phase 2e: in a Lab document the L*a*b* picked is the foreground's Lab ink,
        // kept exactly (the swatch shows its clipped sRGB).
        if let lab { setPaintLab(lab, color: paintColor) }
        RecentSwatches.record(picked)
        // The popup's rows are built with the bar, so the bar is rebuilt.
        if currentTool == .eyedropper { updateOptionsBar() }
    }

    /// The options bar's Recent popup: row `index` becomes the foreground.
    func chooseRecentSwatch(_ index: Int) {
        guard RecentSwatches.colors.indices.contains(index) else { return }
        setPaintColor(RecentSwatches.colors[index])
        optionsBar.refreshValues()
    }
}
