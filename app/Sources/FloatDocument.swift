import AppKit

// `ToneMap` is the one vocabulary the menu, the sheet, the MCP tools, Actions and the archive
// speak, but the CORE owns every rule — the handler forwards `white` VERBATIM so `clip` + `white`
// reaches the core's refusal instead of being dropped host-side. No new guard was added: a float
// document answers `linearLightRefusal()` with its own narrower table, so the two guards 11E
// installed already refuse an unadmitted float edit. `RasterDocument.display(of:context:)` is the
// ONE place a typed projection becomes display bytes — a float document goes through
// `dr_precision_doc_display_rgba8` (map → TRC → quantize) and every other through `previewRGBA8`,
// because `dr_precision_image_preview_rgba8` now REFUSES float storage. `DocumentPrecision.refusal`
// asks `FloatDocument.tools` FIRST and then the core's depth-32 capability table,
// `NativeLayerBlending.floatModes` limits blending (layer and Fill/Stroke ink alike) to the five
// kernels with no clamp of their own plus the nine modes measured and built and Linear Burn,
// Linear Light, Pin Light and Divide, and a group may be Pass Through (Group Layers asks for it: a
// new 32-bit group is Pass Through), the editor's selector whitelist and tool rail admit only the
// selection/navigation/eyedropper tools plus Move and Crop, and the filtered geometry (Image Size,
// straighten, arbitrary rotation, Free Transform and its Copy, Distort, Transform Mask, Transform
// Again with no selection; `EvaluationOperators.resizeRefusal` answers nil for float) and
// Layer ▸ Warp… (`showPersistedWarp:`, MCP `warp_layer`/`get_warp`, family `geometry_warp` in
// `floatCoreRefusal`), and the Export sheet's size modes at 32 bits (format, profile and
// sharpen stay pinned and shown disabled), and the Info panel's readouts become linear components
// with stops plus a stop-binned histogram whose endpoint markers report below-zero and above-white,
// for Composite, Active layer and Below active layer with selection weighting, through
// `ScopedAnalysisRead` like every other depth; MCP `histogram` reaches `scopedHistogram` at
// every depth, and its `source: "scalar"` measures a mask or channel while the core refuses
// red/green/blue/alpha by name. A float document holds layer styles, Blend If and the global
// light: the Layer Style sheet offers `effectModes` in its blend popups and
// seeds a new effect with `effectDefaultModes` (the 32-bit defaults), both read from the
// core's operator table, and the core refuses an effect mode outside its float set by name.
// The float adjustment layers are Levels, Brightness/Contrast/Saturation, Black & White (Tint
// included), Channel Mixer and Photo Filter beside Exposure and White Balance (also
// Hue/Saturation's Colorize, master Lightness alone or master Hue alone, `floatName`, Color
// Balance, Curves in map (pencil) form beside the point form, and Color Lookup for a 3D table with
// a 0…1 domain and entries in 0…1, later widened to every Hue/Saturation setting and every Color
// Lookup table), each under its own form in the core (`adjust_float`), measured against
// Photoshop's 32-bit renders; `adjustmentOps`, the limitation sentence and `get_precision` read
// the list from the core's operator table, and every other adjustment refuses by name in the core.
// Image ▸ Adjustments applies `destructiveOps` (the table's `*_apply` rows) to a layer's pixels
// under the same forms: the menu items and MCP `apply_filter` admit only those, on the layer.
// A float document takes placed images: Place Embedded/Linked and Place Live
// Photo are whitelisted and their MCP twins are `.floatTool`. The core renders the placed file's
// 8-, 16- or 32-bit samples into linear light (converted to the document profile first, then
// resampled), and since 2026-10-10 the placed layer is an ordinary float raster: only a camera
// RAW layer keeps a retained source.
// A float document holds text and shape layers: the Type and Shape tools,
// re-editing, Artwork Colors and Rasterize Artwork are admitted (`DocumentPrecision.allows`, the
// selector whitelist) and their MCP twins are `.floatTool`; the core renders through
// `dr_artwork_render_float`, the colour numbers being linear light at 32 bits as Photoshop stores
// them (description v4, `ArtworkInk`), and a description pasted in or converted out is rewritten
// so it looks the same.
// A camera RAW layer placed or pasted into a float
// document keeps its original and its u16 development, which is rendered into linear light as a
// placed image is: Layer ▸ Redevelop RAW…, RAW Source…, Migrate RAW Decoder…, RAW Presets… and
// Synchronize RAW Settings… are whitelisted, and the 13 document RAW tools (redevelopment,
// presets and settings, decoder migration, locals) are `.floatTool`. A placed RAW file lands at
// 32 bits as its SDR development (`float-raw-602/take2`, R-SDR by the controller's ruling).
// The Brush, Eraser and Gradient tools and `brush_stroke`, `eraser_stroke` and
// `gradient` paint a float layer in linear light, and its mask or an alpha channel as at 16 bits
// (`FloatPaint.swift`).
// As display-defined tools (`designs/v1-float-editing.md`; the core's
// `display_encoding` states each rule) the Paint Bucket (`fill`), Dodge and Burn
// (`dodge_burn`, on a layer and, by `target`, on its mask or an alpha channel) and the Sponge
// (`sponge_stroke`) work in a float document, and so do the Magic Wand, Quick Selection, Select ▸
// Color Range…, Grow and Similar (`select_magic_wand`, `quick_select`, `select_color_range`,
// `select_similar`): tolerances and colour distances compare the display-encoded copy (the
// profile's curve over each value clamped to 0…1, never the tone map), and the change is made in
// linear light. Select and Mask's edge stage (Refine Edge, Quick Selection's Enhance Edge, Object
// Selection's refinement) reads the same display codes, and the Magnetic Lasso traces them at 8
// bits (`ImageDocument.edgeTraceComposite`); Select and Mask's New Layer outputs keep the float
// composite's bits; Decontaminate Colors refuses by name, as does the Sponge on a mask or
// channel. Linear Burn, Linear Light, Pin Light and Divide blend on layers, ink and
// effects; the nine modes that invert about white stay refused. Every adjustment but Invert,
// Shadows/Highlights and the legacy White Balance runs as a layer and as Image ▸ Adjustments:
// Hue Rotate, Grayscale and Sepia in closed form, Threshold, Posterize and Gradient Map through
// the display curve, Vibrance, Selective Color, Replace Color and every Hue/Saturation setting
// scaled about white, and every Color Lookup table on the stored values.
// A float document also runs Apply Layer Mask (alpha · coverage in f32), Layer Via
// Copy / Cut, Assign Profile (the linear samples relabelled) and Convert to Profile (the matrix
// between the two profiles' primaries, no clamp; `float_color` in the core), Calculations, Add
// Luminosity Masks and Contact Sheet: their selectors are whitelisted and their MCP twins are
// `.floatTool`. Apply Image, a profile with no matrix/TRC model, and a Convert with absolute
// intent, black-point compensation or preserve-appearance stay refused by name (the core's
// operator table carries each reason).
// The Clone Stamp, Healing Brush, Spot Healing Brush, Patch, Remove and
// Content-Aware Move tools, Content-Aware Fill and Stroke Path work on a float layer's pixels
// (`FloatRetouch.swift`, `designs/v1-float-editing.md`).
// Select ▸ Subject, Select ▸ Objects…, Select ▸ People…, Select ▸ All People, Select ▸
// Face Parts…, the Subject tool and the Object Selection tool run in a float document, and
// `select_subject`, `inspect_subjects` and `select_subjects` (either `model`, "objects" or
// "people"), `select_people`, `inspect_faces`, `select_face_parts` and `object_select` are
// `.floatTool`. Vision reads the
// document's tone-mapped 8-bit display at view exposure 0 (`ImageDocument.analysisComposite(of:)`)
// and its byte coverage is the selection, as at 16 bits. Object Selection's edge stage is opt-in
// at float (`ObjectSelection.refines` is its default): it reads the display codes, where an
// above-white subject on an above-white background is flat white. Select ▸ Sky,
// Select ▸ Person Parts… and the Object Selection tool's Object Finder (`select_sky`,
// `select_person_parts`, `object_finder_select`, each `.floatTool`) run their models on the same
// display copy, and their edge stage always runs: with Smart Radius it leaves flat display codes
// untouched.
// Twenty-five filters run on a float layer's own pixels through the kernels
// the other depths run (`float_filter_apply` in the core), each under one stated rule: eleven in
// linear light with no clamp to 0…1, Pixelate and Add Noise in linear light too, and the twelve
// with a display-referred parameter (Surface Blur, Dust & Scratches, Reduce Noise, Add Grain,
// Clouds, Difference Clouds, Emboss, Edge Detect, Sharpen, Presence, the Develop Filter, Smart
// Sharpen) on the layer's display-encoded copy, decoded back. `filterOps` reads them from the operator
// table's `*_filter` rows, `filterActions` names their Filter menu items, and `apply_filter`
// and `liquify` answer over MCP. Every control of Presence and the Develop Filter runs, Dehaze,
// Highlights, Shadows, Vibrance and Saturation included (each rule in its core module); Red Eye
// runs too.
// Auto Tone, Auto Contrast, Auto Color and the Levels Auto buttons stay disabled at
// 32 bits, and `refusal(tool:)` words their MCP refusal from `autoRow`.
// Every filter of `planeFilterOps` (all but `layerOnlyFilters`) runs on a layer
// mask, an alpha channel or one colour channel too, through the same kernels on a one-channel plane
// (`float_plane_filter` in the core): coverage clamped to 0…1, a colour channel unclamped.
//
// `designs/phase-11f-hdr-float-shipped.md` is the contract; `designs/v1-float-editing.md` holds
// what shipped after it.

/// The 32-bit float document mode (Phase 11F) as the host speaks it.
///
/// A float document's rasters are straight RGBA32 float in LINEAR LIGHT with
/// an EXTENDED range: any finite value with `|v| ≤ 65504`, above white and
/// below zero alike. It is `linear_light` by construction and cannot be
/// `encoded`, and it admits a bounded operator set — the
/// `NativeLayerBlending.floatModes` blending, layer properties, masks and channels, the
/// `FloatDocument.adjustmentOps` adjustment layers, Edit ▸ Fill… and Stroke… on a layer,
/// its mask, an alpha channel or one colour channel, the readouts, render, convert,
/// export and the tone map.
/// Everything else refuses BY NAME through `float_document::float_refusal`,
/// which `linear_light_refusal` also returns, so the two guards Phase 11E
/// installed (`ImageDocument.applyEdit` and `AgentServer.performGroupedEdit`)
/// already cover float edits with no new guard anywhere.
///
/// Every `dr_*` call behind this lives in `DoctoredCore+Float.swift`; the
/// policy itself lives in the core.
enum ToneMapOperator: String, CaseIterable {
    case clip
    case reinhard

    var displayName: String {
        switch self {
        case .clip: return "Clip to white"
        case .reinhard: return "Reinhard"
        }
    }

    /// What the sheet says the choice means.
    var summary: String {
        switch self {
        case .clip:
            return "Everything at or above white becomes white. Nothing above the range is shown."
        case .reinhard:
            return "A curve that maps the white point exactly to white and compresses "
                + "everything below it, so highlights above 1.0 stay distinguishable."
        }
    }

    /// True when the white point applies. `white` is refused on `clip` by the
    /// core, which is why the handler forwards it verbatim.
    var usesWhite: Bool { self == .reinhard }
}

/// One tone map, the wire shape `float_document::ToneMap` parses.
struct ToneMap: Equatable {
    var `operator`: ToneMapOperator
    /// Meaningful for `reinhard` only; the core's default is 4.0.
    var white: Double

    static let defaultWhite = 4.0
    static let whiteRange = 1.0...65504.0

    init(operator op: ToneMapOperator = .clip, white: Double = ToneMap.defaultWhite) {
        self.operator = op
        self.white = white
    }

    /// The reply the core gives back. A map this build does not know is an
    /// error, never a silent default.
    init(json: [String: Any]) throws {
        guard let name = json["operator"] as? String,
              let op = ToneMapOperator(rawValue: name) else {
            throw DoctoredCoreError(message: "Unknown tone map operator.")
        }
        self.operator = op
        white = (json["white"] as? NSNumber)?.doubleValue ?? ToneMap.defaultWhite
    }

    init(text: String) throws {
        guard let value = try JSONSerialization.jsonObject(with: Data(text.utf8))
                as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid tone map description.")
        }
        try self.init(json: value)
    }

    /// `white` only where it means something, exactly as the core writes it.
    var json: [String: Any] {
        self.operator.usesWhite
            ? ["operator": self.operator.rawValue, "white": white]
            : ["operator": self.operator.rawValue]
    }

    var jsonText: String {
        (try? String(data: JSONSerialization.data(withJSONObject: json), encoding: .utf8))
            .flatMap { $0 } ?? "{\"operator\":\"clip\"}"
    }

    var displayName: String {
        self.operator.usesWhite
            ? "\(self.operator.displayName) (white \(Self.format(white)))"
            : self.operator.displayName
    }

    static func format(_ value: Double) -> String {
        value == value.rounded() ? String(Int(value)) : String(format: "%g", value)
    }

    static func == (lhs: ToneMap, rhs: ToneMap) -> Bool {
        lhs.operator == rhs.operator
            && (!lhs.operator.usesWhite || lhs.white == rhs.white)
    }
}

/// The core's float operator table — what the mode evaluates, what it refuses
/// and why. Read ONCE per process: it is a constant of the build.
struct FloatOperators: Decodable {
    struct Refused: Decodable {
        let name: String
        let reason: String

        private enum CodingKeys: String, CodingKey {
            case name = "operator"
            case reason
        }
    }

    let admitted: [String]
    let refused: [Refused]
    let toneMaps: [String]

    private enum CodingKeys: String, CodingKey {
        case admitted
        case refused
        case toneMaps = "tone_maps"
    }

    static let table: FloatOperators? = {
        guard let value = try? FloatCore.operatorsJSON(),
              let data = try? JSONSerialization.data(withJSONObject: value) else { return nil }
        return try? JSONDecoder().decode(FloatOperators.self, from: data)
    }()

    /// The reply shape both `get_tone_map` and the sheet read.
    var reply: [String: Any] {
        [
            "admitted": admitted,
            "refused": refused.map { ["operator": $0.name, "reason": $0.reason] },
            "tone_maps": toneMaps,
        ]
    }
}

/// One extended-range sample readout — the `PrecisionReadout` shape over
/// float storage. It parses both the synchronous composite sample
/// (`float_sample`) and the scoped float sample (`scoped_analysis` with
/// `storage: "rgba32_float"`), which share every key read here.
struct FloatReadout {
    let json: [String: Any]
    let x: Int
    let y: Int
    /// The centre sample, or the mean when the centre is outside the image.
    let values: [Double]
    /// The block mean (`mean_rgba32`): what the eyedropper picks.
    let mean: [Double]
    let aboveWhite: [Bool]
    let belowZero: [Bool]
    let stops: [Double]

    init(_ json: [String: Any]) throws {
        guard let x = (json["x"] as? NSNumber)?.intValue,
              let y = (json["y"] as? NSNumber)?.intValue,
              let mean = json["mean_rgba32"] as? [Double], mean.count == 4,
              let above = json["above_white"] as? [Bool], above.count == 3,
              let below = json["below_zero"] as? [Bool], below.count == 3,
              let stops = json["stops"] as? [Double], stops.count == 3 else {
            throw DoctoredCoreError(message: "Invalid 32-bit float sample.")
        }
        self.json = json
        self.x = x
        self.y = y
        values = (json["center_rgba32"] as? [Double]) ?? mean
        self.mean = mean
        aboveWhite = above
        belowZero = below
        self.stops = stops
    }

    /// Linear components at full precision — there are no codes to report.
    var linearText: String {
        values.map { String(format: "%.6g", $0) }.joined(separator: ", ")
    }

    /// `mean` as `linearText` formats `values`: the numbers the eyedropper
    /// picked.
    var meanText: String {
        mean.map { String(format: "%.6g", $0) }.joined(separator: ", ")
    }

    var stopsText: String {
        stops.map { String(format: "%+.2f", $0) }.joined(separator: ", ") + " EV"
    }

    var rangeNote: String? {
        if aboveWhite.contains(true) && belowZero.contains(true) { return "above white, below zero" }
        if aboveWhite.contains(true) { return "above white" }
        if belowZero.contains(true) { return "below zero" }
        return nil
    }

    /// The extended-range sample at one canvas point — `PrecisionReadout.at`'s
    /// contract: nil when there is no document or no pixel of the
    /// (2·reach+1) block is on the canvas, checked before any render; a
    /// stored projection error, a render error or the core's refusal throws.
    static func at(_ point: (x: Int, y: Int), document: ImageDocument,
                   reach: Int = 0) throws -> Self? {
        guard let doc = document.doc, doc.sampleBlockTouchesCanvas(point, reach: reach) else { return nil }
        let source = try document.currentPrecisionProjection()
        return try at(point, projection: source, reach: reach)
    }

    /// The same read over a projection the caller already holds (the Color
    /// Samplers). nil when no pixel of the block is on the projection.
    static func at(_ point: (x: Int, y: Int), projection: RasterPrecisionImage,
                   reach: Int) throws -> Self? {
        guard point.x >= -reach, point.x < projection.width + reach,
              point.y >= -reach, point.y < projection.height + reach else { return nil }
        return try Self(projection.floatSampleJSON(x: Int32(point.x), y: Int32(point.y),
                                                   reach: UInt32(reach)))
    }
}

extension RasterDocument {
    var isFloat: Bool { precision?.isFloat ?? false }

    /// The document's tone map; nil on a u16 or byte document.
    var toneMap: ToneMap? {
        guard let text = precision?.toneMapJSON() else { return nil }
        return try? ToneMap(text: text)
    }

    /// nil WITHOUT throwing when the map is unchanged.
    func withToneMap(_ map: ToneMap) throws -> RasterDocument? {
        guard let precision else {
            throw DoctoredCoreError(
                message: "A tone map applies only to a 32-bit float document.")
        }
        guard let result = try precision.settingToneMap(map.jsonText) else { return nil }
        return try RasterDocument(precision: result)
    }

    /// The first operator the float mode cannot evaluate; nil on every other
    /// document, answered without a walk.
    func floatRefusal() -> String? { precision?.floatRefusalText() }

    /// The display bytes for a typed projection: a float document's own tone
    /// map, a Lab document's Display P3 view (`LabDocument.display`), or the
    /// ordinary preview. The ONE place that choice is made.
    /// `view` is a float window's exposure and candidate tone map
    /// (`ViewExposure.swift`); it is ignored at every other depth, and the
    /// default is the document's own display.
    func display(of image: RasterPrecisionImage, context: PrecisionWorkContext,
                 control: RasterJobControl? = nil,
                 view: FloatDisplayView = .document) throws -> RasterImage {
        if let precision, precision.isFloat {
            if view != .document {
                return try precision.displayRGBA8(of: image, view: view,
                                                  workingLimit: context.limit,
                                                  otherRetained: context.otherRetained,
                                                  control: control)
            }
            return try precision.displayRGBA8(of: image, workingLimit: context.limit,
                                              otherRetained: context.otherRetained,
                                              control: control)
        }
        // #547 Duotone phase 1a: the gray channel through the inks (`DuotoneDocument.display`).
        if isDuotone, let precision {
            return try DuotoneDocument.display(of: image, precision: precision, context: context,
                                               control: control)
        }
        // #547 Lab: the core's Display P3 view of the u16 Lab codes, never a byte preview of them.
        if isLab { return try LabDocument.display(of: image, context: context, control: control) }
        return try image.previewRGBA8(workingLimit: context.limit,
                                      otherRetained: context.otherRetained, control: control)
    }

    /// The deliberate conversion to the u16 mode through this document's map.
    func floatToU16(control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        guard let precision, precision.isFloat else {
            throw DoctoredCoreError(
                message: "Converting to 16-bit requires a 32-bit float document.")
        }
        let context = try PrecisionWorkContext(source: precision)
        return try precision.floatToU16(workingLimit: context.limit,
                                        otherRetained: context.otherRetained, control: control)
    }
}

/// The host-side gate for the mode: what a float document admits, and the one
/// sentence everything outside it carries.
enum FloatDocument {
    static var limitation: String {
        let modes: String = NativeLayerBlending.floatModeNames.joined(separator: ", ")
        return "32-bit float editing supports " + modes + limitationHead + adjustmentClause
            + limitationPaint
            + limitationSmallGaps
            + limitationRetouch
            + limitationVision
            + destructiveClause + filterClause + limitationRest + limitationTail
    }

    /// `limitation`'s small-gaps part, between the adjustment layers and Image ▸
    /// Adjustments, one literal for `limitationHead`'s reason.
    private static let limitationSmallGaps: String =
        "Apply Layer Mask, Layer Via Copy and Layer Via Cut, Assign Profile and Convert to "
        + "Profile (a matrix/TRC RGB profile; Convert applies the matrix between the two "
        + "profiles' primaries in linear light with no clamp, at relative, perceptual or "
        + "saturation intent with black-point compensation off and adjustment parameters "
        + "kept), Calculations (to a channel or a selection), Add Luminosity Masks, Contact "
        + "Sheet (each cell is the tone-mapped 8-bit rendering), "

    /// `limitation`'s clause for the Brush, Eraser and Gradient.
    private static let limitationPaint: String = FloatPaint.limitationClause

    /// `limitation`'s clause for the retouch tools and Stroke Path.
    private static let limitationRetouch: String = FloatRetouch.limitationClause

    /// `limitation`'s clause for the Vision selections.
    private static let limitationVision: String =
        "Select Subject, Select Objects, Select People (model \"people\"), All People, Face Parts (inspect_faces, "
        + "select_face_parts), the Subject tool and "
        + "the Object Selection tool (Vision reads "
        + "the tone-mapped 8-bit display at view exposure 0; Object Selection's edge refinement "
        + "is off by default and runs on request on the display codes), Select Sky, Person Parts "
        + "and the Object Finder (select_sky, select_person_parts, object_finder_select: their "
        + "models read the same display), "

    /// The adjustment ops a float document evaluates — the `*_adjustment`
    /// rows of the core's operator table, stripped to their op names
    /// (`levels`, `exposure`, …). Empty when the table is unreadable.
    static var adjustmentOps: [String] {
        let suffix = "_adjustment"
        return (FloatOperators.table?.admitted ?? []).compactMap { row in
            row.hasSuffix(suffix) ? String(row.dropLast(suffix.count)) : nil
        }
    }

    /// `adjustmentOps` as the limitation sentence names them, ending ", "
    /// so the sentence reads on; empty when there are none.
    private static var adjustmentClause: String {
        let names: [String] = adjustmentOps.compactMap { name in
            AdjustmentLayerOp(rawValue: name).map(floatName)
        }
        guard let last = names.last else { return "" }
        let list = names.count == 1
            ? last : names.dropLast().joined(separator: ", ") + " and " + last
        return list + " adjustment layers, "
    }

    /// `op`'s name with the qualifier its float form carries: the settings
    /// the core admits at 32 bits when that is not every setting.
    /// Every setting of each admitted op runs, so none carries one.
    private static func floatName(_ op: AdjustmentLayerOp) -> String {
        op.displayName
    }

    /// The adjustments a float document applies DESTRUCTIVELY (Image ▸
    /// Adjustments, MCP `apply_filter`) — the `*_apply` rows of the core's
    /// operator table, stripped to their op names. Empty
    /// when the table is unreadable.
    static var destructiveOps: [String] {
        let suffix = "_apply"
        return (FloatOperators.table?.admitted ?? []).compactMap { row in
            row.hasSuffix(suffix) ? String(row.dropLast(suffix.count)) : nil
        }
    }

    /// The filters a float document runs on a layer's own pixels:
    /// the `*_filter` rows of the core's operator table, stripped to
    /// their selected-edit names (`blur`, `box_blur`, …, `liquify`). Empty
    /// when the table is unreadable.
    static var filterOps: [String] {
        let suffix = "_filter"
        return (FloatOperators.table?.admitted ?? []).compactMap { row in
            row.hasSuffix(suffix) ? String(row.dropLast(suffix.count)) : nil
        }
    }

    /// The filters with no one-plane form: they edit a layer's pixels only, and the core
    /// refuses each by name on a mask, channel or colour plane
    /// (`float_plane_filter::layer_only_refusal`).
    static let layerOnlyFilters: Set<String> = ["liquify", "depth_blur", "develop_filter"]

    /// The filters a float document runs on a mask, an alpha channel or one colour channel:
    /// `filterOps` but `layerOnlyFilters`.
    static var planeFilterOps: [String] { filterOps.filter { !layerOnlyFilters.contains($0) } }

    /// The Filter menu items of `filterOps`, by selector. The menu validates
    /// one only while its op is in `filterOps`, on a layer's own pixels.
    static let filterActions: [String: String] = [
        "showBlur:": "blur",
        "showBoxBlur:": "box_blur",
        "showMotionBlur:": "motion_blur",
        "showRadialBlur:": "radial_blur",
        "showLensBlur:": "lens_blur",
        "showTiltShift:": "tilt_shift",
        "showOffset:": "offset",
        "showLensCorrection:": "lens_correction",
        "showLiquify:": "liquify",
        "showUnsharpMask:": "unsharp_mask",
        "showHighPass:": "high_pass",
        "showSurfaceBlur:": "surface_blur",
        "showDustAndScratches:": "dust_and_scratches",
        "showReduceNoise:": "reduce_noise",
        "showAddNoise:": "add_noise",
        "showAddGrain:": "add_grain",
        "applyClouds:": "clouds",
        "applyDifferenceClouds:": "difference_clouds",
        "applyEmboss:": "emboss",
        "applyEdgeDetect:": "edge_detect",
        "showPixelate:": "pixelate",
        "applySharpen:": "sharpen",
        "showPresence:": "presence",
        "showDevelopFilter:": "develop_filter",
        "showSmartSharpen:": "smart_sharpen",
        "showDepthBlur:": "depth_blur",
    ]

    /// What the limitation sentence calls each of `filterOps` (and a CMYK refusal each filter,
    /// `CmykAdjustFilter.displayName`).
    static let filterNames: [String: String] = [
        "blur": "Gaussian Blur",
        "box_blur": "Box Blur",
        "motion_blur": "Motion Blur",
        "radial_blur": "Radial Blur",
        "lens_blur": "Lens Blur",
        "tilt_shift": "Tilt-Shift",
        "offset": "Offset",
        "lens_correction": "Lens Correction",
        "liquify": "Liquify",
        "unsharp_mask": "Unsharp Mask",
        "high_pass": "High Pass",
        "surface_blur": "Surface Blur",
        "dust_and_scratches": "Dust & Scratches",
        "reduce_noise": "Reduce Noise",
        "add_noise": "Add Noise",
        "add_grain": "Add Grain",
        "clouds": "Clouds",
        "difference_clouds": "Difference Clouds",
        "emboss": "Emboss",
        "edge_detect": "Edge Detect",
        "pixelate": "Pixelate",
        "sharpen": "Sharpen",
        "presence": "Presence",
        "develop_filter": "Develop Filter",
        "smart_sharpen": "Smart Sharpen",
        "depth_blur": "Depth Blur",
    ]

    /// `filterOps` as the limitation sentence names them, ending ", " so the
    /// sentence reads on; empty when there are none.
    private static var filterClause: String {
        let names: [String] = filterOps.map { filterNames[$0] ?? $0 }
        guard let last = names.last else { return "" }
        let list = names.count == 1
            ? last : names.dropLast().joined(separator: ", ") + " and " + last
        return "the " + list + " filters on a layer's pixels (each under its 32-bit rule: linear "
            + "light, or the display encoding for a display-referred parameter; every control of "
            + "Presence and the Develop Filter, Dehaze, Highlights and Shadows holding a value "
            + "above white, Vibrance and Saturation scaled about white), and all but Liquify, "
            + "Depth Blur and the Develop Filter on a mask, an alpha channel or one colour "
            + "channel (coverage clamped to 0…1, a colour channel unclamped in linear light or "
            + "through the display encoding), "
    }

    /// The `apply_filter` names among `filterOps`: all but Liquify and Depth
    /// Blur, which are their own tools (`liquify`, `depth_blur`).
    private static var applyFilterOps: [String] { filterOps.filter { $0 != "liquify" && $0 != "depth_blur" } }

    /// Whether `filter` (an `apply_filter` name; "adjust" is `bcs`) is one
    /// of `destructiveOps` or one of the filters `apply_filter` runs on a
    /// float layer (`applyFilterOps`).
    static func appliesDestructively(_ filter: String) -> Bool {
        destructiveOps.contains(filter == "adjust" ? "bcs" : filter)
            || applyFilterOps.contains(filter)
    }

    /// The refusal `apply_filter` carries for a filter outside
    /// `destructiveOps` and `applyFilterOps` on the layer, outside
    /// `planeFilterOps` on a mask, alpha channel or colour channel, or for an
    /// admitted filter while Quick Mask is on.
    static func applyRefusal(_ filter: String, target: String, quickMask: Bool) -> String {
        if quickMask, target == "layer", appliesDestructively(filter) {
            return "apply_filter \"\(filter)\" is unavailable in a 32-bit float document while "
                + "Quick Mask is on: turn Quick Mask off to apply it to the layer's own pixels."
        }
        let on = target == "layer" ? "" : " on target \(target)"
        let planes = planeFilterOps.filter { applyFilterOps.contains($0) }
        return "apply_filter \"\(filter)\"\(on) is unavailable in a 32-bit float document: "
            + "it applies only " + (destructiveOps + applyFilterOps).joined(separator: ", ")
            + ", to a layer's own pixels, and " + planes.joined(separator: ", ")
            + " on a mask, an alpha channel or one colour channel. " + limitation
    }

    /// `destructiveOps` as the limitation sentence names them, ending ", "
    /// so the sentence reads on; empty when there are none.
    private static var destructiveClause: String {
        let names: [String] = destructiveOps.compactMap { name in
            AdjustmentLayerOp(rawValue: name).map(floatName)
        }
        guard let last = names.last else { return "" }
        let list = names.count == 1
            ? last : names.dropLast().joined(separator: ", ") + " and " + last
        return "Image ▸ Adjustments " + list + " on a layer's pixels, "
    }

    /// `limitation` between the blend-mode list and the adjustment clause,
    /// one literal so the compiler type-checks it in reasonable time.
    private static let limitationHead: String =
        " blending on raster layers, adjustment layers and isolated and Pass Through groups, "
        + "layer opacity, "
        + "names, visibility, "
        + "clipping, duplication, grouping and ungrouping, reordering and deleting layers, "
        + "masks and channels, canvas and layer geometry — Canvas Size, crop and straighten, "
        + "rotation, the flips, Image Size, Free Transform, Distort, Warp and mask transforms "
        + "(filtered geometry clamps each RGB component to the layer's own visible range; a "
        + "warp is applied at once and cannot be reopened), Move and Free Transform of the "
        + "selected pixels of a layer, a mask or an alpha channel — "

    /// `limitation` after the adjustment clause, one literal for the same
    /// reason.
    private static let limitationRest: String =
        "Fill… and Stroke… on a layer, its mask or an alpha channel "
        + "(in the same modes) or on the Red, Green, Blue or Alpha channel (Normal, Dissolve, "
        + "Darken, Multiply, Lighten, Linear Dodge (Add), Difference and Subtract; colours "
        + "above white fill as they are), layer styles (all nine effects in the same modes, fill opacity, "
        + "knockout and Blend Interior Effects as Group), Blend If and the global light, "
        + "placed images (Place Embedded/Linked and Place Live Photo; each file's 8-, 16- or "
        + "32-bit samples are rendered into linear light and land as an ordinary layer), "
        + "a placed or pasted camera RAW layer's Redevelop RAW, RAW Presets and settings, Synchronize "
        + "RAW Settings, Migrate RAW Decoder, RAW locals and Rasterize RAW Layer (the 16-bit "
        + "development rendered into linear light), "

    /// `limitation`'s last part, one literal for the same
    /// reason.
    private static let limitationTail: String =
        "text and shape layers (the Type and Shape tools, editing, Artwork Colors, Doctored "
        + "Artwork; colours are linear-light numbers at 32 bits), "
        + "Load Selection from a channel, "
        + "a mask, a layer's transparency or the Red, Green, Blue or luminosity plane (each "
        + "value clamped to 0…1), the Magic Wand, Quick Selection, Color Range, Grow and Similar "
        + "(tolerances and colour distances compare the display-encoded copy, a value above "
        + "white as white), Select and Mask (its edge stage, Quick Selection's Enhance Edge "
        + "included, on the same display codes; the New Layer outputs keep the float pixels; "
        + "Decontaminate Colors is unavailable), the Magnetic Lasso (traced on the display "
        + "codes), the readouts, the tone map, "
        + "render, export and save. Every other "
        + "operation is unavailable; convert to 16-bit with a tone map to use one."

    /// The MCP tools a float document answers. Everything else refuses with
    /// `limitation` before it reaches the core.
    /// Built from every `.floatTool` entry of the `<prefix>ToolFacts` tables (AgentServer+Registry.swift).
    static let tools: Set<String> = AgentServer.tools(with: .floatTool)

    /// The automatic tone tools 32-bit mode disables, each refused with the
    /// operator table's `autoRow` reason.
    static let autoTools: Set<String> = ["auto_tone", "auto_contrast", "auto_color"]

    /// The core's `float_document::REFUSED_AUTO`, the operator-table row the auto sentence reads.
    static let autoRow = "automatic tone correction (Auto Tone, Auto Contrast, Auto Color and the Levels dialog's Auto buttons)"

    /// The refusal a tool outside the set carries, naming it. An automatic tone tool carries
    /// the core's own reason for its row; with no such row, the generic sentence.
    static func refusal(tool: String) -> String {
        if autoTools.contains(tool),
           let reason = FloatOperators.table?.refused.first(where: { $0.name == autoRow })?.reason {
            return "\(tool) is unavailable in a 32-bit float document: \(reason). "
                + "Convert to 16 bits with a tone map to use it."
        }
        return "\(tool) is unavailable in a 32-bit float document. " + limitation
    }

    /// The blend modes a layer effect admits in a 32-bit float document: the
    /// modes whose `<name>_blend` (Normal's `normal_composite`) or
    /// `<name>_effect_blend` row the core's operator table admits — the
    /// layer set, as the 32-bit effect menu keeps exactly the layer menu's
    /// modes. The layer set when the table is
    /// unreadable.
    static var effectModes: [DrBlendMode] {
        guard let admitted = FloatOperators.table?.admitted else {
            return NativeLayerBlending.floatModes
        }
        return LayerStyleBlend.names.compactMap { mode, name in
            mode == DR_BLEND_NORMAL || admitted.contains(name + "_blend")
                || admitted.contains(name + "_effect_blend") ? mode : nil
        }
    }

    /// The effects whose 32-bit default mode differs from the 8/16-bit one
    /// (`float_styles.py`'s `defaults`):
    /// Outer Glow, Inner Glow and a Bevel's HIGHLIGHT start in Linear Dodge
    /// (Add) where their 8/16-bit default, Screen, has no 32-bit mode. Only
    /// modes the float table admits are kept.
    static var effectDefaultModes: [LayerStyleEffectKind: String] {
        let measured: [LayerStyleEffectKind: DrBlendMode] = [
            .outerGlow: DR_BLEND_ADDITION, .innerGlow: DR_BLEND_ADDITION,
            .bevelEmboss: DR_BLEND_ADDITION,
        ]
        let admitted = effectModes
        return measured.filter { admitted.contains($0.value) }
            .mapValues { LayerStyleBlend.name(for: $0) }
    }

    /// Whether the core's operator table admits `kind` (`<type>_effect`) —
    /// all nine; false only when the table is unreadable.
    static func admitsEffect(_ kind: LayerStyleEffectKind) -> Bool {
        FloatOperators.table?.admitted.contains(kind.rawValue + "_effect") ?? false
    }

    /// The sentence a refused effect row carries in the Layer Style sheet.
    static func effectRefusal(_ kind: LayerStyleEffectKind) -> String {
        "the \(kind.rawValue) layer effect is unavailable in a 32-bit float document"
    }

    /// What `get_precision` reports for the mode.
    static var support: [String: Any] {
        var value: [String: Any] = [
            "storage": "rgba32_float", "evaluation": "linear_light_f32_v1",
            "range": "extended: finite, |v| ≤ 65504, alpha 0…1",
            "negatives": "stored and admitted by every admitted operator; display and SDR "
                + "output map them to 0",
            "blend_modes": NativeLayerBlending.floatModeNames,
            "adjustments": adjustmentOps,
            "filters": filterOps,
            "plane_filters": planeFilterOps,
            "layer_styles": [
                "effects": LayerStyleEffectKind.allCases.filter(admitsEffect).map(\.rawValue),
                "effect_blend_modes": effectModes.map { LayerStyleBlend.name(for: $0) },
                "effect_default_modes": Dictionary(
                    uniqueKeysWithValues: effectDefaultModes.map { ($0.key.rawValue, $0.value) }),
                "blending_options": ["fill_opacity", "knockout", "blend_interior_as_group",
                                     "blend_if"],
                "global_light": true,
            ] as [String: Any],
            "placed_images": true,
            "text_layers": true, "shape_layers": true,
            "tone_maps": ToneMapOperator.allCases.map(\.rawValue),
            "supported_tools": tools.union(DocumentPrecision.documentFreeTools).sorted(),
            "limitations": limitation,
        ]
        if let table = FloatOperators.table {
            value["operators"] = table.reply
        }
        return value
    }
}
