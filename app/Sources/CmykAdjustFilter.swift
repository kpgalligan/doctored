import AppKit

// Image ▸ Adjustments, Layer ▸ New Adjustment Layer, Image ▸ Auto and the Filter menu on a CMYK
// document (#1): which items run at 8 and 16 bits and the sentence each refused item carries;
// the CMYK parameter rows of the four ops whose params name ink (Photo Filter, Channel Mixer,
// Gradient Map, Replace Color); Clouds' ink; and Auto Tone / Contrast / Color through the CMYK
// tone analysis. The core's twins are `cmyk_adjust_rgb`'s `ADJUSTMENT_REFUSALS` and
// `cmyk_filter`'s `FILTER_REFUSALS`, in the same words. No item differs between 8 and 16 bits;
// the depth is asked all the same, so an item that differs changes a table entry and nothing
// else.

enum CmykAdjustFilter {
    /// One item's enablement at each declared depth, whether it is an adjustment
    /// LAYER kind there too, and why it is refused (nil when it runs at both depths, or when its
    /// refusal gives no reason).
    struct Availability {
        let eight: Bool
        let sixteen: Bool
        let layer: Bool
        let reason: String?
    }

    private static let rgbOnly = "Doctored offers it on RGB documents only"

    private static func runs(layer: Bool = true) -> Availability {
        Availability(eight: true, sixteen: true, layer: layer, reason: nil)
    }

    private static func refused(_ reason: String? = nil) -> Availability {
        Availability(eight: false, sixteen: false, layer: false, reason: reason)
    }

    /// Every Doctored adjustment op, Auto included (take1's `apply:` and `layer:` rows).
    static let adjustments: [String: Availability] = [
        "levels": runs(),
        "curves": runs(),
        "hue_saturation": runs(),
        "bcs": runs(),
        "invert": runs(),
        "posterize": runs(),
        "threshold": runs(),
        "color_balance": runs(),
        "photo_filter": runs(),
        "channel_mixer": runs(),
        "selective_color": runs(),
        "gradient_map": runs(),
        "color_lookup": runs(),
        "shadows_highlights": runs(layer: false),
        "replace_color": runs(layer: false),
        "auto_tone": runs(layer: false),
        "auto_contrast": runs(layer: false),
        "auto_color": runs(layer: false),
        "exposure": refused(),
        "vibrance": refused(),
        "black_and_white": refused(),
        "hue_rotate": refused(rgbOnly),
        "grayscale": refused(rgbOnly),
        "sepia": refused(rgbOnly),
        "white_balance": refused(rgbOnly),
    ]

    /// Every Doctored filter (take1's `filter:` rows). Tilt-Shift's and Reduce Noise's
    /// enablement is unconfirmed (take1 could not drive either on RGB or CMYK) and they run, as
    /// the request's filters do; Presence and Add Grain are the Develop Filter's controls.
    static let filters: [String: Availability] = [
        "blur": runs(layer: false),
        "box_blur": runs(layer: false),
        "motion_blur": runs(layer: false),
        "radial_blur": runs(layer: false),
        "lens_blur": runs(layer: false),
        "tilt_shift": runs(layer: false),
        "offset": runs(layer: false),
        "liquify": runs(layer: false),
        "unsharp_mask": runs(layer: false),
        "high_pass": runs(layer: false),
        "surface_blur": runs(layer: false),
        "dust_and_scratches": runs(layer: false),
        "reduce_noise": runs(layer: false),
        "add_noise": runs(layer: false),
        "clouds": runs(layer: false),
        "difference_clouds": runs(layer: false),
        "emboss": runs(layer: false),
        "edge_detect": runs(layer: false),
        "pixelate": runs(layer: false),
        "sharpen": runs(layer: false),
        "smart_sharpen": runs(layer: false),
        "lens_correction": refused(),
        "presence": refused(),
        "add_grain": refused(),
        "develop_filter": refused(),
        "depth_blur": refused(rgbOnly),
        "red_eye": refused(rgbOnly),
    ]

    /// The Image ▸ Adjustments, New Adjustment Layer and Image ▸ Auto items that keep their own
    /// selectors (the tagged ones go through `AdjustmentMenuOrder`): the op each runs and
    /// whether it makes a layer.
    static let legacySelectors: [String: (op: String, layer: Bool)] = [
        "showAdjustments:": ("bcs", false),
        "showLevels:": ("levels", false),
        "showCurves:": ("curves", false),
        "applyInvert:": ("invert", false),
        "showPosterize:": ("posterize", false),
        "showThreshold:": ("threshold", false),
        "applyDesaturate:": ("hue_saturation", false),
        "showHueRotate:": ("hue_rotate", false),
        "applyGrayscale:": ("grayscale", false),
        "applySepia:": ("sepia", false),
        "newAdjustmentLayerBCS:": ("bcs", true),
        "newAdjustmentLayerLevels:": ("levels", true),
        "newAdjustmentLayerCurves:": ("curves", true),
        "newAdjustmentLayerInvert:": ("invert", true),
        "newAdjustmentLayerPosterize:": ("posterize", true),
        "newAdjustmentLayerThreshold:": ("threshold", true),
        "newAdjustmentLayerHueRotate:": ("hue_rotate", true),
        "newAdjustmentLayerGrayscale:": ("grayscale", true),
        "newAdjustmentLayerSepia:": ("sepia", true),
        "autoTone:": ("auto_tone", false),
        "autoContrast:": ("auto_contrast", false),
        "autoColor:": ("auto_color", false),
    ]

    /// The document's declared CMYK depth, 8 or 16 (16 when it cannot be read).
    static func depth(of doc: RasterDocument?) -> Int {
        if let bits = doc?.precisionInfo?["cmyk_depth"] as? Int { return bits }
        return (doc?.precision?.cmykInfo()?["depth"] as? Int) ?? 16
    }

    private static func enabled(_ entry: Availability?, depth: Int) -> Bool {
        guard let entry else { return false }
        return depth == 8 ? entry.eight : entry.sixteen
    }

    /// Whether adjustment `op` (an `AdjustmentLayerOp` raw value or an Auto op) runs at
    /// `depth`, as an adjustment layer when `layer`.
    static func allowsAdjustment(_ op: String, depth: Int, layer: Bool) -> Bool {
        let entry = adjustments[op]
        return enabled(entry, depth: depth) && (!layer || entry?.layer == true)
    }

    /// Whether filter `op` (a selected-edit name) runs at `depth`.
    static func allowsFilter(_ op: String, depth: Int) -> Bool {
        enabled(filters[op], depth: depth)
    }

    /// Whether the Filter menu item `action` (`FloatDocument.filterActions`) runs at `depth`.
    static func allowsFilterAction(_ action: String, depth: Int) -> Bool {
        FloatDocument.filterActions[action].map { allowsFilter($0, depth: depth) } ?? false
    }

    /// The answer for an adjustment menu item — tagged (`showAdjustmentSheet:`,
    /// `newAdjustmentLayerOp:`) or one of `legacySelectors` — or nil for any other item.
    static func allowsAdjustmentAction(_ action: String, tag: Int, depth: Int) -> Bool? {
        if ["showAdjustmentSheet:", "newAdjustmentLayerOp:"].contains(action) {
            guard let op = AdjustmentMenuOrder.op(for: tag) else { return false }
            return allowsAdjustment(op.rawValue, depth: depth, layer: action == "newAdjustmentLayerOp:")
        }
        return legacySelectors[action].map { allowsAdjustment($0.op, depth: depth, layer: $0.layer) }
    }

    /// The name a refusal gives `op`.
    static func displayName(_ op: String) -> String {
        switch op {
        case "auto_tone": return "Auto Tone"
        case "auto_contrast": return "Auto Contrast"
        case "auto_color": return "Auto Color"
        case "red_eye": return "Remove Red Eye"
        default:
            return AdjustmentLayerOp(rawValue: op)?.displayName ?? FloatDocument.filterNames[op] ?? op
        }
    }

    /// The refusal of `op` (as an adjustment layer when `layer`): the table's reason, the
    /// layer-kind rule, or the mode's sentence for an op the tables do not know.
    static func refusal(op: String, layer: Bool) -> String {
        let name = displayName(op)
        let entry = adjustments[op] ?? filters[op]
        guard let entry else { return "The \"\(op)\" adjustment is unavailable in a CMYK document" }
        if !entry.eight, !entry.sixteen {
            return entry.reason.map { "\(name) is unavailable in a CMYK document: \($0)" }
                ?? "\(name) is unavailable in a CMYK document"
        }
        if entry.eight != entry.sixteen {
            return "\(name) is unavailable in a CMYK document at \(entry.eight ? 16 : 8) bits"
        }
        if layer, !entry.layer {
            return "A \(name) adjustment layer is unavailable in a CMYK document: \(name) applies "
                + "to pixels only"
        }
        return "\(name) is unavailable in a CMYK document"
    }

    // MARK: - CMYK parameter rows

    /// Warming Filter (85)'s ink on the CMY triple (`L = 1 − ink` of #ec8a00): Photo Filter's
    /// default `ink`, the colour the core would use with none.
    static let warmingInk: [Double] = [1 - 236.0 / 255, 1 - 138.0 / 255, 1, 0]

    /// The Channel Mixer's CMYK output rows and their identity defaults (`cmyk_adjust_ink`).
    static let mixerRows: [(key: String, defaults: [Double])] = [
        ("cyan", [1, 0, 0, 0, 0]),
        ("magenta", [0, 1, 0, 0, 0]),
        ("yellow", [0, 0, 1, 0, 0]),
        ("black", [0, 0, 0, 1, 0]),
        ("gray", [0, 0, 0, 1, 0]),
    ]
    static let mixerSources = ["c", "m", "y", "k", "constant"]

    /// The CMYK table of an op whose params name ink, or nil for an op whose CMYK params are
    /// its RGB ones (`AdjustmentSchema.params(for:)`).
    static func cmykParams(for op: AdjustmentLayerOp) -> [AdjustmentSchema.Param]? {
        typealias Param = AdjustmentSchema.Param
        switch op {
        case .photoFilter:
            return [
                Param("ink", .numbers(count: 4, 0...1, warmingInk)),
                Param("density", .number(0...1, 0.25)),
                Param("preserve_luminosity", .boolean(true)),
            ]
        case .channelMixer:
            return mixerRows.map { row in
                Param(row.key, .object(zip(mixerSources, row.defaults).map {
                    Param($0.0, .number(-2...2, $0.1))
                }))
            } + [Param("monochrome", .boolean(false))]
        case .gradientMap:
            // "perceptual" is admitted here so the core's named refusal
            // ("The gradient map's Perceptual method is unavailable in a CMYK
            // document") is what MCP and Actions return.
            return [Param("method", .choice(["classic", "perceptual"], nil)),
                    Param("gradient", .inkGradient), Param("dither", .boolean(true))]
        case .replaceColor:
            return [
                Param("inks", .inks(maxCount: 8)),
                Param("fuzziness", .number(0...200, 40)),
                Param("hue", .number(-180...180, 0)),
                Param("saturation", .number(-1...1, 0)),
                Param("lightness", .number(-1...1, 0)),
            ]
        default:
            return nil
        }
    }

    /// `params` validated against `op`'s CMYK table (`AdjustmentSchema.validate`'s `cmyk`) when
    /// `document` is a CMYK document and the op's params name ink; nil otherwise, for the RGB
    /// validation to run. The MCP adjustment tools' one CMYK seam.
    static func validated(_ params: [String: Any], for op: AdjustmentLayerOp,
                          in document: ImageDocument?) throws -> [String: Any]? {
        guard document?.doc?.isCmyk == true, cmykParams(for: op) != nil else { return nil }
        return try AdjustmentSchema.validate(params, for: op, cmyk: true)
    }

    /// Whether `params` spell `op` in ink (a key, row or stop only the CMYK table has) — what
    /// `AdjustmentSchema.effective` reads them by when no document is at hand.
    static func spellsInk(_ params: [String: Any], for op: AdjustmentLayerOp) -> Bool {
        switch op {
        case .photoFilter: return params["ink"] != nil
        case .replaceColor: return params["inks"] != nil
        case .channelMixer:
            if ["cyan", "magenta", "yellow", "black"].contains(where: { params[$0] != nil }) { return true }
            let gray = params["gray"] as? [String: Any] ?? [:]
            return ["c", "m", "y", "k"].contains { gray[$0] != nil }
        case .gradientMap:
            let stops = (params["gradient"] as? [String: Any])?["stops"] as? [[String: Any]] ?? []
            return stops.contains { $0["ink"] != nil }
        default: return false
        }
    }

    // MARK: - Clouds' ink

    /// Clouds' and Difference Clouds' params on a CMYK document: each colour key becomes its ink
    /// (`foreground_ink` / `background_ink`, four fractions) — the swatch's stored ink when the
    /// colour is the swatch's, else the colour through the document's profile
    /// (`CmykPaint.ink`). An ink key passed as percent (MCP's flat `foreground_ink`) is scaled.
    /// Every other filter's params are returned as they are.
    static func inkParams(filter: String, params: [String: Any],
                          document: RasterPrecisionDocument) throws -> [String: Any] {
        guard filter == "clouds" || filter == "difference_clouds" else { return params }
        var out = params
        let shared = ToolOptionsStore.shared.sharedState
        let swatches: [(colorKey: String, inkKey: String, stored: CmykInk?, hex: String)] = [
            ("foreground_color", "foreground_ink", shared.foregroundInk, shared.foreground),
            ("background_color", "background_ink", shared.backgroundInk, shared.background),
        ]
        for swatch in swatches {
            if let raw = out[swatch.inkKey] {
                guard let values = (raw as? [Any])?.compactMap({ ($0 as? NSNumber)?.doubleValue }),
                      let ink = CmykInk(percent: values) else {
                    throw DoctoredCoreError(
                        message: "\(swatch.inkKey) must be four ink percents (C, M, Y, K, 0–100).")
                }
                out[swatch.inkKey] = ink.unit
                continue
            }
            guard let hex = out.removeValue(forKey: swatch.colorKey) as? String else { continue }
            let color = AdjustmentColor.color(fromHex: String(hex.prefix(7)), in: .sRGB) ?? .black
            let stored = hex.lowercased().prefix(7) == swatch.hex.lowercased().prefix(7)
                ? swatch.stored : nil
            out[swatch.inkKey] = Array(try CmykPaint.ink(color, stored: stored, in: document).prefix(4))
        }
        return out
    }

    // MARK: - Auto

    /// Image ▸ Auto Tone / Auto Contrast / Auto Color on a CMYK layer: the CMYK tone analysis's
    /// Auto (`form` `per_channel` for Tone and Color, `monochromatic` for Contrast, `clipPercent`
    /// clipped at each end, 0.1 % by default; the analysis caps it at 9.99) over `layer` within
    /// `coverage` (empty: the whole layer), as CMYK Levels params; nil when there is nothing to
    /// measure.
    static func autoLevelsParams(_ doc: RasterPrecisionDocument, layer: Int, coverage: [UInt16],
                                 form: String, clipPercent: Double = 0.1) throws -> [String: Any]? {
        let clip = min(clipPercent, 9.99)
        let request: [String: Any] = [
            "source": "layer", "layer": layer,
            "auto": ["algorithm": form, "shadows": clip, "highlights": clip],
        ]
        let context = try PrecisionWorkContext(source: doc)
        let analysis = try doc.cmykToneAnalysis(request: request, coverage: coverage, workingLimit: context.limit,
                                                otherRetained: context.otherRetained, control: nil)
        guard let auto = analysis["auto"] as? [String: Any], auto["empty"] == nil,
              let stages = CmykToneTools.levelsStages(fromAuto: auto) else { return nil }
        return LevelsMeta.params(stages)
    }

    /// The refusal of Image ▸ Auto Tone / Auto Contrast / Auto Color on a CMYK document's mask
    /// target: Auto runs on a layer's ink (the CMYK tone analysis), or on one ink plate or an
    /// alpha channel as on a 16-bit colour plane or channel (the scalar Auto).
    static func autoTargetRefusal(_ target: String) -> String {
        "A CMYK document runs Auto on a layer's ink, one ink plate or an alpha channel: target "
            + "\"\(target)\" is unavailable."
    }

    /// The Auto form an Image ▸ Auto command runs on ink.
    static func autoForm(_ tool: String) -> String {
        tool == "auto_contrast" ? "monochromatic" : "per_channel"
    }
}
