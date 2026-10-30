import AppKit

// #1 Lab phase 4c: copy and paste across Lab Color and the other colour modes, host side
// (`GrayClipboard.swift` is the template). The core converts every PIXEL paste
// (`lab_clipboard`: into Lab by Image ▸ Mode ▸ Lab Color's rule, relative colorimetric with no
// BPC; out of Lab relative colorimetric with BPC; Gray and CMYK through 16-bit ProPhoto RGB);
// the pasteboard's TIFF for other apps stays the RGB rendering. This file takes
// the one host step: `LabLayerClip.converted`, a LAYERS clip brought into the destination's mode
// before the core's layers paste, which refuses a Lab mix by name — the mode conversions
// themselves (`convertToLab` / `convertLabToRGB`), a blocker surfacing as the core's sentence.

/// A layers clip crossing into or out of Lab Color (module comment).
enum LabLayerClip {
    /// `clip` in `doc`'s colour mode and the conversion it crossed; nil when no host step is
    /// needed (no Lab side, or Lab → Lab, whose one twin keeps the codes under either mode).
    /// A 16-bit Lab clip into an 8-bit Lab document is put on the 8-bit grid first
    /// (`labDepthEdit(bits: 8)`, with no conversion). Don't Convert across modes refuses. RGB →
    /// Lab promotes an 8-bit clip and narrows a float one to 16 bits, then runs Mode ▸ Lab
    /// Color at the document's depth; an 8 or 16-bit Gray clip converts directly (Mode ▸ Lab
    /// Color's Gray route; a 32-bit one is narrowed to 16 bits first); a CMYK clip alone goes
    /// through 16-bit ProPhoto RGB first (CMYK → Lab's stage). Lab → RGB is Mode ▸ RGB Color
    /// into the document's profile, relative colorimetric with BPC, fitted to its depth; Lab →
    /// Gray and Lab → CMYK go to 16-bit ProPhoto RGB, then `GrayLayerClip` / `CmykLayerClip`.
    static func converted(_ clip: RasterDocument, into doc: RasterDocument,
                          mode: PasteProfile) throws
        -> (clip: RasterDocument, conversion: ColorConversion?)? {
        guard clip.isLab || doc.isLab else { return nil }
        if clip.isLab, doc.isLab {
            guard clip.labDepth == 16, doc.labDepth == 8, let source = clip.precision else {
                return nil
            }
            let budget = try PrecisionWorkContext(source: source)
            guard let eight = try source.labDepthEdit(
                bits: 8, workingLimit: budget.limit, otherRetained: budget.otherRetained,
                control: nil) else { return nil }
            return (try RasterDocument(precision: eight), nil)
        }
        guard mode == .convert else {
            throw DoctoredCoreError(message: ColorConversion.crossModePreserveLab)
        }
        let conversion = ColorConversion.layers(clip, into: doc, mode: mode)
        if doc.isLab {
            return (try toLab(try labSource(clip), into: doc), conversion)
        }
        guard let source = clip.precision else {
            throw DoctoredCoreError(message: "The copied Lab Color layers have no typed pixels.")
        }
        if doc.isGray || doc.isCmyk {
            let proPhoto = RasterProfile.builtin(.proPhotoRGB)
            let rgb = try RasterDocument(precision: toRGB(source, profile: proPhoto, into: doc))
            let crossed = doc.isGray
                ? try GrayLayerClip.converted(rgb, into: doc, mode: .convert)
                : try CmykLayerClip.converted(rgb, into: doc, mode: .convert)
            return (crossed?.clip ?? rgb, conversion)
        }
        let rgb = try toRGB(source, profile: doc.iccProfile ?? RasterProfile.builtin(.sRGB),
                            into: doc)
        return (try GrayLayerClip.fitted(rgb, to: doc), conversion)
    }

    /// The clip's typed pixels Mode ▸ Lab Color converts from (`converted`): an 8-bit RGB clip
    /// promoted, a float clip narrowed to 16 bits, a CMYK clip through 16-bit ProPhoto RGB.
    private static func labSource(_ clip: RasterDocument) throws -> RasterPrecisionDocument {
        guard let precision = clip.precision else { return try clip.promotedStructure() }
        let budget = try PrecisionWorkContext(source: precision)
        if precision.isCmyk {
            return try precision.convertCMYKToRGB(
                profile: RasterProfile.builtin(.proPhotoRGB),
                options: CmykModeConversion.rgbStageOptions, workingLimit: budget.limit,
                otherRetained: budget.otherRetained, control: nil)
        }
        if precision.isFloat {
            return try precision.narrowedStructureToU16(workingLimit: budget.limit,
                                                        otherRetained: budget.otherRetained)
        }
        return precision
    }

    /// `source` through Image ▸ Mode ▸ Lab Color at `doc`'s depth.
    private static func toLab(_ source: RasterPrecisionDocument,
                              into doc: RasterDocument) throws -> RasterDocument {
        do {
            let budget = try PrecisionWorkContext(source: source)
            let lab = try source.convertToLab(
                options: CmykModeConversion.labOptions(flatten: false, eightBit: doc.labDepth == 8),
                workingLimit: budget.limit, otherRetained: budget.otherRetained, control: nil)
            return try RasterDocument(precision: lab)
        } catch {
            throw DoctoredCoreError(message: "The copied layers cannot be pasted into a Lab Color "
                + "document: " + ((error as? DoctoredCoreError)?.message ?? error.localizedDescription))
        }
    }

    /// A Lab clip through Image ▸ Mode ▸ RGB Color into `profile`, relative colorimetric with BPC.
    private static func toRGB(_ lab: RasterPrecisionDocument, profile: Data,
                              into doc: RasterDocument) throws -> RasterPrecisionDocument {
        do {
            let budget = try PrecisionWorkContext(source: lab)
            return try lab.convertLabToRGB(profile: profile,
                                           options: ["black_point_compensation": true],
                                           workingLimit: budget.limit,
                                           otherRetained: budget.otherRetained, control: nil)
        } catch {
            let mode = doc.isCmyk ? "a CMYK" : doc.isGray ? "a Grayscale" : "an RGB"
            throw DoctoredCoreError(message: "The copied layers cannot be pasted into \(mode) "
                + "document: " + ((error as? DoctoredCoreError)?.message ?? error.localizedDescription))
        }
    }
}

extension LayerPasteWork {
    /// #1 Lab phase 4c: `converted`'s Lab arms (`grayConverted`'s shape). A clip of another mode
    /// is brought into the document's here (`LabLayerClip.converted`) and the core splices it as
    /// it is; between Lab documents the core keeps the codes under either mode (a 16-bit clip
    /// into an 8-bit document put on its grid here first).
    static func labConverted(_ entry: LayerClipboard.Entry, into doc: RasterDocument,
                             profile mode: PasteProfile) throws
        -> (clip: RasterDocument, narrowed: Bool, promoted: Bool, profileConverted: Bool,
            profile: String, core: PasteProfile, conversion: ColorConversion?) {
        let clip = entry.layers
        let source = ColorConversion.profileData(of: clip)
        let destination = ColorConversion.profileData(of: doc)
        if let crossed = try LabLayerClip.converted(clip, into: doc, mode: mode) {
            let from = LayerPastePlan.depth(of: clip), to = LayerPastePlan.depth(of: doc)
            let crossesMode = clip.isLab != doc.isLab
            return (crossed.clip, from > to || (clip.labDepth ?? 0) > (doc.labDepth ?? 0),
                    from < to, crossesMode,
                    crossesMode ? "converted" : mode.outcome(source: source,
                                                              destination: destination),
                    .convert, crossed.conversion)
        }
        let outcome = mode.outcome(source: source, destination: destination)
        return (clip, false, false, outcome == "converted", outcome, mode,
                ColorConversion.layers(clip, into: doc, mode: mode))
    }
}
