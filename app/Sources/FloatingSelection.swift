import AppKit

// The floating transform's box is the FLOAT's opaque bounds: `box` stays the cheap
// pre-prompt check, then `floatBox` asks the core (`SelectedPixelsFloat.bounds`, one short "Free
// Transform" wait) and the capture (base, selection history, active layer, coverage — taken BEFORE
// the wait) must still be the document's after it, else `staleRefusal`; MCP does the same titled
// "Transform Selected Pixels". The commands carry the selection's `bounds`. The commit
// builds a `TransformedSelection`, which plans the pixel edit FIRST so a refused transform
// answers with the pixel edit's own sentence (no work refusal: it runs as a cancellable
// job); an integer translation is `moved(dx:dy:)`
// on main, else a standalone `NativeSelectionWork` `transform`/`perspective` with the same map and
// filter. The pixels and then the selection run in ONE compound background job with one cancellable
// wait: 8-bit through `MoveSelectedPixels.prepared(…, base:, selection:, stale: staleRefusal,
// transformedSelection:)`, 16-bit through the open-time gesture's `finish(…,
// transformedSelection:)`; a pixel no-op never computes the selection. ONE "Transform Selected
// Pixels" step.

/// Free Transform of the selected pixels: ⌘T and `transform_selected_pixels`
/// with a selection active. ONE pixel layer — the active one — floats its
/// pixels × the selection coverage, resamples them as premultiplied f32
/// through the box's affine or perspective map, leaves an `a·(1−c)` hole,
/// composites back source-over clipped to the canvas, and the transformed
/// coverage becomes the selection, all in ONE undo step ("Transform Selected
/// Pixels"). The core op is `SelectedPixelsEdit` widened with
/// `transform_selected` (`core/src/selected_pixels_transform.rs`), the same
/// request at 8 and 16 bits; the 8-bit commit is
/// `MoveSelectedPixels.prepared`.
///
/// Decisions stated here and in the feature reference: one layer, the active one (a
/// multi-layer panel selection refuses by name); links are ignored; the box
/// is the FLOAT's opaque bounds — the pixels with alpha > 0 under the
/// selection, found by the core in one short cancellable
/// wait; the hole is transparent; a 32-bit float document transforms
/// the layer's f32 pixels, a filtered landing clamped to the float's own
/// visible range. ⌥⌘T (Free Transform a Copy) is the same session with
/// `duplicate`: no hole, one "Transform Copy" step.
///
/// With a LAYER MASK or ALPHA CHANNEL as the edit target the same
/// session floats that plane's selected coverage (`target`, `background`).
/// The box is the selection's bounds (every selected plane pixel floats);
/// the vacated area takes the BACKGROUND swatch as coverage gray, the
/// rule for a plane without transparency (`planeBackground`); a duplicate
/// leaves none. Masks and channels are admitted at 8, 16 and 32-bit float;
/// Quick Mask refuses. The decisions live in `core/src/selected_plane.rs`.
///
/// With the RED, GREEN, BLUE or ALPHA plane as the edit target the
/// session floats that one component of the layer (`target`, `swatch`), at
/// 8, 16 and 32-bit float: the layer path's lock alert (Pixels; Alpha a
/// merge edit) and rasterize prompt, the selection's bounds as the box, and
/// the vacated area takes the BACKGROUND swatch's 0.30 / 0.59 / 0.11 luma
/// rounded to a code (measured against Photoshop 26.10.0,
/// `plane-gray-431-633/take1`; Alpha: transparent). The decisions live in
/// `core/src/selected_color_plane.rs`.

/// The background colour a plane transform leaves in the vacated area: the
/// swatch as coverage gray (u16), the hex it was read from (recorded), and
/// for a Red, Green or Blue target the swatch's 8-bit codes in the
/// document profile's encoding (the request's `swatch`; nil otherwise).
struct PlaneBackground {
    let gray: UInt16
    let hex: String
    let swatch: [UInt8]?
}

/// What ⌘T captured when it opened over a selection. Everything the preview
/// and the commit use derives from `base` and `selection`, captured AFTER the
/// rasterize prompt (a modal that services MCP).
struct FloatingSelection {
    let layer: Int
    let base: RasterDocument
    /// `base`, with the layer's description dropped when `described` (8-bit;
    /// the typed core clears a description itself).
    let source: RasterDocument
    let selection: SelectionHistoryState
    let coverage: [UInt16]
    let described: Bool
    /// 8-bit only: the off-main preview's captured budget.
    let budget: (limit: UInt64, other: UInt64)?
    /// ⌥⌘T, Free Transform a Copy: no hole, the copy transforms and
    /// the original stays; one "Transform Copy" step.
    let duplicate: Bool
    /// The plane the session floats — the layer's pixels, its mask or
    /// an alpha channel.
    var target: PaintTarget = .layer
    /// A plane target's vacated-area colour; nil for the layer.
    var background: PlaneBackground? = nil

    /// The status line's name for the session.
    var statusName: String {
        let what: String
        switch target {
        case .mask: what = "selected mask area"
        case .channel: what = "selected channel area"
        case .layer, .plane, .ink: what = "selected pixels"
        }
        return duplicate ? "Free Transform · copy of \(what)" : "Free Transform · \(what)"
    }
}

/// The selection a floating-transform commit publishes, computed in the
/// SAME background job as the pixel edit (after it). An integer translation
/// moves the captured selection exactly on main; anything else
/// resamples the captured coverage through the same map and filter as a
/// standalone native selection op.
struct TransformedSelection {
    let captured: SelectionHistoryState
    let width: Int
    let height: Int
    /// The integer translation, moved exactly on main.
    let delta: (dx: Int, dy: Int)?
    /// nil exactly when `delta` is not.
    let work: NativeSelectionWork?

    var scratchEstimate: UInt64 { work?.scratchEstimate ?? 0 }

    /// Plans the PIXEL edit first (and discards the plan), so a refused
    /// transform answers with the pixel edit's sentence before the selection
    /// plan can refuse with its own. Neither plan refuses for work.
    init(source: RasterDocument, command: PrecisionPixelCommand, captured: SelectionHistoryState,
         coverage: [UInt16], box: CGRect, matrix: CGAffineTransform, quad: [CGPoint]?,
         sampler: DrResizeFilter) throws {
        if source.isHighDepth {
            _ = try PrecisionGeometryWork(
                source: source, request: .transformSelected(command: command, coverage: coverage)
            ).scratchEstimate()
        } else {
            _ = try source.selectedPixelsPlan(request: command.request)
        }
        self.captured = captured
        width = source.width
        height = source.height
        delta = TransformSelectedPixels.integerTranslation(matrix, quad: quad)
        guard delta == nil else {
            work = nil
            return
        }
        let filter = try RasterDocument.precisionFilter(sampler).rawValue
        let operation: [String: Any]
        if let quad {
            operation = [
                "kind": "perspective",
                "source_rect": [box.minX, box.minY, box.width, box.height].map(Double.init),
                "quad": quad.flatMap { [Double($0.x), Double($0.y)] },
                "filter": filter,
            ]
        } else {
            operation = [
                "kind": "transform",
                "matrix": [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
                    .map(Double.init),
                "filter": filter,
            ]
        }
        work = try NativeSelectionWork(width: width, height: height, operation: operation,
                                       input: coverage)
    }

    /// Worker side: the resampled coverage, or nil for an integer translation.
    func compute(control: RasterJobControl) throws -> [UInt16]? {
        try work?.compute(control: control)
    }

    /// Main side: the selection history state to publish.
    func state(_ samples: [UInt16]?) throws -> SelectionHistoryState {
        if let delta { return captured.moved(dx: delta.dx, dy: delta.dy) }
        guard let samples else {
            throw DoctoredCoreError(message: "The transformed selection was not computed.")
        }
        var next = captured
        if let old = captured.selection { next.previous = old }
        next.selection = CanvasSelection(shape: .mask16(samples), canvasWidth: width,
                                         canvasHeight: height)
        next.quickMaskActive = false
        return next
    }
}

enum TransformSelectedPixels {
    static let actionName = "Transform Selected Pixels"
    static let duplicateActionName = "Transform Copy"
    static let emptyRefusal = "The selection holds no pixels to transform."
    static let staleRefusal = "The document changed while Free Transform was open. Transform again."

    /// The cheap pre-prompt check: the selection bounds ∩ the layer's pixel
    /// rect, nil when they do not overlap by at least a pixel. The handles
    /// open on `floatBox`, which lies inside it.
    static func box(_ doc: RasterDocument, layer: Int, selection: CanvasSelection) -> CGRect? {
        guard let info = doc.layerInfo(layer) else { return nil }
        let rect = CGRect(x: info.offsetX, y: info.offsetY, width: info.width, height: info.height)
        let box = selection.bounds.intersection(rect)
        guard !box.isNull, box.width >= 1, box.height >= 1 else { return nil }
        return box
    }

    /// The box the handles open on: the float's opaque bounds, asked of
    /// the core under one Cancel/Esc wait titled `title`. Throws
    /// `emptyRefusal` when no selected pixel is opaque. The wait services
    /// MCP, so callers re-check their capture after it.
    static func floatBox(_ source: RasterDocument, layer: Int, coverage: [UInt16],
                         title: String) throws -> CGRect {
        guard let box = try SelectedPixelsFloat.bounds(
            source, layer: layer, coverage: coverage, title: title)
        else { throw DoctoredCoreError(message: emptyRefusal) }
        return box
    }

    /// `MoveSelectedPixels`' refusals, worded for a transform. `panelSelection`
    /// false skips the multi-layer refusal for an MCP call that names its
    /// layer. With `planes` a mask, channel or colour-plane target is
    /// admitted: Quick Mask, a multi-layer panel selection on a mask or a
    /// colour plane, then the shared fill scope (mask presence, locks,
    /// channel existence, a colour plane's retained source and raster layer).
    static func refusal(_ document: ImageDocument, layer: Int, target: PaintTarget,
                        panelSelection: Bool = true, planes: Bool = false) -> String? {
        guard planes, target != .layer else {
            return MoveSelectedPixels.refusal(document, layer: layer, target: target,
                                              panelSelection: panelSelection,
                                              verb: "transform", gerund: "Transforming")
        }
        return MoveSelectedPixels.planeRefusal(document, layer: layer, target: target,
                                               panelSelection: panelSelection, verb: "transform",
                                               subject: "Free Transform of a selection")
    }

    /// A plane transform's box — the selection's bounds, since every
    /// selected plane pixel floats. nil for an empty selection.
    static func planeBox(_ selection: CanvasSelection) -> CGRect? {
        let box = selection.bounds
        guard !box.isNull, box.width >= 1, box.height >= 1 else { return nil }
        return box
    }

    /// The vacated area's colour for an authored hex. A mask or
    /// channel takes the colour in the document's space as coverage gray
    /// (`PlaneAlgebra.coverageColor`, the Fill/Gradient plane rule), its
    /// alpha ignored; a Red, Green or Blue target takes the colour's
    /// 8-bit codes in the document's space as the core's `swatch` (on a Lab
    /// document the gray of that plane's code of the colour's L*a*b*, #1
    /// Lab 6a-ii); Alpha
    /// takes neither (its hole is transparent). The UI passes the background
    /// swatch through `ActionArgs.hex`, so a recorded step replays the same
    /// colour.
    static func planeBackground(hex: String, target: PaintTarget,
                                in document: ImageDocument) throws -> PlaneBackground {
        guard let color = TextLayer.color(fromHex: hex) else {
            throw DoctoredCoreError(message: "background must be a hex colour such as #000000 "
                + "(got \"\(hex)\").")
        }
        let failed = DoctoredCoreError(message: "The background colour could not be converted to "
            + "the document's colour space.")
        if target.isInk {
            // #1: an ink plate's hole is the swatch's 30/59/11 gray read as ink (the core's
            // `cmyk_selected_plate`), from the swatch's sRGB 8-bit codes.
            guard let bytes = ColorProfile.bytes(color, in: .sRGB) else { throw failed }
            return PlaneBackground(gray: 0, hex: hex, swatch: Array(bytes.prefix(3)))
        }
        if document.doc?.isLab == true, case .plane(let plane) = target, plane != .alpha,
           let index = [RasterPlane.red, .green, .blue].firstIndex(of: plane) {
            // #1 Lab 6a-ii: a Lab plane's hole is that plane's 8-bit code of the background's
            // L*a*b* (Offset's rule, `FilterFrame.kernelParameters`): round(L*·255/100),
            // round(a* + 128), round(b* + 128). A gray swatch reads back as that code exactly.
            guard let bytes = ColorProfile.bytes(color, in: .sRGB),
                  let lab = RasterLab.lab(srgb: bytes.prefix(3).map { Double($0) / 255 })
            else { throw failed }
            let value = index == 0 ? lab[0] * 255 / 100 : lab[index] + 128
            let code = UInt8(min(max(value.rounded(), 0), 255))
            return PlaneBackground(gray: 0, hex: hex, swatch: [code, code, code])
        }
        guard let bytes = ColorProfile.bytes(color, in: document.nsColorSpace) else { throw failed }
        if case .plane(let plane) = target {
            return PlaneBackground(gray: 0, hex: hex,
                                   swatch: plane == .alpha ? nil : Array(bytes.prefix(3)))
        }
        guard let gray = PlaneAlgebra.coverageColor(bytes)?.first else { throw failed }
        return PlaneBackground(gray: UInt16(gray) * 257, hex: hex, swatch: nil)
    }

    /// The targeted plane as a gray image — the preview when the
    /// canvas shows that plane as its base. A colour plane is the
    /// edited COMPOSITE's plane, as the Channels base draws it. Runs off-main.
    static func planeDisplay(target: PaintTarget, layer: Int) -> (RasterDocument) -> CGImage? {
        { doc in
            let bytes: [UInt8]?
            switch target {
            case .mask: bytes = doc.maskDisplayBytes(layer)
            case .channel(let index): bytes = doc.channelDisplayBytes(index)
            case .plane(let plane):
                let image = doc.isHighDepth
                    ? (try? doc.nativeSourcePlaneDisplay(plane, layer: nil))
                    : doc.compositePlaneImage(plane.dr, maxSide: 0)
                return image?.makeCGImage(in: doc.colorSpace)
            case .ink(let plate):
                // #1: the edited composite's plate, ink dark, as the Channels base draws it.
                guard let precision = doc.precision,
                      let context = try? PrecisionWorkContext(source: precision) else { return nil }
                let image = try? precision.cmykPlaneView(
                    plate, maxSide: 0, workingLimit: context.limit,
                    otherRetained: context.otherRetained)
                return image?.makeCGImage(in: doc.colorSpace)
            case .layer: bytes = nil
            }
            return bytes.flatMap { CanvasSelection.grayImage($0, doc.width, doc.height) }
        }
    }

    /// The shared core request: `{"op":"transform_selected",…}` with the
    /// matrix, or with the box as `source_rect` and its four destinations,
    /// plus the selection's integral `bounds` and, for ⌥⌘T,
    /// `"duplicate": true`. A mask or channel target adds `"target"`
    /// and `"background"`, a colour plane `"target"` and, for R/G/B,
    /// `"swatch"`; a layer target sends neither.
    static func command(layer: Int, box: CGRect, matrix: CGAffineTransform, quad: [CGPoint]?,
                        sampler: DrResizeFilter, bounds: CGRect?,
                        duplicate: Bool = false, target: PaintTarget = .layer,
                        background: PlaneBackground? = nil) throws -> PrecisionPixelCommand {
        var fields: [String: Any] = [
            "op": "transform_selected", "layer": layer,
            "filter": try RasterDocument.precisionFilter(sampler).rawValue,
        ]
        if duplicate { fields["duplicate"] = true }
        MoveSelectedPixels.addPlaneTarget(&fields, target: target, background: background)
        if let bounds { fields["bounds"] = MoveSelectedPixels.requestBounds(bounds) }
        if let quad {
            guard quad.count == 4 else {
                throw DoctoredCoreError(message: "A perspective transform needs four corners.")
            }
            fields["source_rect"] = [box.minX, box.minY, box.width, box.height].map(Double.init)
            fields["quad"] = quad.flatMap { [Double($0.x), Double($0.y)] }
        } else {
            fields["matrix"] = [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
                .map(Double.init)
        }
        return try PrecisionPixelCommand(fields)
    }

    /// The whole-pixel delta when the matrix is exactly a translation (the
    /// core's own `exact_form` tolerance), which the core runs as a Move of
    /// the selected pixels.
    static func integerTranslation(_ matrix: CGAffineTransform, quad: [CGPoint]?)
        -> (dx: Int, dy: Int)?
    {
        guard quad == nil else { return nil }
        let near = { (value: CGFloat, target: CGFloat) in abs(value - target) <= 1e-9 }
        guard near(matrix.a, 1), near(matrix.b, 0), near(matrix.c, 0), near(matrix.d, 1),
              near(matrix.tx, matrix.tx.rounded()), near(matrix.ty, matrix.ty.rounded()),
              abs(matrix.tx) < 1e9, abs(matrix.ty) < 1e9 else { return nil }
        return (Int(matrix.tx.rounded()), Int(matrix.ty.rounded()))
    }

    /// The 8-bit preview's latest-wins key.
    static func previewKey(matrix: CGAffineTransform, quad: [CGPoint]?,
                           sampler: DrResizeFilter) -> [Double] {
        [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init)
            + (quad ?? []).flatMap { [Double($0.x), Double($0.y)] }
            + [Double(sampler.rawValue)]
    }

    static func record(_ doc: RasterDocument, layer: Int, transform: LayerTransform,
                       quad: [CGPoint]?, sampler: DrResizeFilter, exact: Bool,
                       duplicate: Bool = false, target: String? = nil,
                       background: String? = nil) -> [ActionStep] {
        .transformSelectedPixelsStep(
            layer: doc.layerRef(layer), transform: quad == nil ? transform : nil, corners: quad,
            sampler: EditorViewController.samplerAgentName(sampler), exact: exact,
            duplicate: duplicate, target: target,
            background: target == RasterPlane.alpha.agentName ? nil : background)
    }
}
