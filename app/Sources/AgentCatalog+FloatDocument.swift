import Foundation

/// The Phase 11F catalog: the two tone-map tools, plus the annotations that
/// teach the shared tools what a 32-bit float document does to them.
///
/// Kept in its own file behind an explicit non-inlining boundary, for the
/// reason `AgentCatalog.swift`'s header gives: one giant literal made Release
/// WMO spend minutes in `COWArrayOpts` / `ColdBlockInfo`.
extension AgentServer {
    @inline(never) func floatCatalog() -> [[String: Any]] {
        let docID: [String: Any] = [
            "type": "string",
            "description": "Which open document; defaults to the frontmost.",
        ]
        return [
            tool(
                "get_tone_map",
                "How this 32-bit float document's extended-range linear samples become SDR "
                    + "numbers — on screen and in every 8-bit or 16-bit deliverable. "
                    + "\"clip\" pins everything at or above white to white; \"reinhard\" maps "
                    + "the white point exactly to white and compresses everything below it, so "
                    + "highlights above 1.0 stay distinguishable. The reply also carries the "
                    + "float operator table — what the mode evaluates and what it refuses, "
                    + "with the acceptance criterion for each refusal — and float_refusal, the "
                    + "sentence naming the first operator this document could not evaluate. "
                    + "Reads only; refused on a document that is not 32-bit float.",
                ["document_id": docID]),
            tool(
                "set_tone_map",
                "Chooses this 32-bit float document's tone map — Image ▸ Tone Map…. One undo "
                    + "step; a map the document already has is reported unchanged rather than "
                    + "failing. \"white\" belongs to \"reinhard\" ONLY (1…65504, 4 by default) "
                    + "and is refused on \"clip\" rather than ignored. The map never touches "
                    + "the stored samples: a 32-bit float export writes them exactly as they "
                    + "are, and undo restores the previous map.",
                [
                    "document_id": docID,
                    "operator": [
                        "type": "string", "enum": ToneMapOperator.allCases.map(\.rawValue),
                        "description": "The tone map operator to apply.",
                    ],
                    "white": [
                        "type": "number",
                        "description": "Reinhard's white point in linear multiples of diffuse "
                            + "white, 1…65504. Refused with \"clip\".",
                    ],
                ], required: ["operator"]),
        ]
    }

    /// The filtered-geometry rule at 32-bit float, one sentence for
    /// every tool that resamples there.
    static let floatFilteredGeometry =
        " Available in a 32-bit float document: the resample is premultiplied over the "
        + "extended-range samples, each layer's RGB is clamped to that layer's own visible "
        + "range so a filter's ringing never leaves it, an output alpha below 2^-24 becomes "
        + "transparent, and a float result past 67,108,864 pixels refuses before any work. "
        + "Masks stay 16-bit coverage and resample exactly as at 16 bits."

    @inline(never) func annotateFloatCatalog(_ catalog: inout [[String: Any]]) {
        for index in catalog.indices {
            guard let name = catalog[index]["name"] as? String else { continue }
            let existing = (catalog[index]["description"] as? String) ?? ""
            switch name {
            case "open_document":
                catalog[index]["description"] = existing
                    + " A genuine floating-point TIFF, an OpenEXR file (.exr) and a Radiance "
                    + "RGBE file (.hdr) open as a 32-BIT FLOAT document: linear "
                    + "light with an extended range, where values above white and below zero "
                    + "are kept exactly. precision:\"rgba8\" is refused on one — convert "
                    + "explicitly after opening instead. OpenEXR and Radiance carry no ICC "
                    + "profile: stated Rec.709 primaries (or none) open as sRGB, Display P3 "
                    + "primaries as Display P3, and any other primaries are converted once, in "
                    + "linear light, to sRGB primaries — the import warnings say which. An "
                    + "OpenEXR file opens its first part that is not deep and has R, G and B "
                    + "channels, on a canvas the size of its display window, with its "
                    + "associated alpha unpremultiplied; a file holding a deep part or no RGB "
                    + "part is refused by name. A Radiance file opens opaque; its EXPOSURE and "
                    + "COLORCORR lines are reported and not applied."
            case "get_document":
                catalog[index]["description"] = existing
                    + " A 32-bit float document also carries a \"float\" block: its storage, "
                    + "its extended range, its tone map and projection_error, the sentence "
                    + "naming any operator it cannot evaluate."
            case "convert_precision":
                catalog[index]["description"] = existing
                    + " On a 32-bit float document BOTH modes apply the document's tone map: "
                    + "rgba16 keeps 65,536 values per channel and stays linear-light, rgba8 "
                    + "keeps 256. mode rgba32 converts an 8- or 16-bit document INTO 32-bit "
                    + "float (structure \"preserve\" only): each value is decoded through the "
                    + "profile's tone curve to linear light, which invents no HDR information, "
                    + "and converting back returns every original value. structure:\"preserve\" "
                    + "is admitted on one in either mode: every layer is carried and each "
                    + "raster is tone-mapped per record (then quantized for rgba8, RGB per "
                    + "dither), with the result linear-light."
            case "export_image", "export_preview":
                catalog[index]["description"] = existing
                    + " bit_depth 32 is the 32-bit float document's extended-range deliverable, "
                    + "in one of three containers: format \"tiff\" (four samples, unassociated "
                    + "alpha, Deflate, the document profile embedded), \"exr\" (OpenEXR: RGBA, "
                    + "ZIP scanlines, associated alpha, the document profile's primaries as "
                    + "chromaticities) or \"hdr\" (Radiance RGBE: opaque, each colour "
                    + "multiplied by its alpha, values below zero written as zero, a "
                    + "PRIMARIES line). float_samples \"f16\" writes a tiff or exr at half "
                    + "width, each sample the nearest 16-bit half; the reply's float_samples "
                    + "says f32, f16 or rgbe. \"exr\" and \"hdr\" exist only at bit_depth 32 "
                    + "and convert no pixel. A 32-bit export requires no sharpen, profile "
                    + "\"document\" and dither \"none\". The preview is decoded from the "
                    + "written bytes and shown under the document's tone map. A size mode "
                    + "resamples the composite "
                    + "through Image Size's extended-range resample (premultiplied, RGB clamped "
                    + "to the composite's own visible range, an alpha below 2^-24 transparent); "
                    + "size mode none writes the samples exactly. At 8 or 16 bits the document's "
                    + "tone map runs first. hdr_encoding (v1.x) makes a 32-bit heic an HDR "
                    + "photo: \"gain_map\" (an SDR base with an ISO 21496-1 gain map, the "
                    + "document profile's primaries, 1.0 written as the base's white) or \"pq\" "
                    + "(BT.2100 PQ in BT.2020, 1.0 written as 203 cd/m2); it requires bit_depth 32 "
                    + "and heic or avif, and the reply adds hdr_encoding and content_headroom (the "
                    + "largest linear value written, at least 1). This system's AVIF encoder "
                    + "writes no HDR AVIF, so an avif request refuses by name after the encode. "
                    + "Without hdr_encoding a 32-bit heic or avif is refused."
            case "histogram":
                catalog[index]["description"] = existing
                    + " On a 32-bit float document (storage \"rgba32_float\") the bins are "
                    + "STOPS — 256 bins over −12…+6 EV — with per-channel below_zero/"
                    + "above_white counts and their coverage weights "
                    + "(below_zero_weight_units_rgba / above_white_weight_units_rgba), the "
                    + "counts of values exactly 0 and exactly 1 (exact_zero_rgba / "
                    + "exact_one_rgba and their _weight_units_rgba), "
                    + "weighted linear sums and exact min/max instead of clipping counts. "
                    + "Every source (composite, layer, backdrop) and selection apply as at 8 "
                    + "and 16 bits. source \"scalar\" measures a mask or channel:<name>, which "
                    + "are u16 coverage, exactly as at 16 bits (storage \"u16_scalar\", codes "
                    + "0…65535, not stops); red, green, blue and alpha refuse, because a float "
                    + "layer's components are not u16 codes. Use source \"layer\" for their "
                    + "stop-binned histogram."
            case "sample_pixel", "sample_color":
                catalog[index]["description"] = existing
                    + " On a 32-bit float document the reply is linear components rather "
                    + "than codes — center_rgba32, mean_rgba32, weighted_sum_rgba32, "
                    + "weight_units, stops and above_white/below_zero flags; every source and "
                    + "selection apply as at 8 and 16 bits."
            // The filtered geometry at float. `rotate` and `crop` state
            // the same rule in their own entries (`AgentCatalog+GeometryLayer.swift`).
            // `warp_layer` takes the same resampler.
            case "image_size", "transform_layer", "transform_layers", "distort_layer",
                 "transform_mask", "warp_layer":
                catalog[index]["description"] = existing + Self.floatFilteredGeometry
            // The float adjustment forms, measured against Photoshop.
            case "add_adjustment_layer", "edit_adjustment_layer":
                catalog[index]["description"] = existing
                    + " In a 32-bit float document Levels, bcs, Black & White, "
                    + "Channel Mixer, Photo Filter, Exposure, White Balance, Curves, "
                    + "Hue/Saturation (Colorize, master lightness alone, or master hue alone) "
                    + "and Color Balance "
                    + "evaluate the linear values unclipped. "
                    + "Color Lookup runs every table (3D or 1D, any domain, entries inside or "
                    + "outside 0 to 1, a device link or an abstract look) on the linear values: "
                    + "an input outside the table's domain is read at its nearest end, entries "
                    + "above 1 are reached, and the unloaded identity table passes through. "
                    + "Channel Mixer's "
                    + "coefficients are kept in 1/1024 steps, and at 32 "
                    + "bits its constant adds half its 16-bit amount. A point curve continues "
                    + "along its end slope above its last point, and below its first point only "
                    + "when that point sits at input 0; a map (pencil) curve reads each map as "
                    + "straight lines between its entries, its first and last segments "
                    + "continued past 0 and 1, the channel map before the master. "
                    + "Hue/Saturation's lightness leaves a component already past its target "
                    + "unchanged, and its hue turns on a 1/24576-turn grid; saturation, "
                    + "hue with another master slider, or any band edit takes the display-scaled "
                    + "form below. Color "
                    + "Balance continues each channel beyond 0 and 1 along its end "
                    + "slopes. Black & White's Tint brings the tint's numbers to the grey's "
                    + "luminosity with the grey capped at 16. Hue Rotate and "
                    + "Sepia apply their matrix and Grayscale the Rec. 709 luma to the linear "
                    + "values with no clamp. Threshold, Posterize and Gradient Map are "
                    + "display-mapped: the op runs on the display-encoded value clamped to 0 to 1 "
                    + "and its result is decoded, so above white classifies as white. Vibrance, "
                    + "Selective Color, Replace Color and those Hue/Saturation settings are "
                    + "display-scaled: a pixel is divided by m = max(1, its largest component), "
                    + "the op runs on that colour's display encoding and the decoded result is "
                    + "scaled back by m, so a highlight is treated as its colour at m times the "
                    + "intensity. Invert (1 - v has no meaning above 1.0), Shadows/Highlights "
                    + "(its luma curve is defined on 0 to 1 and reverses above white) and a "
                    + "legacy_srgb White Balance (its matrix acts on sRGB-encoded 0 to 1 numbers, "
                    + "and a value above 1.0 has no encoded form) refuse by name."
            // The destructive twin, the layer's own form.
            case "apply_filter":
                catalog[index]["description"] = existing
                    + " In a 32-bit float document every adjustment its layer admits — levels, "
                    + "black_and_white, channel_mixer, photo_filter, exposure, curves (point or "
                    + "map form), color_balance, color_lookup, adjust (bcs), hue_saturation, a "
                    + "document_v2 white_balance, hue_rotate, grayscale, sepia, threshold, "
                    + "posterize, vibrance, selective_color, gradient_map and replace_color — "
                    + "applies to a layer's pixels "
                    + "exactly as their adjustment layer renders, lerped by the selection with "
                    + "no clamp. Twenty-three filters run "
                    + "on a float layer's pixels, through the same kernels as the other depths, "
                    + "each under one stated rule. In linear light, with no clamp to 0 to 1: "
                    + "blur, box_blur, motion_blur, radial_blur, lens_blur, tilt_shift, offset, "
                    + "lens_correction, unsharp_mask and high_pass (liquify is its own tool), "
                    + "pixelate (the premultiplied mean of the cell's light) and add_noise "
                    + "(v x (1 + delta), so black stays black and a highlight gets proportional "
                    + "grain). A blurred highlight above white spreads its whole energy; "
                    + "unsharp_mask and high_pass may write values below 0, which the document "
                    + "keeps; high_pass adds the stored value 0.5 (linear, lighter than "
                    + "mid-grey on screen); unsharp_mask's threshold compares linear "
                    + "differences; lens_blur's threshold reads linear luma, so a pixel above "
                    + "white always takes the full highlight weight; lens_correction's vignette "
                    + "multiplies the linear values by 2^stops, and its resample is clamped to "
                    + "the range the layer's visible pixels hold. On the layer's DISPLAY "
                    + "ENCODING (the profile's curve to white, a logarithm above it, decoded "
                    + "back; a component the filter leaves alone keeps its bits), so their "
                    + "thresholds and amounts mean what they mean at 16 bits: surface_blur, "
                    + "dust_and_scratches, reduce_noise, add_grain, clouds and "
                    + "difference_clouds (the colours mixed in encoded units), emboss and "
                    + "edge_detect (kept inside the encoded range the layer holds), sharpen "
                    + "(unsharp mask amount 1 on the encoding; unsharp_mask itself stays "
                    + "linear), presence (texture, clarity and dehaze), smart_sharpen (the "
                    + "whole filter; its tonal fades read luma above white as white) and "
                    + "develop_filter "
                    + "(temperature, tint and exposure on the linear values first; contrast, "
                    + "highlights, shadows, whites, blacks, texture, clarity and dehaze on the "
                    + "encoding; vibrance and saturation on the decoded linear value, scaled "
                    + "about white). dehaze measures its haze with values above white read as "
                    + "white and, at a positive amount, leaves a component above white as it "
                    + "is; highlights and shadows leave a pixel whose encoded luma is at or "
                    + "above white as it is. A value above white is never clamped to it. Alpha stays in 0 to 1, a result alpha below 2^-24 is "
                    + "transparent, and a result outside +/-65504 refuses. blur "
                    + "is an exact windowed Gaussian there, so its time grows with sigma (about "
                    + "a minute and a half at sigma 64 on 24 MP; it is cancellable). The reply's "
                    + "storage is rgba32_float. Invert, shadows_highlights, a legacy_srgb "
                    + "white_balance and the remaining filters refuse by name. Every filter "
                    + "but develop_filter also runs on target mask, channel:<name> or one of red, "
                    + "green, blue and alpha: the plane is read as opaque gray; a mask, channel "
                    + "or alpha is coverage (never display-encoded) and its result is clamped "
                    + "to 0..1; red, green and blue are linear light, unclamped, each filter in "
                    + "its own domain; a result whose three components differ is their Rec. 709 "
                    + "luma. add_noise on coverage is the 16-bit additive draw. develop_filter on "
                    + "such a target refuses by name, and an Image > Adjustments op there "
                    + "refuses too. The reply's storage is rgba32_float for red, green, blue "
                    + "and alpha and u16 for a mask or channel."
            case "transform_again":
                catalog[index]["description"] = existing
                    + " In a 32-bit float document it repeats a layer transform, and with a "
                    + "selection a Free Transform of the selected pixels (the floated "
                    + "pixels' RGB is clamped to their own visible range)."
                    + Self.floatFilteredGeometry
            default: continue
            }
        }
    }
}
