import AppKit

// MARK: - Clipping display

extension RasterImage {
    /// The opaque threshold display (`DR_CLIP_DISPLAY_*`): highlights light
    /// each RGB channel at 255 on black, shadows drop each RGB channel at 0
    /// on white; alpha-0 pixels show the ground. nil for a bad mode.
    func clipDisplay(mode: UInt32) -> RasterImage? {
        wrap(dr_image_clip_display(ptr, mode))
    }
}

extension RasterPrecisionImage {
    /// The same display over exact native 0/65535 codes. Float tests linear
    /// values: a channel `>= 1.0` is at the maximum and one `<= 0.0` at zero.
    func clipDisplay(mode: UInt32) throws -> RasterImage {
        let context = try PrecisionWorkContext(image: self)
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_image_clip_display(ptr, mode, context.limit,
            context.otherRetained, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not render the clipping display."))
        }
        return RasterImage(owning: result)
    }
}
