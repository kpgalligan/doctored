import Foundation

extension AgentServer {
    /// `apply_filter`'s account of Motion Blur, Lens Blur and Tilt-Shift.
    /// An array of literals joined, never a `+` chain, and outside the
    /// description's own chain, which is already long for the type checker.
    static let motionLensTiltText: String = [
        "motion_blur (Filters > Blur > Motion Blur\u{2026}: angle in degrees -360..360 default ",
        "0, positive counterclockwise; distance an integer 1-2000 px default 10 \u{2014} the mean ",
        "of distance+1 samples a pixel apart along the line through each pixel; filters ",
        "alpha); lens_blur (Filters > Blur > Lens Blur\u{2026}: radius an integer 1-100 default ",
        "15, the disc's radius; brightness 0-100 default 0 and threshold 0-255 default 255 in ",
        "8-bit LEVELS at every depth \u{2014} pixels brighter than threshold weigh up to ",
        "1 + 8 x brightness/100 times as much, so highlights bloom as discs; filters alpha); ",
        "tilt_shift (Filters > Blur > Tilt-Shift\u{2026}: radius an integer 1-100 default 15, ",
        "the blur far from the focus line; center_x and center_y 0-1 default 0.5, fractions ",
        "of the whole layer or plane; angle -90..90 default 0, a horizontal line, positive ",
        "counterclockwise; focus 0-1 default 0.15 and transition 0-1 default 0.25, fractions ",
        "of the height: within focus of the line nothing blurs, and the radius grows to full ",
        "over transition; filters alpha). These three take no frame: a selection only ",
        "confines where the result is written. ",
    ].joined()

    /// `apply_filter`'s account of Lens Correction, joined after
    /// `motionLensTiltText` for the same type-checker reason.
    static let lensCorrectionText: String = [
        "lens_correction (Filters > Lens Correction\u{2026}: distortion -100..100 default 0, ",
        "positive corrects barrel distortion (k1 = -0.0008 x distortion, the radial model RAW ",
        "development uses); chromatic_red_cyan and chromatic_blue_yellow -100..100 default 0, ",
        "the red and blue channels' lateral scale 1 + 0.00002 x value about the centre; ",
        "vignette_amount -100..100 default 0, up to 2 stops at the farthest corner, positive ",
        "brightens; vignette_midpoint 0-100 default 50, higher keeps more of the middle clear; ",
        "vertical_perspective and horizontal_perspective -100..100 default 0, negative ",
        "vertical widens the top and negative horizontal the left; angle -180..180 degrees ",
        "default 0, positive turns the picture counterclockwise; scale 50-150 percent default ",
        "100. The whole layer or plane is resampled about its centre (Catmull-Rom); what ",
        "falls outside the picture, or past the radial correction's fold, becomes ",
        "transparent; every default changes nothing; no frame, a selection only confines ",
        "where the result is written; profile: \"auto\" applies the bundled Lensfun profile ",
        "(Lensfun database v0.3.4, CC BY-SA 3.0) matched to the document's EXIF camera and lens ",
        "at its focal length, f-number and distance, refused in-band when nothing matches or ",
        "the target is not the camera frame; a lens id from list_lens_profiles applies that ",
        "lens; \"none\" or omitted applies none; a profile object (list_lens_profiles' profile) ",
        "applies as it is. The profile's distortion, chromatic aberration ",
        "and vignetting are corrected after the sliders' map, in one resample). ",
    ].joined()

    /// `apply_filter`'s account of Presence, joined after
    /// `lensCorrectionText` for the same type-checker reason.
    static let presenceText: String = [
        "presence (Filters > Presence\u{2026}: texture, clarity and dehaze, each -100..100 ",
        "default 0, 0 being off. texture raises the contrast of medium-fine detail, finer than ",
        "clarity and coarser than sharpening, without amplifying pixel noise; negative smooths ",
        "that detail and keeps edges. clarity raises local contrast at a wider radius, weighted ",
        "toward the midtones so highlights and shadows do not clip, over an edge-aware base so ",
        "a strong edge gets no halo; negative softens. dehaze removes atmospheric haze, ",
        "estimating the haze colour from the picture's haziest pixels and how much of it covers ",
        "each pixel; negative adds haze of that colour. The radii are a share of the longer ",
        "side of the layer or plane (about 0.05%, 0.3% and 2% of it), so a setting looks the ",
        "same at any size. Colour only; all three at 0 changes nothing; no frame, a selection ",
        "only confines where the result is written. In a 32-bit float document all three run on ",
        "the display-encoded layer, unclamped above white; dehaze measures the haze on that ",
        "encoding with values above white read as white, and leaves a component above white as ",
        "it is at a positive amount). ",
    ].joined()

    /// `apply_filter`'s account of Develop Filter, joined after
    /// `presenceText` for the same type-checker reason.
    static let developFilterText: String = [
        "develop_filter (Filters > Develop Filter\u{2026}: the Develop RAW window's global ",
        "controls on any layer, each 0 by default and 0 being off: temperature and tint ",
        "-100..100, exposure -4..4 stops, contrast, highlights, shadows, whites, blacks, ",
        "texture, clarity, dehaze, vibrance and saturation -100..100. They run in one fixed ",
        "order, each step skipped when its controls are 0: (1) white balance, temperature ",
        "being a RELATIVE shift of one mired a unit from a neutral picture (the white_balance ",
        "illuminant 1e6/(1e6/6504 - temperature) K, so + warms and - cools) and tint passed as ",
        "white_balance's tint (+ toward magenta); (2) exposure, a linear-light gain of ",
        "2^exposure; (3) contrast, the modern adjust S-curve at power 2^(1.5 x contrast/100); ",
        "(4) highlights and shadows, shadows_highlights at tone 0.5 with color 0 and amounts ",
        "shadows/100 (shadows/200 below 0) and -highlights/100 (-highlights/200 above 0), so + ",
        "shadows lifts and + highlights brightens, over the blurred luma of step 3's result at ",
        "radius 30 x longer side / 2048; (5) whites and blacks, a levels master stage moving ",
        "the white or black point by up to 0.2 (+ whites lowers white, - raises output_white; - ",
        "blacks raises black, + raises output_black); (6) texture, clarity and dehaze, exactly ",
        "presence's; (7) vibrance and saturation, vibrance's at value/100. Not re-editable: a ",
        "second run starts from the current pixels. Colour only; all thirteen at 0 changes ",
        "nothing; no frame, a selection only confines where the result is written. In a 32-bit ",
        "float document temperature, tint and exposure (v x 2^exposure) act on the stored linear ",
        "values; contrast, highlights, shadows, whites, blacks, texture, clarity and dehaze on ",
        "the display-encoded layer, unclamped (highlights and shadows leave a pixel whose ",
        "encoded luma is at or above white as it is); vibrance and saturation on the decoded ",
        "linear value scaled about white, as the vibrance adjustment runs at 32 bits). ",
        "develop_filter also takes masks (the sheet's Masks section): an array of at most 16 local ",
        "adjustments applied in order after the global controls, each an object with shape ",
        "(required): {op: radial, center: [x, y], radius: [rx, ry], hardness 0..1} (smoothstep ",
        "from hardness to the ellipse's edge), {op: linear, from: [x, y], to: [x, y]} (1 at from, ",
        "0 at to), {op: full}, {op: empty}, or {op: subject | people | sky} (made by Vision, or by ",
        "the sky model, on the layer before the filter runs; a missing sky model and a model that ",
        "finds nothing refuse by name); coordinates are 0..1 of the layer's width and height, rx ",
        "in widths and ry in heights, each above 0 and at most 2. brush: [{mode: add | subtract, ",
        "dabs: [{center: [x, y], radius (a share of the longer side), hardness 0..1, opacity ",
        "0..1}]}] painted over the shape, at most 4096 dabs and 256 million evaluated pixels a ",
        "stroke. invert (false) flips the coverage; density 0..1 (default 1) weights it; feather ",
        "0..100 (default 0) blurs it by feather/100 x 4% of the longer side, at most 256 px. range ",
        "narrows it as Develop's local range does: {luminance: {low, high, low_falloff?, ",
        "high_falloff?, invert?}, color: {lab: [L, a, b] or sample: [x, y], amount, falloff?, ",
        "invert?}} (falloffs default 0, invert false; range null is no range), read as CIE ",
        "L*a*b* of the layer's own pixels through the document's profile, so a local never ",
        "moves its own range. Each mask takes temperature, tint, exposure, ",
        "contrast, highlights, shadows, whites, blacks, texture, clarity, dehaze and saturation ",
        "in the global ranges (default 0; no vibrance), run as the global controls are over the ",
        "picture the earlier masks left, so overlapping masks compound, then noise_reduction ",
        "0..100 (reduce_noise at radius 0.5, luminance noise_reduction/1000, detail 0, color 0) ",
        "and sharpness -100..100 (above 0 smart_sharpen at amount 1.5 x sharpness %, radius 1, ",
        "noise 0, remove gaussian; below 0 gaussian_blur at radius 3 x -sharpness/100), both ",
        "default 0 and their radii fixed in pixels, and blended by the ",
        "coverage. masks are refused on a mask, colour plane or channel target ('develop_filter ",
        "masks edit layer pixels only'), on a 32-bit float layer ('develop_filter masks are not ",
        "available on a 32-bit float layer') and in CMYK. A recorded step keeps the subject, ",
        "people and sky shapes as they were asked for, so a replay runs the model again. ",
    ].joined()

    /// `apply_filter`'s account of Smart Sharpen, joined after
    /// `developFilterText` for the same type-checker reason.
    static let smartSharpenText: String = [
        "smart_sharpen (Filter > Sharpen > Smart Sharpen\u{2026}: amount 1-500 percent default ",
        "200; radius 0.1-64 px default 1; noise 0-100 percent default 10, Reduce Noise's ",
        "luminance smoothing (radius 0.5, luminance noise/1000) before the sharpen; remove ",
        "\"gaussian\", \"lens\" (default) or \"motion\", the blur the sharpen undoes: each ",
        "colour becomes D + amount/100 x (D - B), B that blur of the denoised picture D (a ",
        "Gaussian of sigma radius, a lens disc of the radius, or a line of round(2 x radius) px ",
        "at angle), so gaussian with noise 0 and no fade is unsharp_mask at the same radius ",
        "with threshold 0; lens removal also holds each colour within 2 levels of its 3x3 ",
        "neighbourhood's range, which keeps edges free of halos; angle -180..180 degrees ",
        "default 0, read only by motion; shadow_fade and highlight_fade 0-100 default 0, ",
        "shadow_tonal_width and highlight_tonal_width 0-100 default 50, shadow_radius and ",
        "highlight_radius integers 1-100 px default 1: the amount fades by up to fade% where ",
        "the neighbourhood luma (a box of that radius) is darker than the shadows' tonal width ",
        "or lighter than the highlights'. Colour only; never a no-op; no frame, a selection ",
        "only confines where the result is written. In a 32-bit float document the whole ",
        "filter runs on the display-encoded layer, unclamped above white, the tonal fades ",
        "reading luma above white as white). ",
    ].joined()

    @inline(never)
    func adjustmentCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        let autoProperties = schema.autoProperties
        let alphaWriteNote = schema.alphaWriteNote
        let levelsChannel: [String: Any] = [
            "type": "object",
            "description": "levels only: one channel's stage (r red, g green, b blue) — "
                + "black, white, gamma, output_black, output_white, as at the top level.",
        ]
        let levelsAuto: [String: Any] = [
            "type": "object",
            "description": "levels only, instead of params: {\"mode\": \"tone\" or \"color\", "
                + "\"clip\": fraction 0…0.1 dropped at each end, default 0.001}. Derives "
                + "per-channel r/g/b stages from the BACKDROP below the layer (see the tool "
                + "description); refused at 32-bit float. On a CMYK document levels and "
                + "curves take {\"algorithm\": \"per_channel\" or \"monochromatic\", \"shadows\", "
                + "\"highlights\": clip percent 0–9.99} (Auto Options; every key optional, the "
                + "saved defaults otherwise, 0.10 %): the ink stages (or the "
                + "composite's) from the histogram of L = 1 − ink.",
        ]
        return [
            tool(
                "add_adjustment_layer",
                "Adds a NON-DESTRUCTIVE adjustment layer above the active layer and selects "
                    + "it: while compositing it recolors everything below it, its own pixels "
                    + "are ignored, and its parameters stay editable with "
                    + "edit_adjustment_layer (get_document reports them) — same math as "
                    + "apply_filter's matching filters, but reversible. The layer always gets "
                    + "a mask gating where the adjustment applies: built from the current "
                    + "selection when one exists (which stays active), else reveal-all; "
                    + "brush/eraser strokes on the layer paint that mask. A colour in an "
                    + "adjustment's params is in the DOCUMENT's numbers, not authored sRGB "
                    + "\u{2014} an adjustment transforms the pixels it sits over. INSIDE A "
                    + "GROUP: in an isolated group (any blend mode but Pass Through, or a "
                    + "style) it recolors everything below it IN THAT GROUP; in a "
                    + "pass-through group — the default — it reaches the layers below the "
                    + "group too, unless some layer is clipped to that group, which isolates "
                    + "it. A mask or an opacity on a pass-through group does NOT isolate it: "
                    + "it restricts the adjustment to where the mask is white and scales it "
                    + "by the opacity. POSITION: by default the layer lands directly above "
                    + "the anchor layer (the active one unless layer names another); position "
                    + "\"top\" puts it at the top of the root stack and \"into_group\" makes it "
                    + "the topmost child of the anchor, which must then be a group. A new "
                    + "layer JOINS a clipping run rather than splitting it: when the entry "
                    + "immediately above the slot is clipped, the new layer is created clipped "
                    + "too, so no neighbour is re-based onto it. Both arguments work at 8 "
                    + "bits, 16 bits and 32-bit float. AUTO (levels only): auto {mode tone|color, "
                    + "clip?} instead of params is the Levels dialog's Auto Tone / Auto Color — "
                    + "per-channel stages measured over the whole visible backdrop where the "
                    + "layer lands (clipped or grouped alike), weighted by the "
                    + "selection, which becomes the new mask; the call runs and records the "
                    + "resolved params. Ops and their "
                    + "params: " + Self.adjustmentOps,
                [
                    "op": [
                        "type": "string",
                        "enum": AdjustmentLayerOp.allCases.map { $0.rawValue },
                        "description": "The adjustment operation.",
                    ],
                    "position": [
                        "type": "string",
                        "enum": LayerInsertPosition.allCases.map { $0.rawValue },
                        "description": "Where the new layer lands relative to the anchor "
                            + "(default above).",
                    ],
                    "layer": [
                        "type": "integer",
                        "description": "The ANCHOR the position is relative to (default: the "
                            + "active layer). A group is a legal anchor, and into_group "
                            + "requires one; ignored for top.",
                    ],
                    "params": [
                        "type": "object",
                        "description": "Parameters for op (see the tool description); omit "
                            + "for that op's defaults. posterize has no default: its levels "
                            + "is required, and color_lookup needs file or a complete table.",
                    ],
                    "auto": levelsAuto,
                    "name": [
                        "type": "string",
                        "description": "Layer name (default: the op's display name).",
                    ],
                    "document_id": docID,
                ], required: ["op"]),
            tool(
                "edit_adjustment_layer",
                "Changes an existing adjustment layer non-destructively by replacing its "
                    + "stored description — one undo step; pixels, mask, opacity, blend mode "
                    + "and stacking are untouched. params REPLACES the whole params object "
                    + "(pass every key you want kept — nothing is merged, with one stated "
                    + "exception); op without params "
                    + "switches the layer to that op's defaults. Ops and params exactly as in "
                    + "add_adjustment_layer. The exception is a color_lookup layer's LUT: "
                    + "because get_document has to elide its table, omitting kind, size, "
                    + "table, source_size, domain_min, domain_max, title and abstract_profile "
                    + "— or passing the elision placeholders back — keeps the LUT the layer "
                    + "already holds (an abstract look is re-baked through the document's "
                    + "profile), so "
                    + "{\"strength\": 0.5} alone is enough to restrength it. Pass file, an "
                    + "absolute .cube path or an .icc/.icm device-link or abstract profile, "
                    + "to replace the LUT instead. Errors when the target "
                    + "layer is not an adjustment layer. auto (levels only, instead of params) "
                    + "re-derives the stages from the whole visible backdrop below the layer, "
                    + "clipped or inside an isolated group alike, and a live "
                    + "selection does not narrow it; only the layer's own mask weights it, "
                    + "exactly as the dialog's Auto buttons; the resolved params are "
                    + "what is applied and recorded.",
                [
                    "layer": index,
                    "op": [
                        "type": "string",
                        "enum": AdjustmentLayerOp.allCases.map { $0.rawValue },
                        "description": "New operation; omit to keep the layer's current op.",
                    ],
                    "params": [
                        "type": "object",
                        "description": "Replacement params object for the op (see "
                            + "add_adjustment_layer).",
                    ],
                    "auto": levelsAuto,
                    "document_id": docID,
                ]),
            tool(
                "auto_tone",
                "Stretches EACH channel's own histogram to the full range, clipping clip of "
                    + "the counted pixels at each end (default 0.001 = 0.1 %; "
                    + "fully transparent pixels are never counted), then applies the "
                    + "levels math destructively to the target through selection coverage, "
                    + "using analysis_scope (whole_target by default) independently. Both depths retain native samples; 16-bit supports layer RGB. One undo step. Reports the "
                    + "derived black, white and gamma per channel; changed is false when the "
                    + "image is already at full range (nothing is written). Errors on an "
                    + "adjustment layer's pixel target; its mask is supported. The UI path is Image > Auto Tone (\u{21E7}\u{2318}L). "
                    + "A 32-bit float document refuses all three (auto_tone, auto_contrast, auto_color).",
                autoProperties),
            tool(
                "auto_contrast",
                "Auto Tone with ONE black and white point derived from the luma histogram "
                    + "and applied to all three channels, so the colour balance is untouched. "
                    + "The UI path is Image > Auto Contrast (\u{2325}\u{21E7}\u{2318}L).",
                autoProperties),
            tool(
                "auto_color",
                "Auto Tone's per-channel stretch plus a per-channel gamma that snaps the "
                    + "midtones neutral. The gamma is derived only from near-neutral midtone "
                    + "pixels and is clamped to 0.5-2.0, so a legitimately coloured scene "
                    + "\u{2014} a sunset, a forest, a brick wall \u{2014} is not neutralised; when too "
                    + "few pixels qualify every gamma stays 1. This is a neutral-candidate "
                    + "snap. The UI path is Image > Auto Color "
                    + "(\u{21E7}\u{2318}B).",
                autoProperties),
            tool(
                "apply_filter",
                "Applies a filter or adjustment DESTRUCTIVELY through active selection "
                    + "coverage, preserving every stored code outside coverage in both depths. 16-bit targets layer RGB; byte mode also supports scalar planes. Neighborhood filters "
                    + "sample the whole target. With no selection they edit the whole target. "
                    + "Supports layer pixels, masks, color planes and saved channels (prefer "
                    + "add_adjustment_layer for a reversible color adjustment; this tool "
                    + "refuses adjustment/group pixel targets but supports their masks). Filters and their "
                    + "parameters: grayscale, invert, sepia, edge_detect, emboss (none); "
                    + "blur (sigma above 0, at most 1000 at 8 bits and 64 at 16 bits, default 4); sharpen (amount, default 1); "
                    + "unsharp_mask (radius 0-64 default 1.5, amount 0-5 default 1 where 1 is "
                    + "100%, threshold 0-1 default 0 in normalized contrast where 0.0039216 is "
                    + "one 8-bit level); high_pass (radius 0-64, default 3); "
                    + "reduce_noise (radius 0-64 default 1, luminance 0-0.25 default 0.01 in "
                    + "normalized units where 0.0039216 is one 8-bit level \u{2014} the noise "
                    + "LEVEL to remove, so it decides what counts as grain \u{2014} detail and "
                    + "color each 0-1 default 0.5, where luminance 0 or detail 1 turns the "
                    + "luminance half off and color 0 turns the colour half off, and both off "
                    + "changes nothing); dust_and_scratches (radius 0-64 default 2, the radius "
                    + "of a DISC of whole source pixels where below 1 changes nothing, "
                    + "threshold 0-1 default 0 in normalized contrast, where 0 is a plain "
                    + "median rather than a bypass) \u{2014} the photographic "
                    + "sharpening and noise-cleanup filters, one kernel at both depths, writing "
                    + "colour only, so alpha stays exact in both depths. adjust "
                    + "(brightness, contrast, saturation, each -1..1, default 0; legacy "
                    + "true/false, absent = true, as add_adjustment_layer's bcs \u{2014} false "
                    + "is the modern form that keeps black and white fixed); levels "
                    + "(normalized black 0-1, white 0-1, gamma 0.1-10, output_black and "
                    + "output_white 0-1, and optional r/g/b objects of the same five keys for "
                    + "per-channel stages, applied before the master one \u{2014} all flat beside "
                    + "filter, as add_adjustment_layer's levels; 16-bit code N is N/65535); "
                    + "hue_rotate (degrees); threshold "
                    + "(level 0-1); posterize (levels 2-64); pixelate (block 1-1024); "
                    + "add_noise (amount 0-1, seed); box_blur (radius, an integer 1-1000 "
                    + "default 5: the plain mean of a (2r+1)x(2r+1) clamped-edge window, which "
                    + "filters ALPHA as gaussian_blur does); surface_blur (radius an integer "
                    + "1-100 default 5, threshold 2-255 default 15 in 8-bit LEVELS at every "
                    + "depth \u{2014} unlike the normalized 0-1 thresholds above \u{2014} where a "
                    + "step of 2.5x the threshold or more is left alone; colour only); "
                    + "add_grain (amount 0-100 default 25, where 100 is an RMS of 25.5 levels "
                    + "at mid-grey and black and white are never changed; size 0.5-4 px "
                    + "default 1; roughness 0-100 default 50; seed, an integer, default 1; "
                    + "amount 0 changes nothing; colour only); "
                    + "radial_blur (Filters > Blur > Radial Blur\u{2026}: amount an integer 1-100 "
                    + "default 10, the arc in degrees for method \"spin\" (default) or the "
                    + "streak as amount x 0.5% of the distance from the centre for \"zoom\"; "
                    + "quality \"draft\" (grainy, deterministic), \"good\" (default) or "
                    + "\"best\"; center_x and center_y 0-1 default 0.5; filters alpha); "
                    + "clouds and difference_clouds (Filters > Render > Clouds / Difference "
                    + "Clouds: seed, an integer, default 1; foreground_color and "
                    + "background_color \"#rrggbb\", default #000000 and #ffffff \u{2014} not "
                    + "the swatches; high_contrast true/false default false, the menu's "
                    + "Option-click; clouds writes opaque pixels, difference_clouds is "
                    + "|pixel - clouds| on colour and keeps alpha); offset (Filters > Other > "
                    + "Offset\u{2026}: horizontal and vertical integers in pixels, default 0, "
                    + "positive moving content right and down, any size allowed; "
                    + "undefined_areas \"wrap\" (default), \"repeat\" (edge pixels), "
                    + "\"transparent\" or \"background\" \u{2014} on a layer both leave the "
                    + "vacated area transparent (there is no Background layer), on a mask "
                    + "they fill white, on a red/green/blue plane that component of "
                    + "background_color (default #ffffff), on the alpha plane or a channel "
                    + "its Rec. 709 gray; both offsets 0 changes nothing); "
                    + Self.motionLensTiltText
                    + Self.lensCorrectionText
                    + Self.presenceText
                    + Self.developFilterText
                    + Self.smartSharpenText
                    + "WITH A SELECTION, "
                    + "radial_blur's centre is a fraction of the selection's bounding box on "
                    + "the target and offset shifts and wraps within that box; "
                    + "without one both use the whole layer or plane. Colour-only filters leave "
                    + "alpha exact and fully transparent pixels untouched. Every adjustment-layer op also runs "
                    + "destructively here \u{2014} curves (rgb/r/g/b arrays of fractional [input,output] points in the historical 0…255 space, or rgb_map/r_map/g_map/b_map arrays of 256 integers 0-255), exposure, vibrance, hue_saturation, "
                    + "color_balance, black_and_white, photo_filter, channel_mixer, "
                    + "selective_color, shadows_highlights, white_balance, gradient_map, "
                    + "color_lookup, replace_color \u{2014} with its parameters in a params OBJECT (not flat), "
                    + "exactly as add_adjustment_layer takes them. Image > Adjustments > "
                    + "Desaturate (\u{21E7}\u{2318}U) is hue_saturation with {\"params\": "
                    + "{\"saturation\": -1}}, which is what it records. The older filters listed "
                    + "first read their arguments FLAT. Each style REFUSES the other rather "
                    + "than ignoring it: posterize takes levels beside filter and is refused "
                    + "a params object, and exposure takes {\"params\": {\"gamma\": 0.4}} and "
                    + "is refused a bare gamma \u{2014} which would otherwise have applied the "
                    + "op's defaults and reported success. A flat filter also refuses, by "
                    + "name, any key it does not take (a misspelt \"sigmaa\" is an error, not "
                    + "the default), and a numeric argument given as null or true/false.",
                [
                    "filter": [
                        "type": "string",
                        "enum": [
                            "grayscale", "invert", "sepia", "edge_detect", "emboss", "blur",
                            "sharpen", "unsharp_mask", "high_pass",
                            "reduce_noise", "dust_and_scratches",
                            "adjust", "levels", "curves", "hue_rotate", "threshold",
                            "posterize", "pixelate", "add_noise",
                            "box_blur", "surface_blur", "add_grain",
                            "radial_blur", "clouds", "difference_clouds", "offset",
                            "motion_blur", "lens_blur", "tilt_shift", "lens_correction",
                            "presence", "develop_filter", "smart_sharpen",
                            "exposure", "vibrance", "hue_saturation", "color_balance",
                            "black_and_white", "photo_filter", "channel_mixer",
                            "selective_color", "shadows_highlights", "white_balance",
                            "gradient_map", "color_lookup", "replace_color",
                        ],
                    ],
                    "layer": index,
                    "sigma": ["type": "number"], "amount": ["type": "number"],
                    "radius": ["type": "number"], "threshold": ["type": "number"],
                    "luminance": ["type": "number"], "detail": ["type": "number"],
                    "color": ["type": "number"],
                    "brightness": ["type": "number"], "contrast": ["type": "number"],
                    "saturation": ["type": "number"], "legacy": ["type": "boolean"],
                    "black": ["type": "number"],
                    "white": ["type": "number"], "gamma": ["type": "number"],
                    "output_black": ["type": "number"], "output_white": ["type": "number"],
                    "r": levelsChannel, "g": levelsChannel, "b": levelsChannel,
                    "degrees": ["type": "number"], "level": ["type": "number"],
                    "levels": ["type": "integer"], "block": ["type": "integer"],
                    "seed": ["type": "integer"],
                    "size": ["type": "number"], "roughness": ["type": "number"],
                    "method": ["type": "string", "enum": ["spin", "zoom"]],
                    "quality": ["type": "string", "enum": ["draft", "good", "best"]],
                    "center_x": ["type": "number"], "center_y": ["type": "number"],
                    "distance": ["type": "integer"], "angle": ["type": "number"],
                    "focus": ["type": "number"], "transition": ["type": "number"],
                    "distortion": ["type": "number"], "chromatic_red_cyan": ["type": "number"],
                    "chromatic_blue_yellow": ["type": "number"], "vignette_amount": ["type": "number"],
                    "vignette_midpoint": ["type": "number"], "vertical_perspective": ["type": "number"],
                    "horizontal_perspective": ["type": "number"], "scale": ["type": "number"],
                    "profile": ["type": ["string", "object"]],
                    "texture": ["type": "number"], "clarity": ["type": "number"],
                    "dehaze": ["type": "number"],
                    "temperature": ["type": "number"], "tint": ["type": "number"],
                    "exposure": ["type": "number"], "highlights": ["type": "number"],
                    "shadows": ["type": "number"], "whites": ["type": "number"],
                    "blacks": ["type": "number"], "vibrance": ["type": "number"],
                    "masks": [
                        "type": "array", "items": ["type": "object"],
                        "description": "develop_filter only: up to 16 local adjustments (see the "
                            + "develop_filter paragraph).",
                    ],
                    "noise": ["type": "number"],
                    "remove": ["type": "string", "enum": ["gaussian", "lens", "motion"]],
                    "shadow_fade": ["type": "number"], "shadow_tonal_width": ["type": "number"],
                    "shadow_radius": ["type": "integer"],
                    "highlight_fade": ["type": "number"], "highlight_tonal_width": ["type": "number"],
                    "highlight_radius": ["type": "integer"],
                    "foreground_color": ["type": "string"], "background_color": ["type": "string"],
                    "high_contrast": ["type": "boolean"],
                    "horizontal": ["type": "integer"], "vertical": ["type": "integer"],
                    "undefined_areas": [
                        "type": "string", "enum": ["background", "transparent", "repeat", "wrap"],
                    ],
                    "params": [
                        "type": "object",
                        "description": "Parameters for the adjustment ops (see "
                            + "add_adjustment_layer); the older filters take their "
                            + "arguments flat.",
                    ],
                    "target": [
                        "type": "string",
                        "description": "What the filter runs on: \"layer\" (default) the "
                            + "layer's own pixels, \"mask\" its mask, "
                            + "\"red\"/\"green\"/\"blue\"/\"alpha\" ONE "
                            + "colour plane of them, or \"channel:<name>\" one of the "
                            + "document's alpha channels (list_channels names them). A "
                            + "plane is 8-bit gray: the filter runs on it and the result's "
                            + "gray goes back in, leaving every other plane byte-identical."
                            + alphaWriteNote,
                    ],
                    "document_id": docID,
                ], required: ["filter"]),
        ]
    }
}
