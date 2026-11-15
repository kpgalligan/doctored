import AppKit

// The Background Eraser (`designs/background-eraser.md`): a brush-tip stroke that erases, on the
// active layer, the pixels under each dab whose colour is like a sampled colour, and keeps the
// rest. The core computes the coverage (`core/src/background_eraser.rs`): at 8 bits it erases
// through the paint walk's Clear in the same call; on a typed document it answers a coverage
// plane, which `erase_native` erases with the selection as its second gate, as the Magic
// Eraser's region is erased. This file holds the options value, its core request, the colours,
// the sentences shared by the canvas and `background_eraser_stroke`, and the recorded step.

/// A reference colour of the Background Eraser: the background swatch (Background Swatch
/// sampling) or the foreground (Protect Foreground). `lab` and `ink` are the swatch's stored Lab
/// and CMYK values, kept exactly on a Lab or CMYK document as the paint tools keep them.
struct BackgroundEraserColor: Equatable {
    var color: NSColor
    var lab: LabInk? = nil
    var ink: CmykInk? = nil

    /// The colour as the core's request reads it on `doc`: a byte document's own bytes over
    /// 255; a typed document's straight colour in its own space (a Lab swatch's stored Lab);
    /// a CMYK document's ink (`CmykPaint.ink`).
    func reference(in doc: RasterDocument) throws -> [String: Any] {
        if let source = doc.precision {
            if source.isCmyk {
                return ["ink": try CmykPaint.ink(color, stored: ink, in: source)]
            }
            let painted = LabPaint.color(color, stored: lab, in: doc)
            return ["color": try PrecisionInk(color: painted, space: doc.nsColorSpace).rgba,
                    "color_space": "document"]
        }
        guard let bytes = ColorProfile.bytes(color, in: doc.nsColorSpace), bytes.count == 4 else {
            throw DoctoredCoreError(message: "Could not convert the Background Eraser's colour.")
        }
        return ["color": bytes.map { Double($0) / 255 }]
    }

    /// The step's spelling: `<key>_color` as an sRGB hex, and `<key>_ink` (C, M, Y, K percent)
    /// when the colour carries a stored ink.
    func arguments(_ key: String) -> [String: Any] {
        var out: [String: Any] = ["\(key)_color": ActionArgs.hex(color)]
        if let ink { out["\(key)_ink"] = [ink.c, ink.m, ink.y, ink.k] }
        return out
    }
}

/// One Background Eraser stroke's options: Tolerance (0…255, in 8-bit codes; the core takes it
/// × 257), Limits, Sampling, the swatch (Background Swatch sampling only) and the protected
/// foreground (Protect Foreground on). The canvas and `background_eraser_stroke` both build one.
struct BackgroundEraserOptions: Equatable {
    enum Limits: String, CaseIterable {
        case contiguous, discontiguous
    }

    enum Sampling: String, CaseIterable {
        case continuous, once
        case swatch = "background_swatch"

        /// The core request's name.
        var coreName: String { self == .swatch ? "swatch" : rawValue }
    }

    var tolerance: Double
    var limits: Limits
    var sampling: Sampling
    var swatch: BackgroundEraserColor?
    var protect: BackgroundEraserColor?

    /// The core's request on `doc` (`dr_doc_background_eraser_layer`'s JSON).
    func request(in doc: RasterDocument) throws -> String {
        var fields: [String: Any] = [
            "version": 1, "tolerance": Int((min(max(tolerance, 0), 255) * 257).rounded()),
            "limits": limits.rawValue, "sampling": sampling.coreName,
        ]
        if sampling == .swatch, let swatch { fields["swatch"] = try swatch.reference(in: doc) }
        if let protect { fields["protect"] = try protect.reference(in: doc) }
        return try NativeSelectionWork.encoding(fields)
    }

    /// The options as `background_eraser_stroke` takes them, so a recorded step replays them.
    var arguments: [String: Any] {
        var out: [String: Any] = [
            "tolerance": Int(tolerance.rounded()), "limits": limits.rawValue,
            "sampling": sampling.rawValue,
        ]
        if sampling == .swatch, let swatch { out.merge(swatch.arguments("sample")) { $1 } }
        if let protect { out.merge(protect.arguments("protect")) { $1 } }
        return out
    }
}

/// The Background Eraser's sentences, shared by the canvas and `background_eraser_stroke`.
enum BackgroundEraser {
    static let name = "Background Eraser"

    /// The tip's default diameter in px: the options store's, and `background_eraser_stroke`'s
    /// `size` default for `points` with or without `stroke_version`.
    static let defaultSize: Double = 40

    /// A mask, a colour or ink plane, an alpha channel or the Quick Mask as the target.
    static let targetRefusal = "The Background Eraser erases a layer's own pixels: a mask, a "
        + "colour channel, an alpha channel and the Quick Mask hold no transparency of their own "
        + "to erase. Target the layer."

    /// A `blend_mode` argument: the erase has none.
    static let blendRefusal = "The Background Eraser has no blend mode: it lowers the "
        + "transparency of the pixels like its sampled colour and keeps their colour."

    /// A tolerance, `limits` or `sampling` outside the tool's.
    static let argumentRefusal = "background_eraser_stroke takes tolerance 0…255, limits "
        + "contiguous or discontiguous, and sampling continuous, once or background_swatch."

    /// The tip keys the tool has no use for.
    static let tipRefusal = "background_eraser_stroke has no flow, opacity, airbrush or aliased "
        + "tip: a pixel it reaches is erased by the tip's own coverage."

    /// Background Swatch sampling with no colour to erase.
    static let swatchMissing = "background_swatch sampling erases sample_color (or sample_ink "
        + "on a CMYK document); pass one."

    /// A sample colour with another sampling.
    static let swatchUnused = "sample_color and sample_ink name the colour background_swatch "
        + "sampling erases; the other samplings take theirs from the layer."

    /// Layer `idx`'s transparency lock (its own or an enclosing group's), which keeps exactly
    /// what the Background Eraser changes; nil when its alpha is free.
    static func transparencyRefusal(_ doc: RasterDocument, layer idx: Int)
        -> (message: String, owner: Int)? {
        guard let owner = doc.transparencyLockOwner(idx) else { return nil }
        let name = doc.layerInfo(idx)?.name ?? "this layer"
        let group = owner == idx ? nil : doc.layerInfo(owner)?.name ?? "this group"
        return (LockFlags.refusal(layerName: name, blocking: .transparency, group: group)
            + " The Background Eraser changes transparency, which the lock keeps.", owner)
    }
}

extension RasterDocument {
    /// The 8-bit Background Eraser on layer `idx` along `stroke` (one call: the core's
    /// coverage erased through the paint walk's Clear). nil when nothing would change.
    func backgroundErased(_ idx: Int, options: BackgroundEraserOptions, stroke: NativeBrushStroke,
                          selection: [UInt8]?) throws -> RasterDocument? {
        try backgroundEraserLayer(idx, request: options.request(in: self),
            brushRequest: stroke.request, dabs: stroke.dabs, selection: selection)
    }
}
