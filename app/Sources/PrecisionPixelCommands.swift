import Foundation
import CoreGraphics

/// `linear`: linear light in the document's primaries, used as is; a 32-bit float
/// document's Fill and Stroke ink only (the core's `float_paint::admit_ink`).
enum PrecisionColorSpace: String { case document, srgb, linear }
enum PrecisionGeometryTarget: String { case pixels, mask }
enum PrecisionGeometryFilter: String { case nearest, bilinear, bicubic, lanczos3 }
enum PrecisionPlaneTarget: String { case mask, effectiveMask = "effective_mask", channel, red, green, blue, alpha }

/// Continuous straight RGBA. These are normalized numbers, never byte color
/// values. A command supplies the document/sRGB interpretation separately.
struct PrecisionInk {
    let rgba: [Double]
    init(red: Double, green: Double, blue: Double, alpha: Double = 1) throws {
        let values = [red, green, blue, alpha]
        guard values.allSatisfy({ $0.isFinite && (0...1).contains($0) }) else {
            throw DoctoredCoreError(message: "High-depth ink requires finite normalized RGBA components.")
        }
        rgba = values
    }
}

extension PrecisionPixelCommand {
    static func paint(layer: Int, ink: PrecisionInk, space: PrecisionColorSpace,
                      opacity: Double = 1) throws -> Self {
        try Self(["op": "paint", "layer": layer, "color": ink.rgba,
                  "color_space": space.rawValue, "opacity": opacity])
    }

    /// Fill is the same continuous paint operation through supplied coverage.
    static func fill(layer: Int, ink: PrecisionInk, space: PrecisionColorSpace,
                     opacity: Double = 1) throws -> Self {
        try paint(layer: layer, ink: ink, space: space, opacity: opacity)
    }

    static func erase(layer: Int, opacity: Double = 1) throws -> Self {
        try Self(["op": "erase", "layer": layer, "opacity": opacity])
    }

    static func clone(layer: Int, sourceLayer: Int, dx: Int, dy: Int,
                      opacity: Double = 1) throws -> Self {
        try Self(["op": "clone", "layer": layer, "source_layer": sourceLayer,
                  "dx": dx, "dy": dy, "opacity": opacity])
    }

    /// Coverage outside this explicit solve rectangle has no effect.
    static func heal(layer: Int, sourceLayer: Int, dx: Int, dy: Int,
                     x: Int, y: Int, width: Int, height: Int, opacity: Double = 1) throws -> Self {
        try Self(["op": "heal", "layer": layer, "source_layer": sourceLayer,
                  "dx": dx, "dy": dy, "x": x, "y": y, "width": width,
                  "height": height, "opacity": opacity])
    }

    static func rasterize(name: String, width: Int, height: Int, x: Int, y: Int,
                          colors: [PrecisionInk], space: PrecisionColorSpace) throws -> Self {
        try Self(["op": "rasterize", "name": name, "width": width, "height": height,
                  "x": x, "y": y, "colors": colors.map(\.rgba), "color_space": space.rawValue])
    }

    /// `interactive` (a ⌘T preview) keeps the core's interactive
    /// pixel-work limit; a commit omits it and has no work limit under its
    /// job control.
    static func affine(layer: Int, target: PrecisionGeometryTarget, matrix: CGAffineTransform,
                       filter: PrecisionGeometryFilter = .bilinear,
                       interactive: Bool = false) throws -> Self {
        var fields: [String: Any] = ["op": "affine", "layer": layer, "target": target.rawValue,
                                     "matrix": [matrix.a, matrix.b, matrix.c, matrix.d,
                                                matrix.tx, matrix.ty].map(Double.init),
                                     "filter": filter.rawValue]
        if interactive { fields["interactive"] = true }
        return try Self(fields)
    }

    static func perspective(layer: Int, target: PrecisionGeometryTarget, quad: [CGPoint],
                            filter: PrecisionGeometryFilter = .bilinear,
                            interactive: Bool = false) throws -> Self {
        guard quad.count == 4 else {
            throw DoctoredCoreError(message: "Perspective requires four TL/TR/BR/BL destination corners.")
        }
        var fields: [String: Any] = ["op": "perspective", "layer": layer, "target": target.rawValue,
                                     "quad": quad.flatMap { [Double($0.x), Double($0.y)] },
                                     "filter": filter.rawValue]
        if interactive { fields["interactive"] = true }
        return try Self(fields)
    }

    static func resize(width: Int, height: Int, filter: PrecisionGeometryFilter = .lanczos3) throws -> Self {
        try Self(["op": "resize", "width": width, "height": height, "filter": filter.rawValue])
    }

    static func crop(x: Int, y: Int, width: Int, height: Int) throws -> Self {
        try Self(["op": "crop", "x": x, "y": y, "width": width, "height": height])
    }

    /// Straighten (Phase 12F-1b2): ONE core op equal to the byte composition
    /// `straighten_layers(matrix)` + `transform_channels(matrix)` +
    /// `crop(x, y, width, height)`. `matrix` turns the picture in CANVAS
    /// coordinates — the identical `CGAffineTransform` the byte chain takes —
    /// and the window becomes the new canvas. `interactive` (the Crop tool's
    /// preview) keeps the core's interactive pixel-work limit; the
    /// commit omits it and has no work limit under its job control.
    /// `deleteCropped` (the
    /// Crop tool's Delete Cropped Pixels) clips each plain raster to the window.
    /// The window may lie past the canvas (a signed origin within
    /// `Int32`, `Int32.min` excluded), and `background` (`CropBackground.fields`)
    /// is the Background (Default) fill of such a window.
    static func straighten(matrix: CGAffineTransform, filter: PrecisionGeometryFilter,
                           x: Int, y: Int, width: Int, height: Int,
                           interactive: Bool = false, deleteCropped: Bool = false,
                           background: [String: Any]? = nil) throws -> Self {
        let range = (Int(Int32.min) + 1)...Int(Int32.max)
        guard range.contains(x), range.contains(y), width > 0, height > 0 else {
            throw DoctoredCoreError(message: "A straighten window needs an origin within the signed "
                + "32-bit range and a positive size.")
        }
        var fields: [String: Any] = ["op": "straighten",
                                     "matrix": [matrix.a, matrix.b, matrix.c, matrix.d,
                                                matrix.tx, matrix.ty].map(Double.init),
                                     "filter": filter.rawValue,
                                     "x": x, "y": y, "width": width, "height": height]
        if interactive { fields["interactive"] = true }
        if deleteCropped { fields["delete_cropped"] = true }
        if let background { fields["background"] = background }
        return try Self(fields)
    }

    /// Canvas Size (Phase 12F-1): `(x, y)` is where the OLD canvas's top-left
    /// corner lands in the new one — the byte `dr_doc_canvas_resize` origin.
    /// The core reads no sample: every raster and mask keeps its allocation
    /// and moves its offset, every channel is padded with 0.
    static func canvasSize(width: Int, height: Int, x: Int, y: Int) throws -> Self {
        try Self(["op": "canvas_size", "width": width, "height": height, "x": x, "y": y])
    }

    static func rotateCanvas(degrees: Double, filter: PrecisionGeometryFilter) throws -> Self {
        try Self(["op": "rotate_canvas", "degrees": degrees, "filter": filter.rawValue])
    }

    static func setMask(layer: Int, width: Int, height: Int, x: Int = 0, y: Int = 0,
                        linked: Bool = true, density: Double = 1, feather: Double = 0,
                        enabled: Bool = true, outsideCoverage: Int = 0) throws -> Self {
        var request: [String: Any] = [
            "op": "set_mask", "layer": layer, "width": width, "height": height,
            "x": x, "y": y, "linked": linked, "density": density,
            "feather": feather, "enabled": enabled]
        // Omitted means the core's default 0, so existing requests are unchanged.
        if outsideCoverage != 0 { request["outside_coverage"] = outsideCoverage }
        return try Self(request)
    }

    static func maskProperties(layer: Int, x: Int, y: Int, linked: Bool,
                               density: Double, feather: Double, enabled: Bool) throws -> Self {
        try Self(["op": "mask_properties", "layer": layer, "x": x, "y": y,
                  "linked": linked, "density": density, "feather": feather, "enabled": enabled])
    }

    static func removeMask(layer: Int) throws -> Self { try Self(["op": "remove_mask", "layer": layer]) }

    static func setChannel(index: Int? = nil, name: String) throws -> Self {
        try Self(["op": "set_channel", "index": index.map { $0 as Any } ?? NSNull(), "name": name])
    }

    static func removeChannel(index: Int) throws -> Self { try Self(["op": "remove_channel", "index": index]) }
}
