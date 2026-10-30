import Foundation

/// The Liquify workspace's live field: a plane's displacement and
/// freeze with the strokes applied so far (`dr_liquify_field_*`, `liquify.rs`).
/// Immutable, as every core handle is: `applying` returns a new field. The
/// workspace previews through it; the committed edit runs the same strokes
/// through `RasterImage.filterOp("liquify")` or the 16-bit selected edit,
/// which build the same field.
final class RasterLiquifyField {
    /// The plane the field belongs to, in its own pixels.
    let width: Int
    let height: Int
    private let ptr: OpaquePointer

    /// The identity field of a `width` × `height` plane. Throws the core's
    /// refusal for an empty plane or one past the pixel limit.
    init(width: Int, height: Int) throws {
        guard width > 0, height > 0, width <= Int(UInt32.max), height <= Int(UInt32.max) else {
            throw DoctoredCoreError(message: "Liquify needs a layer with pixels.")
        }
        var error: UnsafeMutablePointer<CChar>?
        guard let field = dr_liquify_field_create(UInt32(width), UInt32(height), &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not start Liquify."))
        }
        self.width = width
        self.height = height
        ptr = field
    }

    private init(owning pointer: OpaquePointer, width: Int, height: Int) {
        ptr = pointer
        self.width = width
        self.height = height
    }

    deinit { dr_liquify_field_free(ptr) }

    /// A new field with `kernelParameters` (`LiquifyStroke.kernelParameters`:
    /// `{"strokes": […]}` in plane pixels) applied after this one's own. The
    /// field carries its summed work, so the core's 1-billion refusal arrives
    /// at the stroke a whole-list replay would refuse at; this field is
    /// unchanged by a refusal.
    func applying(_ kernelParameters: [String: Any]) throws -> RasterLiquifyField {
        guard JSONSerialization.isValidJSONObject(kernelParameters),
              let data = try? JSONSerialization.data(withJSONObject: kernelParameters, options: [.sortedKeys]),
              let json = String(data: data, encoding: .utf8)
        else { throw DoctoredCoreError(message: "Liquify strokes must be finite JSON values.") }
        var error: UnsafeMutablePointer<CChar>?
        guard let field = dr_liquify_field_apply(ptr, json, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not apply the liquify stroke."))
        }
        return RasterLiquifyField(owning: field, width: width, height: height)
    }

    /// `image`, a picture of the plane at any size (the layer or a reduced
    /// proxy), through the field. With no `previous`: all of it. Otherwise
    /// `previous` (the same size as `image`) with `rect` re-rendered, which
    /// must be non-empty and inside the image; a nil `rect` is the whole image.
    func rendering(_ image: RasterImage, over previous: RasterImage? = nil,
                   rect: (x: Int, y: Int, width: Int, height: Int)? = nil) throws -> RasterImage {
        let area = rect ?? (x: 0, y: 0, width: image.width, height: image.height)
        guard area.x >= 0, area.y >= 0, area.width >= 0, area.height >= 0,
              area.x <= Int(UInt32.max), area.y <= Int(UInt32.max),
              area.width <= Int(UInt32.max), area.height <= Int(UInt32.max) else {
            throw DoctoredCoreError(message: "The liquify preview rectangle is outside the image.")
        }
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_liquify_field_render(ptr, image.ptr, previous?.ptr, UInt32(area.x), UInt32(area.y),
                                                   UInt32(area.width), UInt32(area.height), &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not render the liquify preview."))
        }
        return RasterImage(owning: result)
    }

    /// The field's freeze as coverage 0…255 at the pixel centres of a
    /// `width` × `height` picture of the plane, row 0 = top. nil for an empty
    /// picture.
    func freeze(width: Int, height: Int) -> [UInt8]? {
        guard width > 0, height > 0, width <= Int(UInt32.max), height <= Int(UInt32.max),
              width <= RasterImage.maxResizePixels / height else { return nil }
        var output = [UInt8](repeating: 0, count: width * height)
        let filled = output.withUnsafeMutableBufferPointer { buffer in
            dr_liquify_field_freeze(ptr, UInt32(width), UInt32(height), buffer.baseAddress, buffer.count)
        }
        return filled ? output : nil
    }
}
