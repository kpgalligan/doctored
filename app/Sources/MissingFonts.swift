import AppKit

// Replace Missing Fonts: Layer ▸ Replace Missing Fonts… (ReplaceFontsSheetController.swift) and
// MCP `list_missing_fonts` / `replace_missing_fonts` (AgentServer+MissingFonts.swift) both run
// the two ops below. The decisions are in designs/replace-missing-fonts.md.
//
// MISSING. A text layer's family is missing when `TextStyle.isInstalled` says so: the family
// is not in NSFontManager's `availableFontFamilies`. Every render path refuses such a layer
// (`NativeArtwork.drawing`, the geometry and profile re-renders), and Free Transform rasterizes
// it, so its saved pixels stay what it shows until its family is replaced.
//
// REPLACING. A description holds one family for the whole layer (`TextLayerPayload.font`), so
// a replacement rewrites that one string and keeps every other field — string, size, colours
// and ink, runs, typography, box, map and fraction — and re-renders the layer through
// `nativeArtwork`, mask and properties kept, in every document kind that holds text (RGB 8, 16
// and 32 bits, Grayscale and Duotone, Lab, CMYK, whose stored inks are kept exactly).
//
// PLACEMENT. The layer's anchor is recovered from its raster (`describedAnchor`), but the
// recovery measures the layout, and a missing family measures with the fallback face. Per axis:
// - where the raster origin does not depend on the face, the anchor is kept exactly: x when the
//   map sends source x to canvas x alone with a positive scale (CG `c == 0`, `a > 0`); y the
//   same way (`b == 0`, `d > 0`) at the natural line height, since a fixed leading puts the
//   first line's ascent, a face metric, above the block;
// - on any other axis the anchor moves by a WHOLE number of pixels so the new raster's centre
//   lands on the saved raster's centre. A whole-pixel move keeps `origin_frac`, so replacing a
//   family with the one the layer was made in reproduces the saved raster exactly, at any map
//   and leading, where the fallback-face estimate alone would move a rotated block by the
//   metric difference between the two faces.
//
// ALL OR NOTHING. Every layer re-renders on one handle; one layer that cannot fails the whole
// replacement with a sentence naming it (`MissingFontFailure`), so the one undo step is always
// the whole replacement.

/// A text layer whose family is not installed here.
struct MissingFontLayer: Equatable {
    let layer: Int
    let name: String
    let family: String
}

/// Why a replacement stopped: the layer that could not re-render and the sentence to show.
struct MissingFontFailure: LocalizedError {
    let layer: Int
    let message: String
    var errorDescription: String? { message }
}

extension TextStyle {
    /// The families `isInstalled` reads, as a set read once, for a caller asking about many
    /// layers or names.
    static func installedFamilies() -> Set<String> {
        Set(NSFontManager.shared.availableFontFamilies)
    }
}

extension RasterDocument {
    /// Every text layer whose family is not installed here, bottom first.
    func missingFontLayers() -> [MissingFontLayer] {
        let installed = TextStyle.installedFamilies()
        return (0..<layerCount).compactMap { index in
            guard let meta = layerMeta(index), let payload = TextLayerPayload.decode(meta),
                  !installed.contains(payload.font) else { return nil }
            return MissingFontLayer(layer: index, name: layerInfo(index)?.name ?? "Layer \(index)",
                                    family: payload.font)
        }
    }

    /// Each missing family that is a key of `replacements` replaced by its value on every text
    /// layer that names it, re-rendered in index order on one handle (module comment). nil when
    /// no layer names a key. Throws `MissingFontFailure` naming the first layer that cannot
    /// re-render; nothing is replaced then. The values must be installed families.
    func replacingMissingFonts(_ replacements: [String: String]) throws -> RasterDocument? {
        let targets = missingFontLayers().filter { replacements[$0.family] != nil }
        guard !targets.isEmpty else { return nil }
        var result = self
        for target in targets {
            guard let family = replacements[target.family] else { continue }
            let failure = { (reason: String) in
                MissingFontFailure(
                    layer: target.layer,
                    message: "“\(target.name)” could not be re-rendered in \(family): " + reason
                        + " Nothing was replaced.")
            }
            guard let info = layerInfo(target.layer),
                  case var .text(payload)? = layerDescription(target.layer)?
                    .resolved(rasterWidth: info.width)
            else { throw failure("its text description could not be read.") }
            payload.font = family
            guard let anchor = missingFontAnchor(target.layer, replacement: payload) else {
                throw failure("its position could not be recovered from its pixels.")
            }
            let rendered: RasterDocument?
            do {
                rendered = try result.nativeArtwork(.text(payload), anchor: anchor,
                                                    layer: target.layer, replacing: true,
                                                    name: info.name)
            } catch {
                throw failure(error.localizedDescription)
            }
            guard let rendered else { throw failure("the layer refused the new layout.") }
            result = rendered
        }
        return result
    }

    /// Where layer `index`'s block goes when `replacement` (its description with the new
    /// family, box resolved) re-renders it: the placement rule of the module comment. nil when
    /// the layer has no recoverable anchor or the replacement cannot be laid out.
    func missingFontAnchor(_ index: Int, replacement: TextLayerPayload) -> CGPoint? {
        guard let estimate = describedAnchor(index), let info = layerInfo(index),
              let rect = LayerDescription.text(replacement.withOriginFraction(
                DescribedLayer.anchorFraction(estimate))).rasterRect()
        else { return nil }
        let map = replacement.transform
        var anchor = estimate
        // Moves one axis by whole pixels so the new raster's centre meets the saved one's.
        let recentred = {
            (saved: Int, savedSize: Int, whole: CGFloat, origin: Int, size: Int) -> CGFloat in
            let old = Double(saved) + Double(savedSize) / 2
            let new = Double(Int(whole.rounded(.down)) + origin) + Double(size) / 2
            return CGFloat((old - new).rounded())
        }
        if !(map.c == 0 && map.a > 0) {
            anchor.x += recentred(info.offsetX, info.width, estimate.x, rect.originX, rect.width)
        }
        if !(map.b == 0 && map.d > 0 && replacement.leading == 0) {
            anchor.y += recentred(info.offsetY, info.height, estimate.y, rect.originY, rect.height)
        }
        return anchor
    }
}
