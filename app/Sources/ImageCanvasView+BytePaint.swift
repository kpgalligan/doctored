import AppKit

struct PreparedBytePaintStroke {
    let stroke: NativeBrushStroke
    let canonical: [String: Any]
}

extension ImageCanvasView {
    /// Every byte gesture whose actual footprint is stamped needs the exact
    /// resolved deposits. Plain hard mouse paths retain legacy point meaning.
    func byteDabSessionWanted(coverage: Bool, pressure: CGFloat?) -> Bool {
        guard !nativeBrushEnabled else { return false }
        // Smudge is computed from the stroke's dabs, never from its overlay,
        // so it always needs them.
        if tool == .clone || tool == .heal || tool == .smudge { return true }
        // The Art History Brush's clicks are its dabs, as Smudge's are.
        if tool == .artHistoryBrush { return true }
        let eligible: Set<EditorTool> = [.brush, .eraser, .dodge, .spotHeal,
            .blurBrush, .sharpenBrush, .sponge, .historyBrush]
        guard eligible.contains(tool) || coverage else { return false }
        let tip = BrushTip(hardness: brushHardness, flow: brushFlow,
            spacingPercent: brushSpacingPercent, angleDegrees: brushAngle,
            roundness: brushRoundness)
        return SoftBrush.isStamped(tip: tip, size: brushSize)
            || (pressure != nil && (brushPressureSize || brushPressureOpacity))
            || brushAirbrush
    }

    /// Mouse-down's dab-image admission, asked before `beginStroke`
    /// builds the stroke's dab image or mask: nothing for a size-1 stroke or
    /// one that builds no image and commits through no byte dab session; else
    /// `SoftBrush.admitDabImage`, with the commit's whole charge
    /// (`BytePaintRasterizer.pixels`: the overlay, 4096 and its own dab image)
    /// beside it for a byte dab session, so the commit never asks for more
    /// than the gesture was admitted with.
    func admitDabImages(diameter: CGFloat) throws {
        guard diameter > 1 else { return }
        let byteSession = !nativeBrushEnabled && nativeStrokeSession != nil
        let builds = (tool == .clone || tool == .heal)
            ? SoftBrush.isSoft(hardness: brushHardness, size: diameter)
            // A pressure-tracking stroke always has a session, so this is `beginStroke`'s `stamped`.
            : nativeStrokeSession != nil || brushAirbrush
                || SoftBrush.isStamped(tip: strokeTip, size: diameter)
        guard builds || byteSession else { return }
        let commit = byteSession
            ? UInt64(overlayWidth) * UInt64(overlayHeight) * 4 + 4096
                + SoftBrush.dabImageScratch(diameter: diameter)
            : 0
        try SoftBrush.admitDabImage(diameter: diameter, extra: commit)
    }

    func finalizeBytePaintStroke() throws {
        guard !nativeBrushEnabled, let session = nativeStrokeSession else { return }
        guard let data = overlayData, overlayWidth > 0, overlayHeight > 0 else {
            throw DoctoredCoreError(message: "Byte stroke overlay is unavailable.")
        }
        let stroke = try session.resolved()
        let canonical = try stroke.canonical
        if strokeOnQuickMask {
            preparedByteQuickMask = try QuickMaskStroke(byteBrush: stroke, size: brushSize,
                width: overlayWidth, height: overlayHeight,
                opacity: Double(brushOpacity), erase: tool == .eraser)
        }
        let ink: ByteDabInk
        if tool == .clone || tool == .heal {
            guard let source = cloneSnapshot else {
                throw DoctoredCoreError(message: "No captured retouch source is available.")
            }
            ink = .source(source)
        } else if strokeOnMask || strokeOnQuickMask || tool.strokeIsCoverage
                    || tool == .spotHeal {
            ink = .color(ColorProfile.coverageInk(
                tool == .eraser ? .black : .white, in: documentColorSpace))
        } else {
            guard let color = ColorProfile.quantized(
                paintColor, in: NSColorSpace(cgColorSpace: documentColorSpace) ?? .sRGB)
            else { throw DoctoredCoreError(message: "Could not convert the paint color.") }
            ink = .color(color)
        }
        let coverageOpacity: Double? = strokeOnMask || strokeOnQuickMask
            ? Double(strokeCoverageScale) : nil
        let pixels = try BytePaintRasterizer.pixels(stroke: stroke, size: brushSize,
            ink: ink, width: overlayWidth, height: overlayHeight,
            space: documentColorSpace, selection: strokeOnQuickMask ? nil : strokeByteSelection,
            coverageOpacity: coverageOpacity)
        pixels.withUnsafeBytes { bytes in
            if let source = bytes.baseAddress {
                data.copyMemory(from: source, byteCount: pixels.count)
            }
        }
        preparedByteStroke = PreparedBytePaintStroke(stroke: stroke, canonical: canonical)
        if !strokeOnQuickMask && !strokeOnMask && strokeHealKind == nil { emitStrokeUpdate() }
    }

    func cancelByteStrokeForWindowClose() {
        guard strokeActive, !nativeBrushEnabled else { return }
        cancelStroke()
    }
}
