import AppKit
import QuartzCore

// Develop's HDR preview (#4): on a screen with headroom (`EDRDisplay.potentialHeadroom`,
// which a test instance overrides with `DR_EDR_HEADROOM`), a 32-bit float (linear) development
// shows values above white at Fit and at 100 % detail, Clipping on or off. Each side draws the
// core's rule-X binary16 image of its region (`RasterPrecisionImage.developPreviewExtended`, the
// cells the SDR preview samples), with Clipping's markers at or below 0.0 and at or above white
// (1.0) over every sample of a cell, in its source profile's extended-linearized space on an
// EDR-configured layer (`EDRDisplay.configure`), with the window in extended Display P3 while it
// shows. Without headroom every view is the SDR-clipped u16 one, as before. The histogram reads
// the f32 master's float samples either way (`developHistogram`). Pure view state: no undo, no
// dirty flag, no Action step.

/// The extended-range image of one f32 development's region: RGBA binary16
/// little-endian premultiplied pixels, linear in the source's primaries.
struct RawDevelopExtendedImage {
    let pixels: Data
    let width: Int
    let height: Int
    let divisor: Int
    let peak: Float
    let marked: UInt64

    /// nil for a u16 image; else `region` at its divisor with `clipping`'s
    /// markers (0…3). Consumes `control`, which is single-use.
    static func make(_ image: RasterPrecisionImage,
                     region r: (x: Int, y: Int, width: Int, height: Int, divisor: Int),
                     clipping: UInt32, control: RasterJobControl? = nil) throws -> RawDevelopExtendedImage? {
        guard image.isFloatStorage else { return nil }
        let (pixels, peak, marked) = try image.developPreviewExtended(x: r.x, y: r.y, width: r.width,
            height: r.height, divisor: r.divisor, clipping: clipping, control: control)
        return RawDevelopExtendedImage(pixels: pixels, width: (r.width + r.divisor - 1) / r.divisor,
            height: (r.height + r.divisor - 1) / r.divisor, divisor: r.divisor, peak: peak,
            marked: marked)
    }

    /// The image as a 16-bit float CGImage in `profileData`'s
    /// extended-linearized space; nil when the profile has no such space.
    func image(profileData: Data) -> CGImage? {
        guard let space = CGColorSpace(iccData: profileData as CFData) else { return nil }
        return EDRDisplay.image(halfFloat: pixels, width: width, height: height, documentSpace: space)
    }
}

/// The Develop canvas's EDR state: whether its layer is EDR-configured, and
/// the window colour space it replaced (`.some(nil)` for a window that had
/// none), restored on the way back.
struct RawDevelopExtendedRangeState {
    var engaged = false
    var priorWindowColorSpace: NSColorSpace?? = nil
}

extension RawDevelopCanvasView {
    /// Engages or releases the EDR layer and the window's extended Display
    /// P3 space. Engaged only while an extended image is displayed
    /// (`RawDevelopPreviewFrame.extendedPeak` non-nil).
    /// Not engaged without a window, so a later call engages once the
    /// canvas has one.
    func applyExtendedRange(_ on: Bool) {
        guard on != extendedRange.engaged, !on || window != nil else { return }
        extendedRange.engaged = on
        if let layer { EDRDisplay.configure(layer, extended: on) }
        if on {
            if extendedRange.priorWindowColorSpace == nil, let window,
               let extended = CGColorSpace(name: CGColorSpace.extendedDisplayP3) {
                extendedRange.priorWindowColorSpace = .some(window.colorSpace)
                window.colorSpace = NSColorSpace(cgColorSpace: extended)
            }
        } else if let prior = extendedRange.priorWindowColorSpace {
            window?.colorSpace = prior
            extendedRange.priorWindowColorSpace = nil
        }
        needsDisplay = true
    }
}
