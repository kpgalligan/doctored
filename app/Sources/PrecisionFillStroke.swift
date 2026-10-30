import AppKit

// `fillSelection`, `strokeSelection`, `applyFill` and `applyStroke` branch on `isHighDepth` after
// the Quick Mask branch, the sheets capture a `PrecisionEditorCapture` and publish through it with
// the same records and alerts; a layer paints through the typed `paint_blend` with
// `preserve_alpha`, a coverage target through the scalar `paint` kind with `blend_mode` (a 16-bit
// colour plane with the ink's 30/59/11 luma, `planeInk`; a mask or alpha channel Rec. 709), the band
// is the u16 `stroke` selection kind and the layer edge `stroke_layer_edge`; the MCP twins branch
// after the Quick Mask check / width-and-location parse, reuse `selectionPaintArguments` and add
// `storage` (`"rgba32_float"` for a float layer); the seven selectors are in both the u16 and the
// float whitelists and `FloatDocument.tools` lists both tools (`floatCoreRefusal` maps a layer
// target to the `fill_stroke` family); at float the core's `float_paint` blends LINEAR ink in the
// float layer modes (`NativeLayerBlending.floatModes`), a mask or alpha channel fills as at 16 bits, and a colour plane is
// the same `paint_blend` with a `plane` (the ink's 30/59/11 luma into that one channel, from
// the UI and MCP alike); an above-white foreground or MCP's `rgba32` is `linear` ink.

/// Edit ▸ Fill… / Stroke… at 16 bits and 32-bit float: the typed request, the
/// stroke band, the runners and the layer-edge outline probe, shared by the
/// UI (`EditorViewController+PrecisionFillStroke.swift`) and MCP
/// (`AgentServer+PrecisionFillStroke.swift`). There is no pixel math here:
///
/// - A LAYER target paints through the 16-bit brush's one compositing path,
///   `paint_blend` (`precision_paint_tools`), over the selection's u16
///   coverage (Fill) or the stroke band (Stroke), with Preserve
///   Transparency as its `preserve_alpha`. Like the brush, it grows a
///   trimmed layer to the changed pixels unless Preserve Transparency or a
///   transparency lock keeps the layer's alpha.
/// - A COVERAGE target (mask, colour plane, alpha channel) is the scalar
///   `paint` kind with the blend mode (`precision_scalar_edit`) and the ink's
///   gray: Rec. 709 on a mask or alpha channel (`PrecisionScalarOperations.ink`),
///   and on one colour plane of a 16-bit layer the ink's 30/59/11 luma,
///   unrounded (`planeInk`; measured against Photoshop,
///   `scripts/photoshop-probe/plane_gray.py`, `plane-gray-431-633/take1`),
///   whatever the pixel's alpha.
/// - The band is the selection-plane `stroke` kind (`precision_selection_plane`),
///   the u16 twin of `dr_selection_stroke`.
/// - With nothing selected on a layer the core strokes the layer's own alpha
///   edge in layer space (`stroke_layer_edge`, `precision_fill_stroke`).
///
/// The ink is the authored colour in the DOCUMENT's space with
/// `color_space: "document"` on both paths. At 32-bit float the core decodes
/// that encoded colour to LINEAR light through the document profile's own
/// curves (`float_paint`), never boosted, and blends it in the float layer
/// modes (`NativeLayerBlending.floatModes`); a mask or alpha channel is u16 coverage at every depth, so
/// it fills exactly as at 16 bits. A float layer's COLOUR PLANE is the
/// same `paint_blend` with `"plane"`, which writes the ink's 30/59/11 luma
/// into that one channel, whatever the pixel's alpha (measured against
/// Photoshop, `scripts/photoshop-probe/plane_fill.py`, `plane-fill-431/take1`).
/// An above-white foreground, or MCP's `rgba32`, is `linear` ink:
/// linear light in the document's primaries, used as is.
enum PrecisionFillStroke {
    struct Request {
        let layer: Int
        let target: PaintTarget
        let color: NSColor
        let mode: DrBlendMode
        /// 0…1.
        let opacity: Double
        let preserve: Bool
        /// MCP's `rgba32`: linear light in the document's primaries, RGB
        /// 0…65504 and alpha 0…1; `color` is then the same colour built from
        /// it (`ExtendedRangeColor.linearColor`).
        var linear: [Double]? = nil
        /// A CMYK document's exact ink (#547): the swatch's stored ink or MCP's `ink`; nil
        /// converts `color` through the document's profile (`CmykPaint.ink`).
        var cmykInk: CmykInk? = nil
        /// #547 Lab phase 4a: `color` is MCP's authored sRGB on a Lab layer, sent as `srgb` ink
        /// so the core converts it by Mode ▸ Lab Color's rule (`AgentServer.labSrgbInk`).
        var srgb = false
    }

    /// Fill / Stroke ink as the core reads it: straight RGBA and how its RGB
    /// is interpreted.
    struct Ink {
        let rgba: [Double]
        let space: PrecisionColorSpace
    }

    /// The request's ink: `recordedLinear` as `linear` ink when there is one,
    /// else the authored colour as continuous document-space ink.
    static func ink(_ r: Request, capture: PrecisionEditorCapture) throws -> Ink {
        if let linear = recordedLinear(r, capture: capture) {
            return Ink(rgba: linear, space: .linear)
        }
        if r.srgb { return Ink(rgba: try PrecisionInk(color: r.color).rgba, space: .srgb) }
        let authored = try PrecisionInk(color: r.color, space: capture.facade.nsColorSpace)
        return Ink(rgba: authored.rgba, space: .document)
    }

    /// The linear ink the request paints with, nil when it takes the
    /// document route: on a float layer or one of its colour planes, the
    /// request's own `linear`, else the colour when it is above white
    /// (`ExtendedRangeColor.aboveWhite`). A mask or alpha channel is 0…1
    /// coverage and always takes the document route. The UI records it as
    /// the step's `rgba32`.
    static func recordedLinear(_ r: Request, capture: PrecisionEditorCapture) -> [Double]? {
        guard capture.facade.isFloat else { return nil }
        switch r.target {
        case .mask, .channel, .ink: return nil
        case .layer, .plane:
            return r.linear ?? ExtendedRangeColor.aboveWhite(r.color, in: capture.facade)
        }
    }

    /// Layer: `paint_blend` over `coverage` (nil = the whole canvas); a
    /// float layer's colour plane: the same with `"plane"`. Any other
    /// coverage target: the scalar `paint` kind with the blend mode, a 16-bit
    /// colour plane with `planeInk`'s gray and a mask or alpha channel with
    /// `ink`'s. nil when nothing changed.
    static func paint(
        _ r: Request, coverage: [UInt16]?, capture: PrecisionEditorCapture, title: String
    ) throws -> RasterPrecisionDocument? {
        let width = capture.facade.width, height = capture.facade.height
        let samples = coverage ?? [UInt16](repeating: 65535, count: width * height)
        var floatPlane: RasterPlane?
        if capture.facade.isFloat, case .plane(let plane) = r.target { floatPlane = plane }
        if r.target.isCoverage, floatPlane == nil {
            let ink = try PrecisionInk(color: r.color,
                space: ColorProfile.coverageSpace(capture.facade.nsColorSpace, for: r.color))
            // #547: one CMYK ink plate takes a colour plane's 30/59/11 grey (`cmyk_plate_scalar`).
            let gray = try r.target.isPlane || r.target.isInk
                ? PrecisionScalarOperations.planeInk(ink) : PrecisionScalarOperations.ink(ink)
            let work = try PrecisionScalarOperations.edit(
                capture, target: r.target, layer: r.layer,
                operation: ["kind": "paint", "value": Int(gray),
                            "opacity": r.opacity * ink.rgba[3],
                            "blend_mode": Int(r.mode.rawValue)],
                input: samples, selection: false)
            return try work.run(title: title)
        }
        if capture.source.isCmyk {
            // #547: a CMYK layer fills in ink, in the CMYK modes (`cmyk_paint`).
            if let reason = CmykPaint.modeRefusal(r.mode) { throw DoctoredCoreError(message: reason) }
            let ink = try CmykPaint.ink(r.color, stored: r.cmykInk, in: capture.source)
            let command = try CmykPaint.command([
                "op": "paint_blend", "layer": r.layer, "ink": ink, "opacity": r.opacity,
                "blend_mode": Int(r.mode.rawValue), "selection": false, "preserve_alpha": r.preserve,
            ])
            let input = try PrecisionScalarBuffer(samples: samples, width: width, height: height)
            return try PrecisionPixelEdit(capture.source, command: command, input: input).run(title: title)
        }
        let ink = try ink(r, capture: capture)
        var request: [String: Any] = [
            "op": "paint_blend", "layer": r.layer, "color": ink.rgba,
            "color_space": ink.space.rawValue, "opacity": r.opacity,
            "blend_mode": r.mode.rawValue, "selection": false,
            "preserve_alpha": floatPlane == nil && r.preserve,
        ]
        if let floatPlane { request["plane"] = floatPlane.agentName }
        let command = try PrecisionPixelCommand(request)
        let input = try PrecisionScalarBuffer(samples: samples, width: width, height: height)
        return try PrecisionPixelEdit(capture.source, command: command, input: input).run(title: title)
    }

    /// Edit ▸ Stroke…'s canvas-sized u16 band of `selection`, `widthPx` wide
    /// at `location` (the selection-plane `stroke` kind).
    static func band(
        of selection: CanvasSelection, widthPx: Double, location: StrokeLocation
    ) throws -> [UInt16] {
        try NativeSelectionWork(
            width: selection.canvasWidth, height: selection.canvasHeight,
            operation: ["kind": "stroke", "width": widthPx, "location": location.agentName],
            input: selection.maskSamples16()
        ).run(title: "Stroke")
    }

    /// The layer-edge request (`stroke_layer_edge`). `ink` is the colour keys: `["ink": [c, m,
    /// y, k, a]]` on a CMYK document (`cmyk_stroke`, #547), else `["color": …, "color_space": …]`.
    private static func edgeCommand(
        layer: Int, ink: [String: Any], widthPx: Double, location: StrokeLocation,
        mode: DrBlendMode, opacity: Double, preserve: Bool
    ) throws -> PrecisionPixelCommand {
        var request: [String: Any] = [
            "op": "stroke_layer_edge", "layer": layer, "width": widthPx,
            "location": location.agentName, "opacity": opacity,
            "blend_mode": mode.rawValue, "preserve_alpha": preserve,
        ]
        request.merge(ink) { _, new in new }
        return try PrecisionPixelCommand(request)
    }

    /// With nothing selected on a layer: the layer's own alpha edge stroked
    /// in layer space, growing the layer past the canvas when its alpha is
    /// free to change. nil when nothing changed. A CMYK layer strokes in ink, in the CMYK modes.
    static func strokeLayerEdge(
        _ r: Request, widthPx: Double, location: StrokeLocation, capture: PrecisionEditorCapture
    ) throws -> RasterPrecisionDocument? {
        let keys: [String: Any]
        if capture.source.isCmyk {
            if let reason = CmykPaint.modeRefusal(r.mode) { throw DoctoredCoreError(message: reason) }
            keys = ["ink": try CmykPaint.ink(r.color, stored: r.cmykInk, in: capture.source)]
        } else {
            let ink = try ink(r, capture: capture)
            keys = ["color": ink.rgba, "color_space": ink.space.rawValue]
        }
        let command = try edgeCommand(
            layer: r.layer, ink: keys, widthPx: widthPx, location: location,
            mode: r.mode, opacity: r.opacity, preserve: r.preserve)
        return try PrecisionPixelEdit(capture.source, command: command).run(title: "Stroke")
    }

    /// Whether `layer` has any alpha > 0 pixel to outline: the
    /// `stroke_layer_edge` plan's `outline`. `StrokeSettings.emptyLayer`
    /// names the other answer.
    static func layerHasOutline(_ capture: PrecisionEditorCapture, layer: Int) throws -> Bool {
        let keys: [String: Any] = capture.source.isCmyk
            ? ["ink": [0, 0, 0, 1, 1]]
            : ["color": [0, 0, 0, 1], "color_space": PrecisionColorSpace.document.rawValue]
        let command = try edgeCommand(
            layer: layer, ink: keys,
            widthPx: StrokeSettings.widthRange.lowerBound, location: .outside,
            mode: DR_BLEND_NORMAL, opacity: 1, preserve: false)
        let context = try PrecisionWorkContext(source: capture.source)
        let plan = try capture.source.pixelEditPlan(
            request: command.request, workingLimit: context.limit,
            otherRetained: context.otherRetained)
        return plan["outline"] as? Bool ?? false
    }
}
