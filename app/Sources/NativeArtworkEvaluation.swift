import AppKit

/// TextKit/CGPath preparation ends on main. Workers capture only immutable
/// commands, numeric colors and declared intrinsic bytes, never layout objects.
struct NativeArtworkEvaluation {
    /// The raster pixels a path part's coverage plane covers (the core's
    /// component `window`, `artwork_window`): `x`, `y`, `width`, `height`.
    struct Window: Equatable {
        let x: Int, y: Int, width: Int, height: Int
        var pixels: Int { width * height }
        var json: [Int] { [x, y, width, height] }

        /// `bounds` (raster pixels, y down) made integral, grown 1 px and
        /// clipped to the `width`×`height` raster: every pixel the path's
        /// flattened outline can ink, since its vertices lie on the path and
        /// the path inside its control points' box. A box that is not finite
        /// takes the whole raster; one that misses the raster, a 1×1 window
        /// at the origin (its coverage is zero either way).
        static func covering(_ bounds: CGRect, width: Int, height: Int) -> Window {
            let whole = Window(x: 0, y: 0, width: width, height: height)
            guard !bounds.isNull, [bounds.minX, bounds.minY, bounds.maxX, bounds.maxY]
                .allSatisfy({ $0.isFinite && abs($0) <= 1e9 }) else { return whole }
            let x0 = max(Int(bounds.minX.rounded(.down)) - 1, 0)
            let y0 = max(Int(bounds.minY.rounded(.down)) - 1, 0)
            let x1 = min(Int(bounds.maxX.rounded(.up)) + 1, width)
            let y1 = min(Int(bounds.maxY.rounded(.up)) + 1, height)
            guard x1 > x0, y1 > y0 else { return Window(x: 0, y: 0, width: 1, height: 1) }
            return Window(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
        }
    }

    enum Part {
        /// `points` are relative to `window`'s origin; the coverage is
        /// computed at the window's size.
        case path(commands: [UInt8], points: [Double], rgba: [Double], window: Window)
        case intrinsic([UInt8], opacity: Double)
        case coverage([UInt8], rgba: [Double])
        /// A fill layer's colour or gradient: the render component
        /// itself, which owns no sample plane.
        case planeless([String: Any])
    }
    let width: Int
    let height: Int
    let offsetX: Int
    let offsetY: Int
    let parts: [Part]
    /// The coverage samples every plane-owning part holds: a path part its
    /// window's pixels, a bitmap coverage part the raster's.
    let scalarCount: Int
    let intrinsicCount: Int
    /// How many parts own no sample plane (a fill layer's). Each is charged
    /// in `scratchBytes` as one coverage plane, and none counts against the
    /// per-component 64-million-sample bound: they hold no samples.
    let planelessCount: Int
    let lease: PrecisionEditPayload
    /// True in a 32-bit float document: `compute` renders through
    /// `dr_artwork_render_float`.
    let float: Bool
    /// True in a CMYK document (#1): every colour is five ink numbers, the
    /// components carry `ink`/`inks`, and `components()` feeds
    /// `dr_precision_doc_cmyk_artwork` (CmykArtwork.swift).
    let cmyk: Bool

    /// The per-pixel margin a float render adds on top of its f32 output
    /// plane, fitted at 24 MP (`core/src/artwork_render.rs`' admission
    /// record): the smallest integer at or above the core's
    /// `render_float_scratch` per-pixel term minus 8 that admits at least
    /// 1.25 × every measured peak.
    static let floatMarginPerPixel: UInt64 = 15

    /// The per-pixel margin a CMYK render adds to `scratchBytes`: the core's
    /// assembled CMYKA16 samples (10 B/px), which the RGB-shaped planes of
    /// `ownScratchBytes` do not name. Measured at 24 MP the whole call peaks
    /// at 14–15 B/px (`core/src/cmyk_artwork.rs`' measurement record: the
    /// first build's 40 over-charged it 3.8×); a miss raises it.
    static let cmykMarginPerPixel: UInt64 = 10

    /// The core's per-component bound (`artwork_render::MAX_COMPONENT_SAMPLES`):
    /// a coverage plane's samples, or an intrinsic plane's bytes. There is no
    /// whole-layer total.
    static let maximumComponentSamples = 64_000_000
    static let componentRefusal = failure("Artwork exceeds 64 million samples in one component.")

    init(_ drawing: NativeArtworkDrawing, float: Bool, cmyk: Bool = false) throws {
        self.float = float
        self.cmyk = cmyk
        guard drawing.width > 0, drawing.height > 0,
              drawing.width <= RasterImage.maxResizePixels / drawing.height,
              drawing.parts.count <= 256 else { throw Self.failure("Artwork exceeds its component or raster bounds.") }
        width = drawing.width; height = drawing.height
        offsetX = drawing.offsetX; offsetY = drawing.offsetY
        let count = width * height
        // A float render's numbers are linear light and may be above white;
        // a CMYK render's are ink (#1).
        let ink: ArtworkInk = cmyk ? .cmyk : float ? .linearSRGB : .srgb
        var captured: [Part] = []
        var scalar = 0, intrinsic = 0, planeless = 0
        var bytes: UInt64 = 4096
        let colorRefusal = cmyk
            ? "Artwork colors in a CMYK document must be ink: C, M, Y, K and alpha each 0–1."
            : float
            ? "Artwork colors must be linear light with red, green and blue 0–65504 and alpha 0–1."
            : "Artwork colors must be normalized sRGB. Choose an in-gamut color."
        // The component keys: a colour is `color`/`colors`, ink `ink`/`inks`.
        let colorKey = cmyk ? "ink" : "color"
        for part in drawing.parts {
            switch part {
            case let .path(path, rgba):
                guard ArtworkColor.valid(rgba, in: ink) else { throw Self.failure(colorRefusal) }
                // The plane covers only the path's box (the core's
                // per-component window), so a part costs its own extent,
                // not the raster's.
                let window = Window.covering(path.boundingBox, width: width, height: height)
                let (dx, dy) = (Double(window.x), Double(window.y))
                var commands: [UInt8] = [], points: [Double] = []
                var valid = true
                path.applyWithBlock { pointer in
                    let element = pointer.pointee
                    let command: UInt8, length: Int
                    switch element.type {
                    case .moveToPoint: command = 0; length = 1
                    case .addLineToPoint: command = 1; length = 1
                    case .addQuadCurveToPoint: command = 2; length = 2
                    case .addCurveToPoint: command = 3; length = 3
                    case .closeSubpath: command = 4; length = 0
                    @unknown default: valid = false; return
                    }
                    guard commands.count < 262_144, points.count / 2 + length <= 262_144 else { valid = false; return }
                    commands.append(command)
                    for index in 0..<length {
                        points.append(Double(element.points[index].x) - dx)
                        points.append(Double(element.points[index].y) - dy)
                    }
                }
                guard valid else { throw Self.failure("Artwork outline exceeds its bounded path command limit.") }
                guard window.pixels <= Self.maximumComponentSamples else { throw Self.componentRefusal }
                captured.append(.path(commands: commands, points: points, rgba: rgba, window: window))
                scalar += window.pixels; bytes += UInt64(commands.count + points.count * 8)
            case let .coverage(pixels, rgba):
                guard pixels.count == count, ArtworkColor.valid(rgba, in: ink) else {
                    throw Self.failure("Intrinsic monochrome coverage does not match its extent or ink.")
                }
                guard count <= Self.maximumComponentSamples else { throw Self.componentRefusal }
                captured.append(.coverage(pixels, rgba: rgba))
                scalar += count; bytes += UInt64(pixels.count)
            case let .intrinsic(pixels, opacity):
                guard pixels.count == count * 4, opacity.isFinite, (0...1).contains(opacity) else {
                    throw Self.failure("Intrinsic glyph data does not match its declared extent or opacity.")
                }
                guard pixels.count <= Self.maximumComponentSamples else { throw Self.componentRefusal }
                captured.append(.intrinsic(pixels, opacity: opacity))
                intrinsic += pixels.count; bytes += UInt64(pixels.count)
            case let .fill(rgba, linear):
                guard ArtworkColor.valid(rgba, in: ink) else { throw Self.failure(colorRefusal) }
                var component: [String: Any] = ["kind": "fill", colorKey: rgba]
                if let linear {
                    // The core's `linear` rule (`artwork_render`'s "Fill
                    // layers"): three finite components within ±4, RGB only.
                    guard !cmyk, FillLayerPayload.validLinear(linear) else {
                        throw Self.failure("A fill layer's wide colour is invalid.")
                    }
                    component["linear"] = linear
                }
                captured.append(.planeless(component))
                planeless += 1; bytes += 256
            case let .gradient(gradient, colors, linear, box):
                guard colors.allSatisfy({ $0.count == (cmyk ? 4 : 3) && ArtworkColor.valid($0 + [1], in: ink) })
                else { throw Self.failure(colorRefusal) }
                var component: [String: Any] = ["kind": "gradient", "gradient": gradient, colorKey + "s": colors]
                if let linear {
                    // The core's gradient `linear` rule (`artwork_render`'s
                    // "Fill layers"): one valid triple per stop, RGB only.
                    guard !cmyk, linear.count == colors.count,
                          linear.allSatisfy(FillLayerPayload.validLinear)
                    else { throw Self.failure("A gradient fill's wide stop colours are invalid.") }
                    component["linear"] = linear
                }
                if let box {
                    // The core's `box` rule (`artwork_render::gradient_frame`).
                    guard box.count == 4, box[2] >= 1, box[3] >= 1 else {
                        throw Self.failure("A gradient fill's align box is invalid.")
                    }
                    component["box"] = box
                }
                captured.append(.planeless(component))
                planeless += 1; bytes += UInt64(colors.count) * 256 + 256
            case let .pattern(fill):
                // RGB and CMYK alike: a CMYK document separates the texels at
                // render (FillLayer.swift's INK).
                captured.append(.planeless(["kind": "pattern", "fill": fill]))
                let rgba = ((fill["pattern"] as? [String: Any])?["rgba"] as? String)?.utf8.count ?? 0
                planeless += 1; bytes += UInt64(rgba) + 4096
            }
        }
        if captured.isEmpty {
            // One empty outline: its coverage is zero, so one pixel holds it.
            captured = [.path(commands: [], points: [], rgba: cmyk ? [0, 0, 0, 0, 0] : [0, 0, 0, 0],
                              window: Window(x: 0, y: 0, width: 1, height: 1))]
            scalar = 1
        }
        parts = captured; scalarCount = scalar; intrinsicCount = intrinsic
        planelessCount = planeless
        lease = try PrecisionEditPayload.admitted(bytes: bytes)
    }

    /// The core's coverage bookkeeping (`dr_artwork_path_coverage`: 118 MiB
    /// since the pair stage's full-edge sweep, 106 MiB with the x-cluster
    /// buffers and their 524,288-edge bound, 34 MiB before). One coverage
    /// call reserves it and releases it on return, and parts and members
    /// render one after another, so a set of evaluations needs it once
    /// (`ArtworkGeometryWork.scratchBytes`).
    static let coverageBookkeepingBytes: UInt64 = 118 * 1024 * 1024

    /// What this evaluation needs beside the coverage bookkeeping: its own
    /// planes, and the 14 MiB the fixed term has always had above the core's
    /// figure.
    var ownScratchBytes: UInt64 {
        let pixels = UInt64(width * height)
        // A float render's output is f32 (n·16, the u16 term's n·8 twice)
        // plus its fitted margin.
        let float = self.float ? pixels * 8 + Self.floatMarginPerPixel * pixels : 0
        return 14 * 1024 * 1024 + pixels * 18 + UInt64(width) * 24
            + (UInt64(scalarCount) + UInt64(planelessCount) * pixels) * 4
            + UInt64(intrinsicCount) * 2 + float
    }

    /// One evaluation's stage: 132 MiB (the bookkeeping and the 14 MiB) plus
    /// its planes; under the bookkeeping a small layer's coverage reservation
    /// would not fit this stage.
    var scratchBytes: UInt64 { Self.coverageBookkeepingBytes + ownScratchBytes }

    func compute(profile: Data, control: RasterJobControl) throws -> RasterPrecisionImage {
        let (description, coverage, intrinsic) = try components(control: control)
        return try control.withChildOperation(from: Double(parts.count) / Double(parts.count + 2),
                                             to: Double(parts.count + 1) / Double(parts.count + 2)) {
            try RasterArtworkCore.render(width: width, height: height, components: description,
                samples: coverage, intrinsic: intrinsic, profile: profile, float: float, control: $0)
        }
    }

    /// The component list (sorted-key JSON), the coverage planes in its order (component-major
    /// u16) and the intrinsic RGBA8 planes: what both renders take. Each path's coverage is the
    /// core's (`dr_artwork_path_coverage`), at every depth and in CMYK (C-same).
    func components(control: RasterJobControl) throws
        -> (json: String, samples: [UInt16], intrinsic: [UInt8]) {
        var coverage: [UInt16] = [], intrinsic: [UInt8] = [], descriptors: [[String: Any]] = []
        let colorKey = cmyk ? "ink" : "color"
        coverage.reserveCapacity(scalarCount); intrinsic.reserveCapacity(intrinsicCount)
        for (index, part) in parts.enumerated() {
            let start = Double(index) / Double(parts.count + 2)
            let end = Double(index + 1) / Double(parts.count + 2)
            guard !control.isCancelled else { throw JobCancelled() }
            switch part {
            case let .path(commands, points, rgba, window):
                let plane = try control.withChildOperation(from: start, to: end) {
                    try RasterArtworkCore.coverage(commands: commands, points: points,
                        width: window.width, height: window.height, control: $0)
                }
                coverage.append(contentsOf: plane)
                descriptors.append(["kind": "coverage", colorKey: rgba, "window": window.json])
            case let .coverage(bytes, rgba):
                let promoted = try PrecisionScalarBuffer.promoting(bytes, width: width, height: height)
                coverage.append(contentsOf: promoted.samples)
                descriptors.append(["kind": "coverage", colorKey: rgba])
            case let .intrinsic(bytes, opacity):
                intrinsic.append(contentsOf: bytes)
                descriptors.append(["kind": "intrinsic_rgba8", "opacity": opacity])
            case let .planeless(component):
                descriptors.append(component)
            }
        }
        let json = try JSONSerialization.data(withJSONObject: descriptors, options: .sortedKeys)
        guard let description = String(data: json, encoding: .utf8) else { throw Self.failure("Artwork component encoding failed.") }
        return (description, coverage, intrinsic)
    }

    static func failure(_ message: String) -> DoctoredCoreError { DoctoredCoreError(message: message) }
}

enum NativeArtwork {
    /// The longest description and component list: the core's 16 MiB
    /// meta cap (`artwork_document::MAX_LARGE_DESCRIPTION`). One over 64 KiB
    /// (a pattern fill's custom tile) is carried in `.rz` by word 102.
    static let maximumDescriptionBytes = 16 * 1024 * 1024

    static let information = "Editable artwork uses continuous authored sRGB and native vector-area coverage. "
        + "Intrinsic color glyphs retain RGBA8 sRGB, and bitmap monochrome glyphs retain A8 coverage when they draw ink; authored ink and opacity stay continuous. "
        + "An 8-bit document deliberately quantizes only its rendered fallback."

    /// The drawing of `description` with its colours in `ink` — the
    /// document's own (`RasterDocument.artworkInk`): the
    /// description is converted before it is captured. `reference` is the
    /// layer's reference point, which a linked pattern fill follows.
    /// `alignBox` is the box an aligned gradient fill spans
    /// (`RasterDocument.fillAlignBox`); nil spans the canvas.
    static func drawing(_ description: LayerDescription, anchor: CGPoint,
                        ink: ArtworkInk,
                        reference: (x: Int, y: Int) = (0, 0),
                        alignBox: [Int]? = nil) throws -> NativeArtworkDrawing {
        guard Thread.isMainThread, !BackgroundWork.occupied else {
            throw NativeArtworkEvaluation.failure("Prepare artwork on the main thread after the current job finishes.")
        }
        let description = description.inInk(ink)
        _ = try RasterResources.shared.admit(scratch: 192 * 1024 * 1024)
        switch description {
        case let .text(payload):
            guard !payload.string.isEmpty, payload.string.utf16.count <= 4096,
                  payload.style.isInstalled, payload.artwork?.isValid(for: payload.string) != false else {
                throw NativeArtworkEvaluation.failure("Text requires an installed font, at most 4,096 UTF-16 units, and valid continuous sRGB colors/runs.")
            }
            return try ArtworkTextCapture.capture(payload: payload, anchor: anchor)
        case let .shape(payload): return try shape(payload, anchor: anchor)
        // A fill has no anchor: it covers its canvas from (0, 0).
        case let .fill(payload):
            return try FillLayer.drawing(payload, ink: ink, reference: reference, alignBox: alignBox)
        }
    }

    static func shape(_ payload: ShapeLayerPayload, anchor: CGPoint) throws -> NativeArtworkDrawing {
        guard let json = payload.json(), ShapeLayerPayload.decode(json) != nil,
              let path = ShapeLayer.vectorPath(payload),
              let rect = payload.transform.rasterRect(of: ShapeLayer.sourceRect(payload),
                    fraction: DescribedLayer.anchorFraction(anchor)) else {
            throw NativeArtworkEvaluation.failure("Shape geometry or authored color is outside the supported range.")
        }
        let fraction = DescribedLayer.anchorFraction(anchor)
        var transform = payload.transform.cgAffine
        transform.tx = fraction.x - CGFloat(rect.originX)
        transform.ty = fraction.y - CGFloat(rect.originY)
        var parts: [NativeArtworkPart] = []
        if payload.kind != "line", let fill = payload.fillColor,
           let rgba = ArtworkColor.rgba(fill, in: payload.ink),
           let mapped = path.copy(using: &transform) { parts.append(.path(mapped, rgba: rgba)) }
        if let stroke = payload.strokeColor, payload.strokeWidth > 0,
           let rgba = ArtworkColor.rgba(stroke, in: payload.ink) {
            // A styled stroke is `ShapeLayer.strokeOutline`'s outline,
            // the one the byte renderer fills; a default stroke is unchanged.
            let stroked = ShapeLayer.strokeOutline(payload, path: path)
                ?? path.copy(strokingWithWidth: payload.strokeWidth,
                    lineCap: payload.kind == "line" ? .round : .butt, lineJoin: .miter, miterLimit: 10)
            if let mapped = stroked.copy(using: &transform) { parts.append(.path(mapped, rgba: rgba)) }
        }
        guard !parts.isEmpty else { throw NativeArtworkEvaluation.failure("The shape has no supported visible paint.") }
        return NativeArtworkDrawing(width: rect.width, height: rect.height,
            offsetX: Int(anchor.x.rounded(.down)) + rect.originX,
            offsetY: Int(anchor.y.rounded(.down)) + rect.originY, parts: parts)
    }
}

extension RasterDocument {
    /// `layer` is the target when `replacing`, and the placement ANCHOR
    /// otherwise; `position` names the slot relative to it (12E-2b). The
    /// default `.above` is the UI's decision and is what every pre-12E-2b
    /// caller meant at byte depth — the typed depth used to append a top root
    /// and now follows the same rule.
    /// `matching` (a CMYK document only, #1): the inks a colour of an sRGB
    /// description takes when its sRGB is one of theirs (`cmykArtwork`).
    func nativeArtwork(_ description: LayerDescription, anchor: CGPoint, layer: Int,
                       replacing: Bool, name: String,
                       position: LayerInsertPosition = .above,
                       matching: [CmykInkMatch] = []) throws -> RasterDocument? {
        // A CMYK document renders ink (CmykArtwork.swift).
        if isCmyk {
            return try cmykNativeArtwork(description, anchor: anchor, layer: layer, replacing: replacing,
                                         name: name, position: position, matching: matching)
        }
        // The description is published, and rendered, in the document's own
        // ink: a description from another depth is rewritten so
        // it looks the same. A fill is first sized for THIS canvas:
        // whatever canvas it was last rendered for, it covers this one.
        let ink = artworkInk
        // A Grayscale document stores each colour as its gray (#1 phase 6a-i, GrayArtwork.swift).
        let description = try grayArtwork(description.fitted(toCanvas: (width, height)).inInk(ink)
            .withOriginFraction(DescribedLayer.anchorFraction(anchor)))
        guard let json = description.json(),
              json.utf8.count <= NativeArtwork.maximumDescriptionBytes,
              let profile = iccProfile else { throw NativeArtworkEvaluation.failure("Artwork needs a bounded description and an explicit document ICC profile.") }
        // A replaced layer keeps its reference point; a new one is made at
        // (0, 0) (a linked pattern fill follows it).
        let reference = replacing ? layerReference(layer) : (x: 0, y: 0)
        // An aligned gradient fill spans the replaced layer's mask box;
        // a new layer has no mask yet, so it spans the canvas.
        let alignBox = replacing ? fillAlignBox(layer) : nil
        let evaluation = try NativeArtworkEvaluation(
            NativeArtwork.drawing(description, anchor: anchor, ink: ink, reference: reference,
                                  alignBox: alignBox),
            float: isFloat)
        let metadata = try PrecisionEditPayload.admitted(bytes: UInt64(json.utf8.count) * 2 + UInt64(profile.count) * 2 + 4096)
        let scratch = evaluation.scratchBytes + UInt64(profile.count) * 64
        return try withExtendedLifetime(metadata) {
            try PrecisionCapturedWork.run(scratchEstimate: scratch, title: "Rendering Retained Artwork") { control in
                let image = try evaluation.compute(profile: profile, control: control)
                return try control.withChildOperation(from: Double(evaluation.parts.count + 1) / Double(evaluation.parts.count + 2), to: 1) {
                    try self.publishingArtwork(image, layer: layer, replacing: replacing,
                        position: position, x: evaluation.offsetX, y: evaluation.offsetY,
                        name: name, description: json, control: $0)
                }
            }
        }
    }
}
