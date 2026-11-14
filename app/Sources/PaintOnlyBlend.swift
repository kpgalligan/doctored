import AppKit

/// Behind and Clear: the two paint-only blend modes (`DR_BLEND_BEHIND`,
/// `DR_BLEND_CLEAR`), whose rule is the core's (`core/src/paint_mode.rs`,
/// `designs/phase-11f-hdr-float-status.md`): Behind paints under the layer's
/// pixels, Clear takes the paint's alpha off the layer's. They act on a
/// layer's transparency, so no layer carries one as its own mode: they are
/// offered by the Brush (and the Pencil), Edit ▸ Fill… and Edit ▸ Stroke… only,
/// on a layer's pixels, in RGB, Grayscale, Duotone and Lab at every depth.
///
/// `refusal` is the one gate the UI and MCP ask before the core: a mask, a
/// colour plane, an alpha channel or the Quick Mask has no transparency of
/// its own; Preserve Transparency and a transparency lock keep the alpha
/// either mode would change; CMYK, Indexed and Bitmap documents do not offer
/// them.
enum PaintOnlyBlend {
    /// Where the two modes apply, for the sentence every other mode argument
    /// adds to its unknown-mode refusal.
    static let toolsClause = "Behind and Clear apply only to brush_stroke, fill_selection and "
        + "stroke_selection on a layer's pixels."

    /// Whether `doc`'s colour mode offers Behind and Clear: not CMYK, Indexed
    /// or Bitmap.
    static func offered(in doc: RasterDocument?) -> Bool {
        guard let doc else { return true }
        return !doc.isCmyk && !doc.isIndexed && !doc.isBitmap
    }

    /// The refusal of paint-only `mode` on layer `layer` of `doc` (nil for any
    /// other mode, or where it applies): `coverage` is a mask, colour plane,
    /// alpha channel or Quick Mask target; `preserve` is Preserve
    /// Transparency.
    static func refusal(
        _ mode: DrBlendMode, doc: RasterDocument?, layer: Int, coverage: Bool, preserve: Bool
    ) -> String? {
        guard DrBlendMode.isPaintOnly(mode) else { return nil }
        let name = DrBlendMode.displayName(for: mode)
        if let doc, !offered(in: doc) {
            let place = doc.isCmyk ? "a CMYK document"
                : doc.isIndexed ? "an Indexed Color document" : "a Bitmap document"
            return "The \(name) blend mode is unavailable in \(place)."
        }
        if coverage {
            return "The \(name) blend mode paints a layer's transparency; a mask, colour plane, "
                + "alpha channel or the Quick Mask has none of its own. Target the layer, or "
                + "choose another mode."
        }
        if preserve {
            return "The \(name) blend mode changes a layer's transparency, which Preserve "
                + "Transparency keeps. Turn it off, or choose another mode."
        }
        if let doc, doc.transparencyLockOwner(layer) != nil {
            return "The \(name) blend mode changes a layer's transparency, which the layer's "
                + "transparency lock keeps. Unlock it, or choose another mode."
        }
        return nil
    }
}
