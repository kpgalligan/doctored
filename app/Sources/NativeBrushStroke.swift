import AppKit

// At 16 bits, unversioned typed point calls generate native dabs through the same walk
// (`NativeBrushStrokeSession.generated`: unit pressure, no timing) and rasterize through
// `dr_brush_u16`, so they refuse by name at BOTH core limits (50,000 dabs, 2 billion work units);
// the ×257 byte stroke adapter is gone. Every over-cap MCP refusal is
// `NativeBrushStroke.dabLimitRefusal` or the core's own sentence verbatim — refuse by name; no
// partial stroke. Native Quick Mask retains canonical dabs and starts work only after ImageDocument
// captures the selection/target. A CANVAS stroke is the one exception: its session runs in
// `.stopInk` mode, so a drag that reaches either cap stops laying ink (one beep) and mouse-up
// commits the dabs already laid as one undo step; dab diameters reach 5,000 px
// (`EditorTool.maximumBrushSize`). A canvas SMUDGE stroke also stops ink at Smudge's own bound
// (`maximumSmudgeWork`, the core's `MAX_SMUDGE_WORK`, 256 million): the core walks it a pixel at a
// time and the 8-bit stroke runs synchronously on the final tick.

/// RZBR version 1 records resolved deposits, including tablet pressure and
/// timer ticks. Four little-endian binary64 values per dab are x, y, diameter
/// and deposit opacity. Geometry is canvas-relative; pixel work stays in Rust.
struct NativeBrushStroke {
    static let maximumDabs = 50_000
    /// The largest dab diameter a stroke carries: `EditorTool.maximumBrushSize`'s 5,000 px.
    static let maximumDiameter = 5000.0
    /// The core's work cap for one native brush job (`brush_coverage::MAX_BRUSH_WORK`).
    static let maximumWork: UInt64 = 2_000_000_000
    /// The core's work bound for one Smudge stroke (`brush_coverage::MAX_SMUDGE_WORK`).
    static let maximumSmudgeWork: UInt64 = 256_000_000
    static let dabLimitRefusal = "This stroke exceeds the native brush limit of 50,000 dabs; no partial stroke was applied. Use a shorter stroke or wider spacing."
    let tip: BrushTip
    let pixel: Bool
    let dabs: [Double]
    /// Brush dynamics' tip shape: one `(angle_degrees, roundness)` pair per dab, which
    /// replaces the tip's for that dab; nil for every stroke without it (`tip(forDab:)`).
    let shapes: [Double]?
    /// Brush dynamics' texture (`BrushTexture.swift`), nil for every stroke without it: every
    /// rasterization of the stroke multiplies each dab's coverage by it, and `canonical`
    /// records its tile, scale and depth.
    let texture: StrokeTexture?
    /// Brush dynamics' dual brush (`BrushDual.swift`), nil for every stroke without it: every
    /// rasterization of the stroke multiplies its coverage by the second tip's before the one
    /// rounding, and `canonical` records the second tip and its dabs.
    let dual: StrokeDual?
    /// Brush dynamics' colour (`BrushColour.swift`): three values per dab, the dab's ink in the
    /// document's own unit triple; nil for every stroke without it. Every rasterization then
    /// carries each dab's ink per pixel beside the coverage (`colouredFootprint`, `coreOverlay`),
    /// and `canonical` records the list.
    let inks: [Double]?
    let payload: PrecisionEditPayload

    static func jsonNumber(_ value: Any?) -> Double? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else { return nil }
        return number.doubleValue
    }

    static func jsonBoolean(_ value: Any?) -> Bool? {
        guard let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else { return nil }
        return number.boolValue
    }

    static let shapesRefusal = "Native stroke shapes hold one angle (−180…180) and roundness "
        + "(0.01…1) per dab, on a stamped tip that is neither the pixel brush nor the Pencil; "
        + "no partial stroke was applied."

    static let textureRefusal = "A native stroke's texture holds a pattern (a built-in {preset} "
        + "or a custom tile of 1 to 512 pixels a side), a scale from 0.01 to 10 and a depth from "
        + "0 to 1, on a stamped tip that is neither the pixel brush nor the Pencil; no partial "
        + "stroke was applied."

    static let dualRefusal = "A native stroke's dual brush holds a second tip (hardness 0 to 1, "
        + "roundness 0.01 to 1, angle -180 to 180) and 1 to 50,000 dabs of diameter 1 to 5000, on "
        + "a stamped tip that is neither the pixel brush nor the Pencil; no partial stroke was "
        + "applied."

    static let inksRefusal = "A native stroke's inks hold three values from 0 to 1 per dab, on a "
        + "stamped tip that is neither the pixel brush nor the Pencil; no partial stroke was "
        + "applied."

    init(tip: BrushTip, pixel: Bool, dabs: [Double], shapes: [Double]? = nil,
         texture: StrokeTexture? = nil, dual: StrokeDual? = nil, inks: [Double]? = nil) throws {
        if texture != nil, pixel || tip.aliased {
            throw DoctoredCoreError(message: Self.textureRefusal)
        }
        if dual != nil, pixel || tip.aliased {
            throw DoctoredCoreError(message: Self.dualRefusal)
        }
        if let inks {
            guard !pixel, !tip.aliased, inks.count * 4 == dabs.count * 3,
                  inks.allSatisfy({ $0.isFinite && (0...1).contains($0) }) else {
                throw DoctoredCoreError(message: Self.inksRefusal)
            }
        }
        if let shapes {
            guard !pixel, !tip.aliased, shapes.count * 2 == dabs.count,
                  stride(from: 0, to: shapes.count, by: 2).allSatisfy({
                      (-180...180).contains(shapes[$0]) && (0.01...1).contains(shapes[$0 + 1])
                  }) else {
                throw DoctoredCoreError(message: Self.shapesRefusal)
            }
        }
        guard dabs.count / 4 <= Self.maximumDabs else {
            throw DoctoredCoreError(message: Self.dabLimitRefusal)
        }
        guard !dabs.isEmpty, dabs.count % 4 == 0,
              dabs.allSatisfy(\.isFinite), tip.hardness.isFinite, (0...1).contains(tip.hardness),
              tip.roundness.isFinite, (0.01...1).contains(tip.roundness),
              tip.angleDegrees.isFinite, (-180...180).contains(tip.angleDegrees) else {
            throw DoctoredCoreError(message: "Invalid native stroke tip or dab tuples; no partial stroke was applied.")
        }
        guard !tip.aliased || tip.hardness >= 1 else {
            throw DoctoredCoreError(message: Self.aliasedHardnessRefusal)
        }
        for i in stride(from: 0, to: dabs.count, by: 4) {
            guard abs(dabs[i]) <= 1_000_000, abs(dabs[i + 1]) <= 1_000_000,
                  (1...Self.maximumDiameter).contains(dabs[i + 2]), (0...1).contains(dabs[i + 3]) else {
                throw DoctoredCoreError(message: "Native dab coordinates, diameter or opacity are outside their supported range.")
            }
        }
        self.tip = tip; self.pixel = pixel; self.dabs = dabs; self.shapes = shapes
        self.texture = texture
        self.dual = dual
        self.inks = inks
        let doubles = dabs.count + (shapes?.count ?? 0) + (dual?.dabs.count ?? 0)
            + (inks?.count ?? 0)
        let tileBytes: UInt64 = texture?.tileBytes ?? 0
        payload = try PrecisionEditPayload.admitted(bytes: UInt64(doubles) * 8 + 4096 + tileBytes)
    }

    /// Dab `index`'s tip: the stroke's, at that dab's angle and roundness when the
    /// stroke has shapes.
    func tip(forDab index: Int) -> BrushTip {
        guard let shapes else { return tip }
        var out = tip
        out.angleDegrees = CGFloat(shapes[2 * index])
        out.roundness = CGFloat(shapes[2 * index + 1])
        return out
    }

    var request: String {
        get throws {
            try NativeSelectionWork.encoding(tipFields)
        }
    }

    /// The core request's tip: `aliased` is written only for the Pencil, so
    /// every other request keeps its earlier bytes.
    private var tipFields: [String: Any] {
        var fields: [String: Any] = ["version": 1, "hardness": Double(tip.hardness),
            "roundness": Double(tip.roundness), "angle_degrees": Double(tip.angleDegrees), "pixel": pixel]
        if tip.aliased { fields["aliased"] = true }
        return fields
    }

    static let aliasedHardnessRefusal = "An aliased (Pencil) stroke takes a hard tip: hardness must be 100."

    /// The Pencil's dab geometry: the diameter rounded to whole pixels (at
    /// least one) and the centre snapped so the dab's box lands on the pixel
    /// grid — a pixel centre for an odd diameter, a pixel corner for an even
    /// one. Every snapped dab of one diameter then covers the same pixel
    /// pattern wherever it lands, so a stroke's edge cannot shimmer as the
    /// pointer moves by a fraction of a pixel, and the canvas preview can draw
    /// the core's own dab image 1:1.
    static func aliasedGeometry(_ point: CGPoint, diameter: CGFloat) -> (point: CGPoint, diameter: CGFloat) {
        let d = diameter.isFinite ? max(1, diameter.rounded()) : 1
        let odd = Int(d) % 2 == 1
        func snap(_ v: CGFloat) -> CGFloat { odd ? floor(v) + 0.5 : v.rounded() }
        return (CGPoint(x: snap(point.x), y: snap(point.y)), d)
    }

    func healBounds(width: Int, height: Int) -> (x: Int, y: Int, width: Int, height: Int) {
        var left = Double(width), top = Double(height), right = 0.0, bottom = 0.0
        for i in stride(from: 0, to: dabs.count, by: 4) {
            let radius = dabs[i + 2] / 2 + 2
            left = min(left, dabs[i] - radius); right = max(right, dabs[i] + radius)
            top = min(top, dabs[i + 1] - radius); bottom = max(bottom, dabs[i + 1] + radius)
        }
        let x = min(width - 1, max(0, Int(floor(left))))
        let y = min(height - 1, max(0, Int(floor(top))))
        return (x, y, max(1, min(width, Int(ceil(right))) - x), max(1, min(height, Int(ceil(bottom))) - y))
    }

    var canonical: [String: Any] {
        get throws {
        _ = try RasterResources.shared.admit(scratch: UInt64(dabs.count) * 24 + 4096)
        var data = Data(); data.reserveCapacity(dabs.count * 8)
        for value in dabs {
            var bits = value.bitPattern.littleEndian
            withUnsafeBytes(of: &bits) { data.append(contentsOf: $0) }
        }
        var out: [String: Any] = ["version": 1, "encoding": "le_f64_xy_diameter_opacity",
                "dabs": data.base64EncodedString(),
                "hardness": Double(tip.hardness), "roundness": Double(tip.roundness),
                "angle_degrees": Double(tip.angleDegrees), "pixel": pixel]
        // Written only for the Pencil, so every other recorded stroke keeps its bytes.
        if tip.aliased { out["aliased"] = true }
        // Little-endian binary64 (angle, roundness) pairs, written only for a stroke with a
        // tip shape, so every other recorded stroke keeps its bytes.
        if let shapes {
            var packed = Data(); packed.reserveCapacity(shapes.count * 8)
            for value in shapes {
                var bits = value.bitPattern.littleEndian
                withUnsafeBytes(of: &bits) { packed.append(contentsOf: $0) }
            }
            out["shapes"] = packed.base64EncodedString()
        }
        // The texture's tile, scale and depth (`Texture.json`), written only for a textured
        // stroke, so every other recorded stroke keeps its bytes.
        if let texture { out["texture"] = texture.value.json }
        // The second tip and its dabs (`StrokeDual.json`), written only for a dual stroke, so
        // every other recorded stroke keeps its bytes.
        if let dual { out["dual"] = dual.json }
        // Little-endian binary64 ink triples, written only for a coloured stroke, so every
        // other recorded stroke keeps its bytes.
        if let inks { out["inks"] = Self.packed(inks) }
        return out
        }
    }

    /// The standard base64 of `values` as little-endian binary64.
    private static func packed(_ values: [Double]) -> String {
        var data = Data(); data.reserveCapacity(values.count * 8)
        for value in values {
            var bits = value.bitPattern.littleEndian
            withUnsafeBytes(of: &bits) { data.append(contentsOf: $0) }
        }
        return data.base64EncodedString()
    }

    init(canonical: [String: Any]) throws {
        let allowed: Set<String> = ["version", "encoding", "dabs", "hardness", "roundness", "angle_degrees", "pixel"]
        let aliased = canonical["aliased"].map { Self.jsonBoolean($0) }
        guard Set(canonical.keys).subtracting(["aliased", "shapes", "texture", "dual", "inks"])
                == allowed,
              aliased != .some(nil),
              Self.jsonNumber(canonical["version"]) == 1,
              canonical["encoding"] as? String == "le_f64_xy_diameter_opacity",
              let text = canonical["dabs"] as? String,
              let hardness = Self.jsonNumber(canonical["hardness"]),
              let roundness = Self.jsonNumber(canonical["roundness"]),
              let angle = Self.jsonNumber(canonical["angle_degrees"]),
              let pixel = Self.jsonBoolean(canonical["pixel"]) else {
            throw DoctoredCoreError(message: "Invalid or unsupported canonical native stroke version, encoding or fields.")
        }
        guard text.utf8.count <= Self.maximumDabs * 44 else {
            throw DoctoredCoreError(message: Self.dabLimitRefusal)
        }
        _ = try RasterResources.shared.admit(scratch: UInt64(text.utf8.count) * 3 + 4096)
        guard let data = Data(base64Encoded: text), !data.isEmpty, data.count % 32 == 0 else {
            throw DoctoredCoreError(message: "Invalid canonical dab payload length or base64 encoding.")
        }
        guard data.count / 32 <= Self.maximumDabs else {
            throw DoctoredCoreError(message: Self.dabLimitRefusal)
        }
        func doubles(_ data: Data) -> [Double] {
            var values = [Double](); values.reserveCapacity(data.count / 8)
            data.withUnsafeBytes { bytes in
                for i in stride(from: 0, to: data.count, by: 8) {
                    values.append(Double(bitPattern: UInt64(littleEndian: bytes.loadUnaligned(fromByteOffset: i, as: UInt64.self))))
                }
            }
            return values
        }
        // 16 bytes per dab: one (angle, roundness) pair.
        var shapes: [Double]?
        if let raw = canonical["shapes"] {
            guard let text = raw as? String, text.utf8.count <= Self.maximumDabs * 24,
                  let packed = Data(base64Encoded: text), packed.count == data.count / 2 else {
                throw DoctoredCoreError(message: Self.shapesRefusal)
            }
            shapes = doubles(packed)
        }
        // The recorded tile itself: a `{name}` naming the library refuses, so a replay never
        // reads it.
        let texture = try canonical["texture"].map {
            try StrokeTexture(BrushDynamics.Texture(json: $0, library: false))
        }
        let dual = try canonical["dual"].map { try StrokeDual(json: $0) }
        // 24 bytes per dab: one ink triple.
        var inks: [Double]?
        if let raw = canonical["inks"] {
            guard let text = raw as? String, text.utf8.count <= Self.maximumDabs * 33,
                  let packed = Data(base64Encoded: text), packed.count * 4 == data.count * 3 else {
                throw DoctoredCoreError(message: Self.inksRefusal)
            }
            inks = doubles(packed)
        }
        try self.init(tip: BrushTip(hardness: CGFloat(hardness),
            angleDegrees: CGFloat(angle), roundness: CGFloat(roundness),
            aliased: aliased.flatMap { $0 } ?? false), pixel: pixel, dabs: doubles(data),
            shapes: shapes, texture: texture, dual: dual, inks: inks)
    }

    /// The second tip's request and dabs as the rasterizers take them, nil without a dual.
    private var dualInput: (request: String, dabs: [Double])? {
        get throws {
            guard let dual else { return nil }
            return (try dual.request, dual.dabs)
        }
    }

    private func footprintRequest(opacity: Double = 1) throws -> String {
        var fields = tipFields
        fields["opacity"] = opacity
        return try NativeSelectionWork.encoding(fields)
    }

    /// The byte path's overlay of a stroke the core rasterizes (the Pencil's aliased tip, a
    /// texture, a dual brush): the core's coverage at `opacity` in `rgb`'s straight ink,
    /// multiplied by `selection` (bytes of the output's size), over a `width × height` output
    /// whose pixel (0, 0) is canvas `origin` (the texture's anchor; the dabs are the output's
    /// coordinates). A coloured stroke paints each pixel in its own ink (`inks`; `rgb` unread).
    /// `BytePaintRasterizer.pixels` admits it.
    func coreOverlay(width: Int, height: Int, origin: (x: Int, y: Int) = (0, 0), rgb: [UInt8],
                     selection: [UInt8]?, opacity: Double) throws -> [UInt8] {
        let context = try NativeSelectionWork.context(nil)
        return try RasterSelection.nativeBrushOverlay(width: width, height: height,
            request: footprintRequest(opacity: opacity), dabs: dabs,
            shapes: texture == nil && dual == nil && inks == nil ? nil : shapes,
            texture: texture?.handle, dual: dualInput, inks: inks, origin: origin, rgb: rgb,
            selection: selection, workingLimit: context.limit, otherRetained: context.other)
    }

    /// The core's scratch for `request` over these dabs: `dr_brush_u16_plan`'s, plus the 16
    /// bytes per dab (its two shape doubles) `dr_brush_u16_shaped` and the textured exports
    /// reserve beyond it. A texture adds nothing: its level is the handle's, built and admitted
    /// once (`StrokeTexture`), and its read is inside the plan's work. A dual brush adds the
    /// second list's own plan (`dr_brush_u16_dual` reserves the two plans' sum). A coloured
    /// stroke adds 24 bytes per output pixel, the bound of the premultiplied ink the core keeps
    /// over the footprint (`dr_brush_u16_coloured`).
    private func plannedScratch(width: Int, height: Int, request: String,
                                origin: (x: Int, y: Int) = (0, 0)) throws -> UInt64 {
        let context = try NativeSelectionWork.context(nil)
        func scratch(_ request: String, _ dabs: [Double]) throws -> UInt64 {
            let plan = try RasterSelection.nativeBrushPlan(width: width, height: height,
                request: request, dabs: Self.moved(dabs, by: origin), workingLimit: context.limit,
                otherRetained: context.other)
            return try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        }
        let second = try dualInput.map { try scratch($0.request, $0.dabs) } ?? 0
        let ink = inks == nil ? 0 : UInt64(max(width, 0)) * UInt64(max(height, 0)) * 24
        return try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.sum(scratch(request, dabs), second),
            PrecisionDocumentDescriptor.sum(UInt64(shapes?.count ?? 0) * 8, ink))
    }

    /// `dabs` (packed x, y, diameter, opacity) moved by `-origin`: the coordinates of a
    /// rect-sized output whose pixel (0, 0) is canvas `origin`.
    private static func moved(_ dabs: [Double], by origin: (x: Int, y: Int)) -> [Double] {
        guard origin != (0, 0) else { return dabs }
        var out = dabs
        for i in stride(from: 0, to: out.count - 3, by: 4) {
            out[i] -= Double(origin.x); out[i + 1] -= Double(origin.y)
        }
        return out
    }

    /// The core's scratch estimate for `footprint`, planned on main.
    func footprintPlan(width: Int, height: Int) throws -> UInt64 {
        try plannedScratch(width: width, height: height, request: footprintRequest())
    }

    /// The core's scratch estimate for `footprint(in:)`, planned on main.
    func footprintPlan(in rect: CoverageRect) throws -> UInt64 {
        try plannedScratch(width: rect.width, height: rect.height, request: footprintRequest(),
                           origin: (rect.x, rect.y))
    }

    /// The stroke's u16 footprint at opacity 1, under the caller's control
    /// with no modal wait: a job's worker calls it.
    func footprint(width: Int, height: Int, control: RasterJobControl) throws -> [UInt16] {
        let context = try NativeSelectionWork.context(nil)
        return try RasterSelection.nativeBrush(width: width, height: height, request: footprintRequest(),
            dabs: dabs, shapes: shapes, texture: texture?.handle, dual: dualInput,
            workingLimit: context.limit, otherRetained: context.other, control: control)
    }

    /// A coloured stroke's u16 footprint at opacity 1 and its three ink planes, each the
    /// straight ink `round(S · 65535)` in the document's own unit triple
    /// (`dr_brush_u16_coloured`): the typed commit's `ink_planes`. Under the caller's control,
    /// as `footprint` is.
    func colouredFootprint(width: Int, height: Int,
                           control: RasterJobControl) throws -> (coverage: [UInt16], ink: [UInt16]) {
        guard let inks else { throw DoctoredCoreError(message: Self.inksRefusal) }
        let context = try NativeSelectionWork.context(nil)
        return try RasterSelection.nativeBrushColoured(width: width, height: height,
            request: footprintRequest(), dabs: dabs, shapes: shapes, texture: texture?.handle,
            dual: dualInput, inks: inks, workingLimit: context.limit,
            otherRetained: context.other, control: control)
    }

    /// `footprint` over the canvas rect `rect` only (13B-9c, a retouch's `coverage_rect`):
    /// rect-sized, the dabs of both tips moved by its origin and the texture anchored there,
    /// which is the canvas footprint cropped to the rect (`core/tests/brush_rect_footprint.rs`).
    func footprint(in rect: CoverageRect, control: RasterJobControl) throws -> [UInt16] {
        let context = try NativeSelectionWork.context(nil)
        let origin = (x: rect.x, y: rect.y)
        let second = try dualInput.map { (request: $0.request, dabs: Self.moved($0.dabs, by: origin)) }
        return try RasterSelection.nativeBrush(width: rect.width, height: rect.height,
            request: footprintRequest(), dabs: Self.moved(dabs, by: origin), shapes: shapes,
            texture: texture?.handle, dual: second, origin: origin,
            workingLimit: context.limit, otherRetained: context.other, control: control)
    }

    func coverage(width: Int, height: Int, selection: CanvasSelection?, opacity: Double = 1) throws -> PrecisionScalarBuffer {
        let request = try footprintRequest(opacity: opacity)
        let scratch = try plannedScratch(width: width, height: height, request: request)
        let second = try dualInput
        let samples = try PrecisionCapturedWork.run(scratchEstimate: scratch, title: "Native Brush Coverage") { control in
            let context = try NativeSelectionWork.context(nil)
            return try RasterSelection.nativeBrush(width: width, height: height, request: request,
                dabs: dabs, shapes: shapes, texture: texture?.handle, dual: second,
                workingLimit: context.limit, otherRetained: context.other, control: control)
        }
        let footprint = try PrecisionScalarBuffer(samples: samples, width: width, height: height, provenance: .nativeU16)
        if let selection {
            let selected = try PrecisionStrokeCoverage.selection(width: width, height: height, selection: selection)
            let input = try PrecisionScalarBuffer.joining([footprint, selected])
            let result = try NativeSelectionWork(width: width, height: height,
                operation: ["kind": "blend", "channels": 1, "mode": Int(DR_BLEND_MULTIPLY.rawValue), "opacity": 1],
                input: input.samples).run(title: "Clip Native Stroke")
            return try PrecisionScalarBuffer(samples: result, width: width, height: height, provenance: .nativeU16)
        }
        return footprint
    }
}

/// One value session on the canvas. Actual stamps append the resolved diameter
/// and deposit, so replay never approximates pressure or reconstructs timing.
struct NativeBrushStrokeSession {
    /// What a stamp past a cap does. `.refuse` (the MCP walk, the default)
    /// refuses the whole stroke by name. `.stopInk` (a canvas stroke)
    /// drops that stamp and every later one and sets `isFull`, so the dabs
    /// already laid still resolve and commit; `canvasPixels` is the extent the
    /// commit plans over.
    enum Overflow {
        case refuse
        case stopInk(canvasPixels: Int)
    }

    let tip: BrushTip
    let size: CGFloat
    let pressureOpacity: Bool
    let strokeOpacity: Double
    let erasing: Bool
    let overflow: Overflow
    /// A Smudge stroke: in `.stopInk` mode a stamp is also charged
    /// against `NativeBrushStroke.maximumSmudgeWork` (see `smudgeCharge`).
    let smudging: Bool
    /// A Background Eraser stroke: in `.stopInk` mode a stamp is also charged
    /// against the core's bound for it (`background_eraser::plan`).
    let backgroundErasing: Bool
    let reservation: PrecisionEditPayload?
    var dabs: [Double] = []
    var refusal: String?
    /// `.stopInk` only: a cap was reached; later stamps are dropped.
    private(set) var isFull = false
    /// `.stopInk` only: the work charged so far (see `workCharge`), both lists' with a dual brush.
    private(set) var usedWork: UInt64 = 0
    /// `.stopInk` and `smudging` only: the Smudge work charged so far.
    private var usedSmudgeWork: UInt64 = 0
    /// `.stopInk` and `backgroundErasing` only: the Background Eraser work charged so far.
    private var usedBackgroundWork: UInt64 = 0
    /// `.stopInk` and `smudging` only: the last dab laid and its `workCharge`.
    private var lastSmudgeDab: (point: CGPoint, charge: UInt64)?
    /// An aliased tip only: the last snapped dab, which a repeat is dropped against.
    private var lastAliasedDab: (point: CGPoint, diameter: CGFloat)?
    /// The stroke's brush dynamics, nil for none: each nominal stamp then resolves
    /// through the core (`NativeBrushStrokeSession+Dynamics.swift`).
    var dynamics: RasterBrushDynamics?
    /// `dynamics` only: the nominal stamps laid so far and the last one's point,
    /// which give the next stamp its index and its direction of travel.
    var dynamicsIndex: UInt64 = 0
    var lastNominalPoint: CGPoint?
    /// `dynamics` with a tip shape only: one `(angle, roundness)` pair per laid dab
    /// (`shapes.count == dabs.count / 2`), which `resolved()` hands to the stroke.
    var dynamicsShaped = false
    var shapes: [Double] = []
    /// `dynamics` only: the last non-zero move between nominal stamps, the direction of
    /// travel a tip shape's angle reads; zero until the stroke first moves.
    var lastHeading = CGVector.zero
    /// `dynamics` with a texture only (the Brush and the Eraser): the texture `resolved()`
    /// hands to the stroke, which the canvas preview also draws each dab through
    /// (`ImageCanvasView+BrushTexture.swift`).
    var texture: StrokeTexture?
    /// `dynamics` with a dual brush only (the Brush and the Eraser): the second tip's walk and
    /// tip (`attachDual`), the second dabs laid so far, the walk's anchor (nil until the first
    /// nominal stamp feeds it) and the next second stamp's index
    /// (`NativeBrushStrokeSession+Dual.swift`).
    private(set) var dual: (handle: RasterBrushDual, tip: BrushTip)?
    var dualDabs: [Double] = []
    var dualAnchor: CGPoint?
    var dualIndex: UInt64 = 0
    /// `dynamics` with colour dynamics only (the Brush on a layer, `BrushColour.swift`): the
    /// handle each laid dab's ink comes from, and the inks laid so far (three per dab,
    /// `inks.count == dabs.count / 4 * 3`), which `resolved()` hands to the stroke.
    var colour: RasterBrushColour?
    var inks: [Double] = []

    /// Whether a stamp would still lay ink: no refusal and no cap reached.
    var acceptsInk: Bool { refusal == nil && !isFull }
    private var fullNoticed = false

    /// True exactly once after the session turns full: the canvas's one beep.
    mutating func takeFullNotice() -> Bool {
        guard isFull, !fullNoticed else { return false }
        fullNoticed = true
        return true
    }

    init(tip: BrushTip, size: CGFloat, pressureOpacity: Bool,
         strokeOpacity: Double = 1, erasing: Bool = false, overflow: Overflow = .refuse,
         smudging: Bool = false, backgroundErasing: Bool = false) {
        self.tip = tip; self.size = size; self.pressureOpacity = pressureOpacity
        self.strokeOpacity = strokeOpacity; self.erasing = erasing; self.overflow = overflow
        self.smudging = smudging; self.backgroundErasing = backgroundErasing
        // The core's plan charges `n + |F|` up front, and `|F| ≤ n`; the Background
        // Eraser's charges `|F|`.
        if case .stopInk(let canvasPixels) = overflow {
            usedWork = 2 * UInt64(max(canvasPixels, 0))
            if backgroundErasing { usedBackgroundWork = UInt64(max(canvasPixels, 0)) }
        }
        do { reservation = try PrecisionEditPayload.admitted(bytes: UInt64(NativeBrushStroke.maximumDabs) * 64 + 4096) }
        catch { reservation = nil; refusal = error.localizedDescription }
    }

    /// One dab's work in `.stopInk` mode, never below what the core's plan
    /// charges for it (masters: `brush_coverage::plan` and `band_bound`): the
    /// clipped box is at most `s²` with `s = ⌈d⌉ + 2`, and a hard dab
    /// (hardness ≥ 1, not the size-1 pixel brush) adds 64 per band pixel,
    /// `⌈4πaρ + πρ²⌉` of them with `a = d/2` and `ρ = √2 + 2⁻¹⁶·a`.
    static func workCharge(diameter: CGFloat, tip: BrushTip, size: CGFloat) -> UInt64 {
        let d = Double(diameter)
        guard d.isFinite, d >= 0, d <= 1_000_000 else { return NativeBrushStroke.maximumWork }
        let side = UInt64(d.rounded(.up)) + 2
        var charge = side * side
        if tip.hardness >= 1, size > 1 {
            let a = d / 2
            let rho = 2.0.squareRoot() + a / 65536
            charge += 64 * UInt64((4 * Double.pi * a * rho + Double.pi * rho * rho).rounded(.up))
        }
        return charge
    }

    /// The Smudge work of the move from the last dab laid to a dab at `point`
    /// whose `workCharge` is `charge`, never below what the core charges for
    /// it (master: `retouch_smudge::smudge_work`): the move is walked in
    /// `m = max(1, ⌈distance⌉)` sub-steps, each a dab whose diameter lies
    /// between the two dabs', charged its dab work + 1. 0 for the first dab,
    /// which only anchors. nil when the figure does not fit.
    private func smudgeCharge(to point: CGPoint, charge: UInt64) -> UInt64? {
        guard let last = lastSmudgeDab else { return 0 }
        let distance = Double(hypot(point.x - last.point.x, point.y - last.point.y))
        guard distance.isFinite, distance <= Double(NativeBrushStroke.maximumSmudgeWork) else {
            return nil
        }
        let steps = max(1, UInt64(distance.rounded(.up)))
        let (product, overflow) = steps.multipliedReportingOverflow(by: max(last.charge, charge) + 1)
        return overflow ? nil : product
    }

    mutating func stamp(at point: CGPoint, diameter: CGFloat, pressure: CGFloat) {
        stamp(at: point, diameter: diameter,
              deposit: tip.flow * (pressureOpacity ? min(max(pressure, 0), 1) : 1))
    }

    /// One dab at `deposit` (the tip's flow times the stamp's opacity factor),
    /// through the caps and the Pencil's snap.
    mutating func stamp(at point: CGPoint, diameter: CGFloat, deposit: CGFloat) {
        guard acceptsInk else { return }
        var point = point, diameter = diameter
        if tip.aliased {
            // The Pencil lays snapped dabs (`NativeBrushStroke.aliasedGeometry`),
            // and one that repeats the dab before it is dropped: it would paint
            // the same pixels again, building up a pressure-opacity stroke
            // wherever the hand slowed.
            (point, diameter) = NativeBrushStroke.aliasedGeometry(point, diameter: diameter)
            if let last = lastAliasedDab, last.point == point, last.diameter == diameter { return }
            lastAliasedDab = (point, diameter)
        }
        switch overflow {
        case .refuse:
            guard dabs.count / 4 < NativeBrushStroke.maximumDabs else {
                refusal = NativeBrushStroke.dabLimitRefusal
                return
            }
        case .stopInk:
            let charge = Self.workCharge(diameter: diameter, tip: tip, size: size)
            guard dabs.count / 4 < NativeBrushStroke.maximumDabs,
                  usedWork <= NativeBrushStroke.maximumWork,
                  charge <= NativeBrushStroke.maximumWork - usedWork else {
                isFull = true
                return
            }
            if smudging {
                guard let move = smudgeCharge(to: point, charge: charge),
                      usedSmudgeWork <= NativeBrushStroke.maximumSmudgeWork,
                      move <= NativeBrushStroke.maximumSmudgeWork - usedSmudgeWork else {
                    isFull = true
                    return
                }
                usedSmudgeWork += move
                lastSmudgeDab = (point, charge)
            }
            if backgroundErasing {
                // The core charges a dab its tip work plus its clipped box (the match
                // and the walk), against Smudge's bound (master:
                // `background_eraser::plan`); `charge` bounds the first, `s²` the second.
                let side = UInt64(Double(max(diameter, 0)).rounded(.up)) + 2
                let erase = charge + side * side
                guard usedBackgroundWork <= NativeBrushStroke.maximumSmudgeWork,
                      erase <= NativeBrushStroke.maximumSmudgeWork - usedBackgroundWork else {
                    isFull = true
                    return
                }
                usedBackgroundWork += erase
            }
            usedWork += charge
        }
        dabs.append(contentsOf: [Double(point.x), Double(point.y), Double(diameter), Double(deposit)])
    }

    /// Gives the session its dual brush (nil: none). In `.stopInk` mode the second list's plan
    /// charges its own `n + |F|` up front, as `init` charges the first's.
    mutating func attachDual(_ dual: (handle: RasterBrushDual, tip: BrushTip)?) {
        self.dual = dual
        guard dual != nil, case .stopInk(let canvasPixels) = overflow else { return }
        usedWork += 2 * UInt64(max(canvasPixels, 0))
    }

    /// The caps for one second dab of `diameter`: true when it may be laid. `.refuse` refuses the
    /// stroke by name past 50,000 second dabs; `.stopInk` also charges the dab's work against the
    /// one budget both lists share and turns the session full when either cap is reached. The
    /// charge is `workCharge` at `size: 2`, whatever the second tip's size: the second request is
    /// never the pixel brush, so a hard second tip is charged its band term at every diameter, 1
    /// included, as the core charges a classified dab (master: `brush_coverage::dab_work`), and a
    /// stroke that stopped ink is never refused whole at mouse-up by the coverage export's
    /// admission (`ffi_brush_dual`'s `dual_request`, whose work sum covers both lists).
    mutating func chargeDual(diameter: CGFloat) -> Bool {
        guard let dual, acceptsInk else { return false }
        switch overflow {
        case .refuse:
            guard dualDabs.count / 4 < NativeBrushStroke.maximumDabs else {
                refusal = NativeBrushStroke.dabLimitRefusal
                return false
            }
        case .stopInk:
            let charge = Self.workCharge(diameter: diameter, tip: dual.tip, size: 2)
            guard dualDabs.count / 4 < NativeBrushStroke.maximumDabs,
                  usedWork <= NativeBrushStroke.maximumWork,
                  charge <= NativeBrushStroke.maximumWork - usedWork else {
                isFull = true
                return false
            }
            usedWork += charge
        }
        return true
    }

    mutating func pixelSegment(from start: CGPoint, to end: CGPoint, fromPressure: CGFloat = 1, toPressure: CGFloat = 1) {
        let distance = hypot(end.x - start.x, end.y - start.y)
        guard distance.isFinite, distance <= CGFloat(NativeBrushStroke.maximumDabs) else {
            if case .stopInk = overflow { isFull = true } else { refusal = NativeBrushStroke.dabLimitRefusal }
            return
        }
        let steps = max(1, Int(ceil(distance)))
        for i in 1...steps {
            let t = CGFloat(i) / CGFloat(steps)
            stamp(at: CGPoint(x: floor(start.x + (end.x - start.x) * t) + 0.5,
                y: floor(start.y + (end.y - start.y) * t) + 0.5), diameter: 1,
                pressure: fromPressure + (toPressure - fromPressure) * t)
            if !acceptsInk { return }
        }
    }

    func resolved() throws -> NativeBrushStroke {
        if let refusal { throw DoctoredCoreError(message: refusal) }
        let second = try dual.map { try StrokeDual(tip: $0.tip, dabs: dualDabs) }
        return try NativeBrushStroke(tip: tip, pixel: size <= 1, dabs: dabs,
                                     shapes: dynamicsShaped ? shapes : nil, texture: texture,
                                     dual: second, inks: colour == nil ? nil : inks)
    }
}

extension NativeBrushStrokeSession {
    /// The generated MCP walk: public points, pressures and timer ticks
    /// resolved to canonical dabs. Versioned calls and unversioned 16-bit
    /// typed point strokes share this one walk.
    /// With `dynamics`, each nominal stamp resolves through it (`stamp(at:pen:)`) and the
    /// walk spaces by its nominal diameter; the pen sample's pressure is NaN unless
    /// `pressureSampled`, its tilt NaN unless `tilts` and its rotation NaN unless
    /// `rotations` (one per point; interpolated on the shortest arc) is given. `shaped`
    /// records each dab's tip shape and `texture` is the stroke's. The airbrush ticks are laid
    /// after the whole walk, so each passes its own heading (`tickHeading`) rather than its
    /// jump back along the path. `dual` (beside `dynamics` only) walks the spacing stamps'
    /// points; a tick is a deposit at rest, not a point of the path, so it never feeds it.
    /// `colour` (beside `dynamics` only) gives every laid dab its ink, ticks included.
    static func generated(points: [CGPoint], size: CGFloat, tip: BrushTip, pressures: [Double], times: [Double],
                          duration: Double, pressureSize: Bool, pressureOpacity: Bool, airbrush: Bool,
                          dynamics: RasterBrushDynamics? = nil, pressureSampled: Bool = false,
                          tilts: [CGVector]? = nil, shaped: Bool = false,
                          rotations: [Double]? = nil, texture: StrokeTexture? = nil,
                          dual: (handle: RasterBrushDual, tip: BrushTip)? = nil,
                          colour: RasterBrushColour? = nil) throws -> NativeBrushStroke {
        let size = Double(size)
        var session = NativeBrushStrokeSession(tip: tip, size: CGFloat(size), pressureOpacity: pressureOpacity)
        session.dynamics = dynamics
        session.dynamicsShaped = shaped && dynamics != nil
        session.texture = texture
        session.attachDual(dynamics == nil ? nil : dual)
        session.colour = dynamics == nil ? nil : colour
        func diameter(_ pressure: Double) -> CGFloat { CGFloat(pressureSize ? max(1, size * pressure) : size) }
        var tilt = tilts?.first ?? StrokeVertex.noTilt
        var rotation = CGFloat(rotations?.first ?? .nan)
        func pen(_ pressure: Double) -> PenSample {
            PenSample(pressure: pressureSampled ? pressure : .nan, tiltX: Double(tilt.dx), tiltY: Double(tilt.dy),
                      rotation: Double(rotation))
        }
        func spacing(_ pressure: Double) -> CGFloat {
            SoftBrush.spacing(for: session.dynamics?.nominalDiameter(size: CGFloat(size), pen: pen(pressure))
                ?? diameter(pressure), percent: tip.spacingPercent)
        }
        func lay(_ point: CGPoint, _ pressure: Double, heading: CGVector? = nil, tick: Bool = false) {
            if session.dynamics != nil {
                session.stamp(at: point, pen: pen(pressure), heading: heading, tick: tick)
            } else {
                session.stamp(at: point, diameter: diameter(pressure), pressure: CGFloat(pressure))
            }
        }
        func interpolated(_ from: CGVector, toward i: Int, by fraction: CGFloat) -> CGVector {
            guard let tilts else { return from }
            return CGVector(dx: from.dx + (tilts[i].dx - from.dx) * fraction, dy: from.dy + (tilts[i].dy - from.dy) * fraction)
        }
        func turned(_ from: CGFloat, toward i: Int, by fraction: CGFloat) -> CGFloat {
            guard let rotations else { return from }
            return StrokeVertex.mixRotation(from, CGFloat(rotations[i]), fraction)
        }
        // A tick's heading: the `times` segment it lies on, or the nearest earlier segment of
        // `points` with length when that one has none; zero (the tip's own angle) without one.
        func tickHeading(_ segment: Int) -> CGVector {
            var k = min(segment, points.count - 2)
            while k >= 0 {
                let leg = CGVector(dx: points[k + 1].x - points[k].x, dy: points[k + 1].y - points[k].y)
                if leg.dx != 0 || leg.dy != 0 { return leg }
                k -= 1
            }
            return .zero
        }
        lay(points[0], pressures[0])
        var anchor = points[0], pressure = pressures[0]
        for i in points.indices.dropFirst() {
            if size <= 1 {
                session.pixelSegment(from: anchor, to: points[i], fromPressure: CGFloat(pressure), toPressure: CGFloat(pressures[i]))
                anchor = points[i]; pressure = pressures[i]; continue
            }
            var distance = hypot(points[i].x - anchor.x, points[i].y - anchor.y)
            var step = spacing(pressure)
            while distance >= step, session.refusal == nil {
                let fraction = step / distance
                anchor.x += (points[i].x - anchor.x) * fraction
                anchor.y += (points[i].y - anchor.y) * fraction
                pressure += (pressures[i] - pressure) * Double(fraction)
                tilt = interpolated(tilt, toward: i, by: fraction)
                rotation = turned(rotation, toward: i, by: fraction)
                lay(anchor, pressure)
                distance = hypot(points[i].x - anchor.x, points[i].y - anchor.y)
                step = spacing(pressure)
            }
            if session.refusal != nil { break }
        }
        if airbrush, duration >= 0.05 {
            var segment = 0
            for tick in 1...Int(floor(duration / 0.05 + 1e-9)) {
                let time = Double(tick) * 0.05
                while segment + 1 < times.count, times[segment + 1] <= time { segment += 1 }
                var point = points[segment], p = pressures[segment]
                tilt = tilts?[segment] ?? StrokeVertex.noTilt
                rotation = CGFloat(rotations?[segment] ?? .nan)
                if segment + 1 < times.count, times[segment + 1] > times[segment] {
                    let t = min(1, max(0, (time - times[segment]) / (times[segment + 1] - times[segment])))
                    point.x += (points[segment + 1].x - point.x) * t
                    point.y += (points[segment + 1].y - point.y) * t
                    p += (pressures[segment + 1] - p) * t
                    tilt = interpolated(tilt, toward: segment + 1, by: CGFloat(t))
                    rotation = turned(rotation, toward: segment + 1, by: CGFloat(t))
                }
                lay(point, p, heading: tickHeading(segment), tick: true)
                if session.refusal != nil { break }
            }
        }
        return try session.resolved()
    }

    static func generated(points: [CGPoint], size: CGFloat, tip: BrushTip) throws -> NativeBrushStroke {
        try generated(points: points, size: size, tip: tip, pressures: Array(repeating: 1, count: points.count),
                      times: Array(repeating: 0, count: points.count), duration: 0,
                      pressureSize: false, pressureOpacity: false, airbrush: false)
    }
}
