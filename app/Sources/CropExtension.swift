import AppKit

// Crop past the canvas. A box whose ROUNDED window reaches past any canvas edge
// (`CropSession.extends`) is committed as the core's signed straighten window: at angle 0 the
// identity Nearest straighten (`RasterDocument.extendedCrop`, the core's `extended_crop`), turned
// through `straightenedCrop` as before. Either way it carries the Background (Default) fill
// (`CropBackground`): the core pads an opaque D5 Background with the background colour and leaves
// every other layer transparent in the new area, and pads nothing with Delete Cropped Pixels off
// (take1, `crop-extend-665`). A box whose unrounded rect lies inside the canvas always
// rounds inside it (`roundedWindow`), so it never takes this route and stays bit-identical.

/// The core's `"background"` of a crop past the canvas: `color` (document-space RGB, 0…1) on an
/// RGB document, `ink` (`[c, m, y, k]`) on a CMYK one.
struct CropBackground {
    let fields: [String: Any]

    /// The background colour as the document paints it: on RGB `PrecisionInk`'s components in
    /// the document's own space (on Lab, #1 phase 4a, the stored swatch ink's codes:
    /// `LabPaint.color`, else the colour as `srgb` for the core's Mode rule), on CMYK
    /// `CmykPaint.ink` (the stored swatch ink, else the colour through the profile), its first
    /// four.
    static func make(_ color: NSColor, cmykInk: CmykInk?, labInk: LabInk? = nil,
                     in doc: RasterDocument) throws -> CropBackground
    {
        if doc.isCmyk, let precision = doc.precision {
            let ink = try CmykPaint.ink(color, stored: cmykInk, in: precision)
            return CropBackground(fields: ["ink": Array(ink.prefix(4))])
        }
        // A Lab document with no stored ink takes the colour as sRGB, converted in the core by
        // Mode ▸ Lab Color's rule (`prepare_colors`), as the Brush's `srgb` ink is.
        if doc.isLab, labInk == nil {
            let ink = try PrecisionInk(color: color)
            return CropBackground(fields: ["color": Array(ink.rgba.prefix(3)),
                                           "color_space": PrecisionColorSpace.srgb.rawValue])
        }
        let color = LabPaint.color(color, stored: labInk, in: doc)
        let ink = try PrecisionInk(color: color, space: doc.nsColorSpace)
        return CropBackground(fields: ["color": Array(ink.rgba.prefix(3)),
                                       "color_space": PrecisionColorSpace.document.rawValue])
    }
}

extension CropSession {
    /// Whether a ROUNDED window reaches past any edge of the canvas.
    static func extends(_ window: CGRect, canvas: CGSize) -> Bool {
        window.minX < 0 || window.minY < 0
            || window.maxX > canvas.width || window.maxY > canvas.height
    }

    /// `rect` at least 1×1 and at most `maxResizePixels`, scaled down UNIFORMLY about its origin
    /// when it is larger, so a ratio the box was built to survives. Never pinned to the canvas:
    /// the box may reach past it.
    static func bounded(_ rect: CGRect, ratio: Double?) -> CGRect {
        var rect = rect.standardized
        rect.size.width = max(rect.width, 1)
        rect.size.height = max(rect.height, 1)
        let limit = CGFloat(RasterImage.maxResizePixels)
        let area = rect.width * rect.height
        if area > limit {
            let scale = (limit / area).squareRoot()
            rect.size.width = max((rect.width * scale).rounded(.down), 1)
            rect.size.height = max((rect.height * scale).rounded(.down), 1)
            if let ratio, ratio > 0 {
                rect.size.height = max((rect.width / CGFloat(ratio)).rounded(.down), 1)
            }
        }
        return rect
    }
}

extension RasterDocument {
    /// Why a `width × height` window past the canvas cannot become the canvas: Canvas Size's
    /// 100 MP sentence, then the channel budget's; nil when it can.
    func extendedCropRefusal(width: Int, height: Int) -> String? {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            return Self.canvasTooLargeRefusal
        }
        return channelBudgetRefusal(width: width, height: height)
    }

    /// A crop at angle 0 to a window past the canvas: the identity Nearest straighten over it
    /// (`editingGeometry`), then at 8 bits the Delete Cropped Pixels host trim, then `cropped`'s
    /// artwork-description restore at depth, then the fill layers refit. nil is unchanged.
    func extendedCrop(x: Int, y: Int, w: Int, h: Int, deleteCropped: Bool,
                      background: CropBackground?) throws -> RasterDocument? {
        let descriptions = isHighDepth ? try artworkDescriptionsBeforeGeometry("Crop") : []
        let command = try PrecisionPixelCommand.straighten(
            matrix: .identity, filter: .nearest, x: x, y: y, width: w, height: h,
            deleteCropped: deleteCropped, background: background?.fields)
        guard var result = try editingGeometry(request: command.request) else { return nil }
        if deleteCropped, !isHighDepth { result = result.deletingCroppedPixels() ?? result }
        for (index, description) in descriptions {
            result = try result.restoringArtworkDescription(description, layer: index)
        }
        return try result.refittingFillLayers()
    }
}
