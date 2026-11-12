import Foundation

// A startup check in both configurations enforces that `catalogJSON`'s names match the dispatch
// registry.
//
// Keep catalog groups behind their explicit non-inlining boundaries: a giant literal/concatenation
// function caused Release COWArrayOpts optimization to spend minutes in ColdBlockInfo. Preserve
// tool values/order when regrouping and compare parsed live catalogs.

// MARK: - Catalog

/// The MCP tool catalog AgentServer registers with the core: every tool's
/// name, description, and JSON-Schema input. The names here must match
/// AgentServer.handlers exactly — the parity check at server start (Debug and
/// Release) enforces it.
extension AgentServer {
    func tool(
        _ name: String, _ description: String,
        _ properties: [String: Any], required: [String] = []
    ) -> [String: Any] {
        [
            "name": name,
            "description": description,
            "inputSchema": [
                "type": "object", "properties": properties, "required": required,
            ] as [String: Any],
        ]
    }

    static let docIDProperty: [String: Any] = [
        "type": "integer",
        "description": "Target document id from list_documents; omit for the frontmost document.",
    ]

    /// `open_document`'s develop settings — the mirror of the RAW Develop
    /// dialog the UI puts up before a camera RAW opens
    /// (RawDevelopWindowController; the parsing and the refusals are in
    /// AgentServer+Raw.swift). An argument rather than a tool of its own
    /// Initial settings; redevelop_raw patches the saved effective recipe later.
    ///
    /// Written as an array of literals joined, never a `+` chain — the type
    /// checker resolves a long chain combinatorially and gives up, worst of
    /// all at type scope where there is no contextual type.
    static let rawDevelopProperty: [String: Any] = [
        "type": "object",
        "description": [
            "Develop settings for a CAMERA RAW (CR3, CR2, NEF, ARW, DNG, Apple ProRAW, ",
            "ORF, RW2, RAF, SRW, PEF). Omit it, or any key in it, to keep the value the ",
            "file itself reports \u{2014} an empty object is a valid \"as shot\" develop, ",
            "and an agent open never shows the Develop dialog either way. Ignored, with a ",
            "note in the result, for a file that is not developed as a camera RAW \u{2014} ",
            "one Core Image will not develop, and one that is already open, use redevelop_raw to change an already-open retained RAW. Keys: exposure (stops, ",
            "-4..4); temperature (2000..50000 K) and tint (-150..150) \u{2014} one coupled ",
            "white balance, so passing either leaves the as-shot balance and the other ",
            "half comes from the file; tone_curve (0..1, the global curve, 0 = linear); ",
            "shadows (0..2, no effect while tone_curve is 0); contrast (0..1, LOCAL ",
            "contrast on edges); sharpness (0..1); detail (0..3); luminance_noise (0..1); ",
            "baseline_compensation (−4..4 stops relative to each camera's own captured baseline); moire (0..1, only supported camera decoders); vignette {edge_exposure:−4..4, center:[x,y] normalized0..1, falloff:0.25..4} applies calibrated manual source lens compensation after local development. source_geometry is an explicit lens profile {profile_id,center:[x,y],radial:[k1,k2,k3],red_scale,blue_scale,filter}; its native inverse radial/TCA sampling follows local composition. Global transfer preserves each target's native local masks and optical calibration. ",
            "grain {amount:0..100 (required), size:0.5..4 px (default 1), roughness:0..100 (default 50)} is the Add Grain kernel applied LAST, after local development, vignette and explicit optics; amount 0 removes it, omitted size/roughness take the defaults (not the saved values), and its seed is the first 8 bytes of the embedded camera file's SHA-256, so redevelopment, reopen, presets and sync reproduce identical grain. ",
            "texture, clarity and dehaze (each −100..100, 0 = off) are Filter ▸ Presence on the finished 16-bit development, after local development, vignette and explicit optics and just before grain (dehaze first, then texture and clarity of the dehazed picture); their radii scale with the image's long side, so the look does not depend on resolution; an omitted one keeps its saved value. ",
            "color_noise (0..1); lens_correction (bool, Enable Profile Corrections: the lens profile the file carries, or when its decoder has none the bundled Lensfun profile matched at the first development (never both); omitted, it is ON for every file that carries one or matched one, not as shot, and open_document reports lens_profile {lens, supported, enabled, source: decoder|lensfun|null, lensfun: {id, model}|null}); highlight_recovery (bool, needs ",
            "macOS 26 or newer and refused by name below it). A value outside its range is ",
            "refused by name rather than clamped. A control this particular file's decoder ",
            "does not support is refused. output_space selects the retained master encoding, normalized SDR16: srgb (default on initial open), display_p3, adobe_rgb (Adobe RGB (1998)) or prophoto_rgb (ProPhoto RGB at gamma 1.8 with no ROMM linear toe, best in a 16-bit document). A colour outside the chosen space is clamped per channel to 0 or 65535 in it; neutral highlights clip at the same exposure in every space. get_raw_source reports the source; get_color_profile reports the document profile, which redevelopment preserves. Omit output_space during redevelopment to retain the saved choice. output_encoding selects the master: sdr16 (normalized 16-bit SDR, the default on open) or linear_float, the 32-bit float (linear) development: scene-linear f32 in output_space's primaries with the highlights the sensor recorded above the SDR white kept above 1.0. At tone_curve 0, where it starts unless one is named, it is linear in exposure with no highlight shoulder, so it looks brighter in the highlights than the 16-bit development until tone-mapped, and where a channel exceeds white it differs in hue as well (the 16-bit development moves such a colour toward white). With it tone_curve and shadows are the decoder's own under the extended range and keep values above white (shadows is inert at tone_curve 0); grain applies on the display encoding (each channel's encoded value moves by the 16-bit grain's offset for the pixel's encoded luma, unclamped, so a pixel at or above white keeps its value); nonzero texture, clarity or dehaze are refused. It exists only in a 32-bit float document: open_document with it opens one (refused with precision rgba8), place_image into a float document uses it by default, and it refuses on a layer of an 8- or 16-bit document. RAW presets and sync never carry it. as_shot_white_balance:true restores captured as-shot balance and cannot combine with temperature/tint. Omitted redevelopment fields keep saved values.",
        ].joined(),
    ]

    /// The op vocabulary `add_adjustment_layer` publishes and `apply_filter`
    /// points at — one constant, so the two surfaces can never describe the
    /// same op differently.
    ///
    /// Written as an array of independently-typed literals joined, never as
    /// a `+` chain: Swift's checker resolves a long `+` chain
    /// combinatorially and gives up well before thirty terms, worst of all
    /// at type scope where there is no contextual type.
    static let adjustmentOps: String = [
        "bcs (brightness, contrast, saturation, each -1..1, default 0; legacy true/false, ",
        "ABSENT = true, the offset-and-line form that clips; false is the modern form, a ",
        "brightness power and a contrast S-curve that keep black and white fixed); ",
        "curves (rgb, r, g, b: each an optional array of 2-16 [in, out] control points, ",
        "values 0-255, natural-spline-interpolated; a missing channel is identity; per-channel ",
        "curves apply before rgb; OR the map form a Photoshop pencil curve imports as, ",
        "rgb_map, r_map, g_map, b_map: each an optional array of 256 integers 0-255, entry ",
        "i the output for input code i, never both forms); ",
        "levels (black default 0, white default 1, 0 <= black < white <= 1; gamma 0.1-10, ",
        "default 1 \u{2014} here gamma > 1 BRIGHTENS; output_black default 0 and output_white ",
        "default 1, each 0-1 in either order, map the result into that range, so 1 and 0 ",
        "invert; r, g, b: each an optional object of the same five keys, a per-channel ",
        "stage applied BEFORE the master one; absent = identity); ",
        "hue_rotate (degrees, default 0); ",
        "threshold (level 0-1, default 0.5); posterize (levels, integer 2-64, REQUIRED); ",
        "exposure (exposure -20..20 stops default 0, offset -0.5..0.5 default 0, gamma ",
        "0.01-9.99 default 1 \u{2014} computed in linear light, and this is ",
        "Gamma Correction, the reciprocal power like levels' midtone gamma: above 1 BRIGHTENS); ",
        "vibrance (vibrance -1..1 default 0: raising it lifts the least saturated pixels most, ",
        "the lift damped by up to 40% on red and skin hues, and lowering it mutes every hue ",
        "evenly; saturation -1..1 default 0, a scale about a ",
        "ProPhoto-luminance anchor, not hue_saturation's, on the document profile's linear values); ",
        "hue_saturation (hue -180..180 degrees, saturation -1..1, lightness -1..1, all ",
        "default 0; bands, an object with any of reds/yellows/greens/cyans/blues/magentas, ",
        "each {hue -180..180 degrees, saturation -1..1, lightness -1..1, all default 0, ",
        "center 0-360 (360 excluded) degrees defaulting to 60 \u{d7} the band's index \u{2014} ",
        "reds 0, yellows 60, greens 120, cyans 180, blues 240, magentas 300 \u{2014} inner ",
        "0-180 default 15, falloff 0-180 default 30}. A band selects on CHROMA as well as ",
        "hue: a neutral has no hue to be one of, so it takes no band edit at all, and a ",
        "near-neutral ramps in, reaching full weight about 13 levels off grey. ",
        "colorize false, colorize_hue 0-360 ",
        "(360 excluded) default 0, ",
        "colorize_saturation 0-1 default 0.25, colorize_lightness -1..1 default 0); ",
        "color_balance (shadows, midtones, highlights, each {cyan_red, magenta_green, ",
        "yellow_blue} in -1..1 default 0, acting in whole hundredths; preserve_luminosity ",
        "default true, a per-tone renormalisation, not a luma rescale); ",
        "black_and_white (reds 0.4, yellows 0.6, greens 0.4, cyans 0.6, blues 0.2, ",
        "magentas 0.8, each -2..3; tint false, tint_color \"#998a66\" \u{2014} its numbers ",
        "are set to the grey's luminosity); ",
        "photo_filter (color \"#ec8a00\" (Warming 85; Cooling 80 is \"#006dff\"), density ",
        "0-1 default 0.25, preserve_luminosity default true); ",
        "channel_mixer (monochrome false; red, green, blue, gray, each {r, g, b, constant} ",
        "in -2..2, defaulting to the identity rows and gray {0.4, 0.4, 0.2, 0}; values are ",
        "fractions \u{2014} the dialog's percentages over 100); ",
        "selective_color (method \"relative\" or \"absolute\"; reds, yellows, greens, cyans, ",
        "blues, magentas, whites, neutrals, blacks, each {c, m, y, k} in -1..1 default 0); ",
        "shadows_highlights (shadows {amount 0-1 default 0.35, tone 0.01-1 default 0.5}, ",
        "highlights {amount default 0, tone default 0.5}, radius 0-1000 px default 30, ",
        "color -1..1 default 0.2, midtone_contrast -1..1 default 0 \u{2014} the tone estimate ",
        "is an alpha-weighted large-radius blur of the luma, so local contrast survives and ",
        "a cut-out gets no halo; it is the one op whose destructive twin sees different ",
        "pixels, because a layer reads the backdrop below it and a filter reads itself); ",
        "white_balance (temperature 1667-25000 K default 6504, tint -150..150 default 0; ",
        "the pair describes the illuminant the pixels were shot under and is Bradford-",
        "adapted to D65, so a HIGHER kelvin warms the image and a POSITIVE tint pushes it ",
        "toward magenta; 6504/0 is the exact identity); ",
        "Exposure and White Balance accept params.evaluation: document_v2 (default for ",
        "new RGB edits) uses the document ICC matrix/TRC profile: White Balance stays on a ",
        "common unclipped linear sRGB reference basis, while Exposure runs per channel in ",
        "the document's own primaries at the profile's effective gamma (2.2 for sRGB-shaped ",
        "curves, a pure power's own exponent such as ROMM's 1.8, and 1.883 for an offset ",
        "curve that fails the precision check such as ITU-709's); ",
        "legacy_srgb preserves the original sRGB numeric ",
        "processing. Re-editing a layer preserves its evaluation when omitted. Old Actions ",
        "without this key remain legacy. Scalar masks, individual planes and saved channels ",
        "default to legacy grayscale processing and refuse explicit document_v2. LUT-only ",
        "RGB profiles refuse a new or re-edited document_v2 of either op, and matrix/TRC ",
        "profiles whose tone curves fail the precision check refuse a new or re-edited ",
        "document_v2 White Balance (Exposure only when a curve is degenerate) at every depth ",
        "with the core's sentence; an ",
        "existing 8-bit document_v2 layer under a matrix/TRC profile whose curves fail the ",
        "check keeps rendering (one under a LUT-only profile does not render), and ",
        "legacy_srgb stays admitted under either profile: re-editing a document_v2 layer ",
        "with params.evaluation legacy_srgb switches it to the original sRGB processing, ",
        "which changes its look (Adjustment Options\u{2026} offers the same switch). ",
        "Alpha is unchanged in both versions. ",
        "gradient_map (gradient \u{2014} the same object SHAPE a layer style carries: stops, ",
        "2 to 32 of {position 0..1, color, opacity}, plus reverse \u{2014} but its stop colours ",
        "are the DOCUMENT's numbers, not the authored sRGB a style's are; method \"classic\" is ",
        "the Classic ramp (gradient.smoothness 0..1, default 1; each stop's midpoint / ",
        "opacity_midpoint 0.05..0.95 toward the next stop), \"perceptual\" the same over Oklab ",
        "stops (RGB only); linear and smooth are not offered; no method: linear, keys refuse; ",
        "the pixel's Rec. 709 luma picks the colour and a stop's opacity blends back toward the ",
        "original; dither default true adds a deterministic sub-level offset that stops ",
        "8-bit banding \u{2014} it is keyed on the pixel's position, which the layer counts ",
        "from the canvas and the destructive twin counts from the layer it was handed, so ",
        "on a layer whose offset is not 0,0 the two land the jitter on different pixels ",
        "and differ by one code on about half of them; pass dither false to compare them ",
        "exactly); ",
        "color_lookup (a .cube LUT, or an .icc/.icm look \u{2014} an RGB-to-RGB device-link ",
        "profile applied to the document's numbers, or a Lab-to-Lab abstract profile baked ",
        "through the document's profile when it loads: pass file, an absolute path this ",
        "app parses, or the ",
        "stored form kind \"1d\"|\"3d\", size, table (base64 f32), domain_min, domain_max, ",
        "title, and source_size \u{2014} the .cube's own size when the table was resampled ",
        "down to this build's storage cap (3D 33, 1D 1024); strength 0-1 default 1. ",
        "get_document elides the table but keeps every other key); ",
        "replace_color (colors, REQUIRED, an array of 1-8 \"#rrggbb\" sampled colours in the ",
        "DOCUMENT's numbers; fuzziness 0-200 default 40, a Euclidean RGB distance in 0-255 ",
        "units: a pixel within fuzziness/2 of its nearest colour takes the whole edit, fading ",
        "linearly to none at fuzziness; hue -180..180, saturation -1..1, lightness -1..1, all ",
        "default 0, hue_saturation's master sliders applied to the matched pixels); ",
        "invert, grayscale, sepia (none).",
    ].joined()

    func catalogJSON() throws -> String {
        let schema = CatalogSchema()
        var catalog: [[String: Any]] = []
        catalog.append(contentsOf: documentCatalog(schema))
        catalog.append(contentsOf: layerCatalog(schema))
        catalog.append(contentsOf: maskStyleCatalog(schema))
        catalog.append(contentsOf: adjustmentCatalog(schema))
        catalog.append(contentsOf: paintingCatalog(schema))
        catalog.append(contentsOf: retouchingCatalog(schema))
        catalog.append(contentsOf: retouchBrushCatalog(schema))
        catalog.append(contentsOf: liquifyCatalog(schema))
        catalog.append(contentsOf: depthBlurCatalog(schema))
        catalog.append(contentsOf: neuralFiltersCatalog(schema))
        catalog.append(contentsOf: skyReplacementCatalog(schema))
        catalog.append(contentsOf: generativeFillCatalog(schema))
        catalog.append(contentsOf: modelSelectionCatalog(schema))
        catalog.append(contentsOf: describedLayerCatalog(schema))
        catalog.append(contentsOf: missingFontsCatalog(schema))
        catalog.append(contentsOf: basicSelectionCatalog(schema))
        catalog.append(contentsOf: guideChannelCatalog(schema))
        catalog.append(contentsOf: channelMathCatalog(schema))
        catalog.append(contentsOf: geometryLayerCatalog(schema))
        catalog.append(contentsOf: geometrySetCatalog())
        catalog.append(contentsOf: persistedWarpCatalog())
        catalog.append(contentsOf: puppetWarpCatalog(schema))
        catalog.append(contentsOf: perspectiveWarpCatalog(schema))
        catalog.append(contentsOf: persistenceActionCatalog(schema))
        catalog.append(contentsOf: psdExportCatalog())
        catalog.append(contentsOf: exportWorkspaceCatalog())
        catalog.append(contentsOf: productionOutputsCatalog())
        catalog.append(contentsOf: sessionCatalog())
        catalog.append(contentsOf: advancedSelectionCatalog())
        catalog.append(contentsOf: precisionCatalog())
        catalog.append(contentsOf: retainedImageCatalog())
        catalog.append(contentsOf: rawSourceCatalog())
        catalog.append(contentsOf: rawLocalCatalog())
        catalog.append(contentsOf: livePhotoCatalog())
        catalog.append(contentsOf: rawPresetCatalog())
        catalog.append(contentsOf: scalarMorphologyCatalog())
        catalog.append(contentsOf: assistedAnnotationCatalog())
        catalog.append(contentsOf: outputColorCatalog())
        catalog.append(contentsOf: proofPresetCatalog())
        catalog.append(contentsOf: AgentServer.formatAdditionsCatalog())
        catalog.append(contentsOf: evaluationCatalog())
        catalog.append(contentsOf: floatCatalog())
        catalog.append(contentsOf: moveLayersCatalog())
        catalog.append(contentsOf: moveSelectedPixelsCatalog())
        catalog.append(contentsOf: transformSelectedPixelsCatalog())
        catalog.append(contentsOf: transformAgainCatalog())
        catalog.append(contentsOf: reusableActionsCatalog())
        catalog.append(contentsOf: layerLabelCatalog())
        catalog.append(contentsOf: layerCompCatalog())
        catalog.append(contentsOf: newDocumentCatalog())
        catalog.append(contentsOf: workspaceCatalog())
        catalog.append(contentsOf: viewsCatalog())
        catalog.append(contentsOf: colorSamplerCatalog())
        catalog.append(contentsOf: fillSelectionCatalog())
        catalog.append(contentsOf: strokeSelectionCatalog())
        catalog.append(contentsOf: photoshopPresetCatalog())
        catalog.append(contentsOf: horizonCatalog())
        catalog.append(contentsOf: pixelArtCatalog())
        catalog.append(contentsOf: adjustmentPresetCatalog())
        catalog.append(contentsOf: brushPresetCatalog())
        catalog.append(contentsOf: patternCatalog())
        catalog.append(contentsOf: stylePresetCatalog())
        catalog.append(contentsOf: canvasFramingCatalog())
        catalog.append(contentsOf: layerDragCatalog())
        catalog.append(contentsOf: fileInfoCatalog())
        catalog.append(contentsOf: pathsCatalog())
        catalog.append(contentsOf: stacksCatalog())
        catalog.append(contentsOf: mergeHDRCatalog())
        catalog.append(contentsOf: autoBlendCatalog())
        catalog.append(contentsOf: mergePanoramaCatalog())
        catalog.append(contentsOf: lensProfilesCatalog())
        catalog.append(contentsOf: fadeCatalog())
        catalog.append(contentsOf: equalizeCatalog())
        catalog.append(contentsOf: cmykModeCatalog())
        catalog.append(contentsOf: stampMergeGroupCatalog())
        catalog.append(contentsOf: hdrToningCatalog())
        catalog.append(contentsOf: indexedColorTableCatalog())
        catalog.append(contentsOf: duotoneCatalog())
        catalog.append(contentsOf: spotChannelCatalog())
        annotatePrecisionCatalog(&catalog)
        annotateOutputColorCatalog(&catalog)
        annotateEvaluationCatalog(&catalog)
        annotateFloatCatalog(&catalog)
        annotateCmykCatalog(&catalog)
        annotateGrayCatalog(&catalog)
        annotateLabCatalog(&catalog)
        annotateIndexedCatalog(&catalog)
        annotateDuotoneCatalog(&catalog)
        annotateBitmapCatalog(&catalog)
        let data = try JSONSerialization.data(withJSONObject: catalog)
        return String(decoding: data, as: UTF8.self)
    }

    // Keep tool groups behind non-inlining boundaries: one giant builder caused
    // Release WMO to spend minutes in COWArrayOpts / ColdBlockInfo on catalogJSON.
    /// Shared schema values are constructed once and passed to the bounded catalog builders.
    struct CatalogSchema {
        let docID: [String: Any]
        let index: [String: Any]
        let selectionMode: [String: Any]
        let profileChoice: [String: Any]
        let profilePath: [String: Any]
        let autoProperties: [String: Any]
        let blendNames: [String]
        let planeBlendNames: [String]
        let flowProperty: [String: Any]
        let spacingProperty: [String: Any]
        let angleProperty: [String: Any]
        let roundnessProperty: [String: Any]
        let ringProperty: [String: Any]
        let seedProperty: [String: Any]
        let sampleProperty: [String: Any]
        let ignoreAdjustmentsProperty: [String: Any]
        let retouchOutputProperty: [String: Any]
        let pupilSizeProperty: [String: Any]
        let darkenProperty: [String: Any]
        let strokeBlendProperty: [String: Any]
        let targetVocabulary: String
        let alphaWriteNote: String
        let cloneBlendProperty: [String: Any]
        let textWeightProperty: [String: Any]
        let textItalicProperty: [String: Any]
        let textUnderlineProperty: [String: Any]
        let textStrikethroughProperty: [String: Any]
        let textTrackingProperty: [String: Any]
        let textLeadingProperty: [String: Any]
        let textBaselineProperty: [String: Any]
        let transformProperty: (String) -> [String: Any]

        @inline(never)
        init() {
            let docID = AgentServer.docIDProperty
            let index: [String: Any] = [
                "type": "integer",
                "description": "Layer index (0 = bottom); omit for the active layer. An index "
                    + "may name a layer GROUP — get_document says which, and a group has no "
                    + "pixels of its own.",
            ]
            let selectionMode: [String: Any] = [
                "type": "string",
                "enum": ["replace", "add", "subtract", "intersect"],
                "description": "How the new shape combines with the current selection "
                    + "(default replace). add unions, subtract removes the new shape from "
                    + "the current selection, intersect keeps the overlap; an empty result "
                    + "clears the selection.",
            ]
            // Which profile an assign/convert targets: the two this build writes
            // for itself, or an .icc/.icm file on disk.
            let profileChoice: [String: Any] = [
                "type": "string",
                "enum": ["srgb", "display_p3", "file"],
                "description": "srgb or display_p3 use the built-in profile; file reads the "
                    + "profile at path.",
            ]
            let profilePath: [String: Any] = [
                "type": "string",
                "description": "Path to an .icc/.icm profile; required when profile is \"file\".",
            ]
            // The three Auto commands take the same three arguments; the prose
            // above each is what differs.
            let autoProperties: [String: Any] = [
                "analysis_scope": ["type": "string", "enum": ["whole_target", "selection"],
                    "description": "Measurement scope independently of selected write coverage. whole_target is the historical default; selection weights each sample by its coverage and requires an active selection. Native source codes determine clipping."],
                "layer": index,
                "target": [
                    "type": "string",
                    "description": "layer (default), mask, red, green, blue, alpha or channel:<name>. "
                        + "Analyzes the whole target; writes only through active selection coverage.",
                ],
                "clip": [
                    "type": "number", "minimum": 0, "maximum": 0.1,
                    "description": "Share of the counted pixels dropped at EACH end before the "
                        + "stretch (default 0.001 = 0.1 %). Larger values push more of the "
                        + "image to pure black and pure white.",
                ],
                "document_id": docID,
            ]
            let blendNames = DrBlendMode.allBlendModes.map { $0.1 }
            // The modes a SINGLE 8-bit plane can carry: the four HSL modes are
            // defined over an RGB triple, so `dr_blend_planes` refuses them and
            // calculations — whose result is always one plane — must not offer
            // them as schema-valid values (the sheet's popup filters the same
            // list through ChannelMathControls.fillBlendModes).
            let planeBlendNames = DrBlendMode.allBlendModes
                .filter { !DrBlendMode.degeneratesOnGray($0.0) }
                .map { $0.1 }
            // The tip options every stroke tool shares beyond hardness — the
            // same knobs the options bar's paint tools carry.
            let flowProperty: [String: Any] = [
                "type": "number", "minimum": 1, "maximum": 100,
                "description": "Per-dab deposit in percent (default 100). Below 100 the "
                    + "stroke builds up where it overlaps itself instead of landing at full "
                    + "strength at once.",
            ]
            let spacingProperty: [String: Any] = [
                "type": "number", "minimum": 1, "maximum": 200,
                "description": "Dab spacing as a percent of the brush diameter (default 25). "
                    + "Above 25 the stroke stamps visibly; 150+ reads as a dotted line.",
            ]
            let angleProperty: [String: Any] = [
                "type": "number", "minimum": -180, "maximum": 180,
                "description": "Tip rotation in degrees, counter-clockwise (default 0). "
                    + "Visible only when roundness is below 100.",
            ]
            let roundnessProperty: [String: Any] = [
                "type": "number", "minimum": 1, "maximum": 100,
                "description": "Tip roundness in percent (default 100). Below 100 the dab "
                    + "squashes into an ellipse — with angle, a calligraphy nib.",
            ]
            // The retouching tools' shared numbers: the inpaint's sampling ring
            // and seed, and red-eye's two dials.
            let ringProperty: [String: Any] = [
                "type": "integer", "minimum": 0, "maximum": 512,
                "description": "Sampling-ring width in px around the region, 0 = automatic "
                    + "(the default). Clamped to 21-512 — the floor is three patch widths, "
                    + "the narrowest band a source patch can move in, so anything from 1 to 20 "
                    + "comes out as 21 — and then widened from below by the "
                    + "region's own size — a ring narrower than the hole offers too few "
                    + "distinct patches and the fill tiles visibly, so on a large region a "
                    + "small number here is raised to half the region's radius. It is a "
                    + "quality control: pass a wide one for a wide neighbourhood, not for "
                    + "speed. It costs little — rule 2 narrows every part's ring together to "
                    + "fit the work caps before anything runs, and the working windows are "
                    + "sized from the ring that survives that, so on a 1131-part selection a "
                    + "ring of 512 measures 3.8 s against 3.6 s at 21.",
            ]
            // The retouch sampling vocabulary, shared by every tool that has
            // one so they cannot drift apart: a tool whose enum said
            // something slightly different would be a tool whose recorded
            // Action means something slightly different.
            let sampleProperty: [String: Any] = [
                "type": "string", "enum": RetouchSampling.vocabulary,
                "description": "WHICH pixels this gesture reads. current is the target "
                    + "layer's own pixels on a transparent canvas, deliberately raw — its "
                    + "opacity, blend mode and mask say how it composites, not what its "
                    + "pixels are. current_and_below is the picture with everything ABOVE "
                    + "the target contributing nothing, which is what keeps a retouch from "
                    + "being graded twice when an adjustment layer sits over it. all is the "
                    + "whole composite. Above excludes the target's own enclosing groups, "
                    + "whose mask, opacity and blend still say how its level meets the "
                    + "picture. Defaults differ by tool and match what that tool always "
                    + "sampled: all for clone_stamp, heal_stroke, patch_region and "
                    + "content_aware_move, current for spot_heal_stroke, remove_stroke and "
                    + "content_aware_fill. The older "
                    + "sample_all_layers boolean still works and still means the same thing "
                    + "(true = all, false = current); sending both is fine when they agree "
                    + "and refused when they contradict, and current_and_below has no "
                    + "boolean spelling at all. Typed clone/heal retain current as their default; "
                    + "all three modes sample authoritative native-depth sources.",
            ]
            let ignoreAdjustmentsProperty: [String: Any] = [
                "type": "boolean",
                "description": "Leave adjustment LAYERS above the target out of the sampled "
                    + "picture (default false). A provable no-op with sample current or "
                    + "current_and_below, which already exclude everything above the target, "
                    + "so it is the meaningful control with sample all: take the texture of "
                    + "the layers stacked above me, but not the grade they are read "
                    + "through. Only an adjustment ENTRY is dropped — a layer style, a Blend "
                    + "If, and a plain raster layer in a tone blend mode are ordinary "
                    + "content and stay in.",
            ]
            let retouchOutputProperty: [String: Any] = [
                "type": "string", "enum": RetouchOutput.vocabulary,
                "description": "Where the repair lands (default layer: into the target's own "
                    + "pixels). new_layer puts it on a fresh raster layer directly above the "
                    + "target — same depth, same group, inheriting its clipped flag — so the "
                    + "original pixels stay exactly as they were. Refused with a reason when "
                    + "the target sits inside a group carrying its own blending, opacity, "
                    + "mask or style, where the repair would be gated twice: once in the "
                    + "sampled picture and again by the group. Native-depth output preserves "
                    + "the original raster samples.",
            ]
            let seedProperty: [String: Any] = [
                "type": "integer", "minimum": 0,
                "description": "RNG seed (default 0). The fill is reproducible: the same seed "
                    + "over the same pixels gives byte-identical output, and a different one "
                    + "gives a different plausible fill.",
            ]
            let pupilSizeProperty: [String: Any] = [
                "type": "number", "minimum": 1, "maximum": 100,
                "description": "Rejects a red region whose larger side exceeds this percent of "
                    + "the rectangle's SHORTER side (default 100). It exists to spare something "
                    + "bigger than an eye — a red shirt caught by a sloppy rectangle — not to "
                    + "require a small pupil, so 100 is the intended default; turn it down when "
                    + "something red was corrected that should not have been. A red region that "
                    + "reaches all four sides of the rectangle is rejected at any pupil_size: "
                    + "the rectangle is then inside the red, with no pupil in it to find.",
            ]
            let darkenProperty: [String: Any] = [
                "type": "number", "minimum": 0, "maximum": 100,
                "description": "How far the corrected pupil darkens once the red is neutralised "
                    + "(0-100, default 50; 0 neutralises only).",
            ]
            let strokeBlendProperty: [String: Any] = [
                "type": "string", "enum": blendNames,
                "description": "Blend mode the stroke's paint composites with (default Normal) "
                    + "— the layer blend-mode names. Not valid with a coverage target — a "
                    + "mask, a colour plane or a channel.",
            ]
            // The widened paint target: a colour plane or an alpha channel is
            // coverage exactly like a mask. Shared by brush_stroke,
            // eraser_stroke and apply_filter, which differ only in the prose
            // above it, so the vocabulary is written once.
            let targetVocabulary =
                "\"layer\" (default) the layer's own pixels, \"mask\" the layer's mask, "
                + "\"red\"/\"green\"/\"blue\"/\"alpha\" ONE colour plane of that layer's "
                + "pixels, or \"channel:<name>\" one of the document's alpha channels "
                + "(list_channels names them)."
            // The one exception to "every other plane byte-identical", and it
            // destroys data: the plane WRITER clears a pixel's colour bytes when
            // its new alpha is 0 — the core's straight-alpha rule, stated for
            // dr_doc_with_layer_plane in the C header — so inverting alpha twice
            // does not bring the colours back. It rides with the tools that WRITE
            // a whole plane (apply_filter, fill, gradient); the stroke ops
            // deliberately do the opposite ("a stroke is incremental" —
            // painting_layer_plane never clears colour), so they must not carry it.
            let alphaWriteNote =
                " Writing \"alpha\" is the one exception to that: every pixel whose new "
                + "alpha is 0 loses its colour bytes as well, so re-running the edit "
                + "does not bring them back."
            let cloneBlendProperty: [String: Any] = [
                "type": "string", "enum": blendNames,
                "description": "Blend mode the cloned paint composites with (default Normal) "
                    + "— the layer blend-mode names.",
            ]
            // The text-layer typography every text tool takes — the options
            // bar's own knobs — and the described layers' shared transform.
            let textWeightProperty: [String: Any] = [
                "type": "integer", "minimum": 0, "maximum": 15,
                "description": "NSFontManager weight 0-15: 3 light, 5 regular (default), 6 "
                    + "medium, 8 semibold, 9 bold — the options bar's Weight popup.",
            ]
            let textItalicProperty: [String: Any] = [
                "type": "boolean", "description": "Italic face (default false).",
            ]
            let textUnderlineProperty: [String: Any] = [
                "type": "boolean", "description": "Single underline (default false).",
            ]
            let textStrikethroughProperty: [String: Any] = [
                "type": "boolean", "description": "Single strikethrough (default false).",
            ]
            let textTrackingProperty: [String: Any] = [
                "type": "number", "minimum": -100, "maximum": 100,
                "description": "Extra spacing between glyphs in px (kern), -100 to 100, "
                    + "default 0 — the options bar's Tracking field.",
            ]
            let textLeadingProperty: [String: Any] = [
                "type": "number", "minimum": 0, "maximum": 1000,
                "description": "Line height in px, 0 to 1000; 0 (default) = the font's natural "
                    + "height — the options bar's Leading field.",
            ]
            let textBaselineProperty: [String: Any] = [
                "type": "number", "minimum": -100, "maximum": 100,
                "description": "Baseline shift in px, -100 to 100, positive raises the text, "
                    + "default 0 — the options bar's Baseline field.",
            ]
            let transformProperty: (String) -> [String: Any] = { role in
                [
                    "type": "array", "items": ["type": "number"], "minItems": 4, "maxItems": 4,
                    "description": "The layer's 2×2 linear map [a, b, c, d], row-major — "
                        + "(x, y) maps to (a·x + b·y, c·x + d·y) — \(role); default "
                        + "[1, 0, 0, 1]. Rotation by θ clockwise is [cos θ, -sin θ, sin θ, "
                        + "cos θ]. transform_layer composes onto it; get_document reports it.",
                ]
            }

            self.docID = docID
            self.index = index
            self.selectionMode = selectionMode
            self.profileChoice = profileChoice
            self.profilePath = profilePath
            self.autoProperties = autoProperties
            self.blendNames = blendNames
            self.planeBlendNames = planeBlendNames
            self.flowProperty = flowProperty
            self.spacingProperty = spacingProperty
            self.angleProperty = angleProperty
            self.roundnessProperty = roundnessProperty
            self.ringProperty = ringProperty
            self.seedProperty = seedProperty
            self.sampleProperty = sampleProperty
            self.ignoreAdjustmentsProperty = ignoreAdjustmentsProperty
            self.retouchOutputProperty = retouchOutputProperty
            self.pupilSizeProperty = pupilSizeProperty
            self.darkenProperty = darkenProperty
            self.strokeBlendProperty = strokeBlendProperty
            self.targetVocabulary = targetVocabulary
            self.alphaWriteNote = alphaWriteNote
            self.cloneBlendProperty = cloneBlendProperty
            self.textWeightProperty = textWeightProperty
            self.textItalicProperty = textItalicProperty
            self.textUnderlineProperty = textUnderlineProperty
            self.textStrikethroughProperty = textStrikethroughProperty
            self.textTrackingProperty = textTrackingProperty
            self.textLeadingProperty = textLeadingProperty
            self.textBaselineProperty = textBaselineProperty
            self.transformProperty = transformProperty
        }
    }

    // A set of entries is always EXPLICIT: none of these tools falls back
    // to the app's panel selection, which an agent cannot see. Same shape
    // as `points` and `corners`; no maxItems, because the document's
    // layer count is not knowable when the catalog is built — the
    // handler is what enforces the range.
    func layerList(_ description: String, minItems: Int = 1) -> [String: Any] {
        [
            "type": "array", "items": ["type": "integer"], "minItems": minItems,
            "description": description,
        ]
    }
}
