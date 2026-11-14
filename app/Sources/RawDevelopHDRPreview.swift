import AppKit
import QuartzCore

// Develop's HDR preview (#4): on a screen with headroom (`EDRDisplay.potentialHeadroom`,
// which a test instance overrides with `DR_EDR_HEADROOM`), a 32-bit float (linear) development
// shows values above white at Fit and at 100 % detail, Clipping on or off. Each side draws the
// core's rule-X binary16 image of its region (`RasterPrecisionImage.developPreviewExtended`, the
// cells the SDR preview samples), with Clipping's markers at or below 0.0 and at or above white
// (1.0) over every sample of a cell, in its source profile's extended-linearized space on an
// EDR-configured layer (`EDRDisplay.configure`), with the window in extended Display P3 while it
// shows. Without headroom every view is the SDR-clipped u16 one unless Visualize HDR (below) is
// on. The histogram reads
// the f32 master's float samples either way (`developHistogram`). Pure view state: no undo, no
// dirty flag, no Action step.
//
// Visualize HDR (the preview's checkbox under the Clipping row, MCP `inspect_raw_development`
// `visualize_hdr`) marks every cell any of whose samples has an RGB channel strictly above 1.0 in
// one flat opaque colour, `DS.hdrMarker`, converted into the source profile's extended-linearized
// space; exactly 1.0 is white, not above it. It wins over a Clipping marker in its cells, so red
// then means "at white, nothing above". It works on any screen: with it on, an f32 side draws its
// extended image even without headroom, and since every value above white is under the marker
// that image holds nothing above SDR white, so the canvas engages EDR only on a screen with
// headroom, as before. A u16 side holds nothing above white and is never marked.

/// The extended-range image of one f32 development's region: RGBA binary16
/// little-endian premultiplied pixels, linear in the source's primaries.
struct RawDevelopExtendedImage {
    let pixels: Data
    let width: Int
    let height: Int
    let divisor: Int
    let peak: Float
    let marked: UInt64
    /// The output pixels in `DS.hdrMarker`; 0 without Visualize HDR.
    let hdrMarked: UInt64

    /// nil for a u16 image; else `region` at its divisor with `clipping`'s
    /// markers (0…3) and, with `visualizeHDR`, the HDR marker. Throws when
    /// the image's profile has no extended-linearized space for the marker.
    /// Consumes `control`, which is single-use.
    static func make(_ image: RasterPrecisionImage,
                     region r: (x: Int, y: Int, width: Int, height: Int, divisor: Int),
                     clipping: UInt32, visualizeHDR: Bool = false,
                     control: RasterJobControl? = nil) throws -> RawDevelopExtendedImage? {
        guard image.isFloatStorage else { return nil }
        var marker: [Float]?
        if visualizeHDR {
            guard let components = hdrMarker(profileData: image.profileData) else {
                throw DoctoredCoreError(message: "This RAW development's profile has no extended linear space for Visualize HDR.")
            }
            marker = components
        }
        let made = try image.developPreviewExtended(x: r.x, y: r.y, width: r.width,
            height: r.height, divisor: r.divisor, clipping: clipping, hdrMarker: marker,
            control: control)
        return RawDevelopExtendedImage(pixels: made.pixels, width: (r.width + r.divisor - 1) / r.divisor,
            height: (r.height + r.divisor - 1) / r.divisor, divisor: r.divisor, peak: made.peak,
            marked: made.marked, hdrMarked: made.hdrMarked)
    }

    /// `DS.hdrMarker` as linear components in `profileData`'s
    /// extended-linearized space, the space `image(profileData:)` tags the
    /// pixels with, each clamped to 0…1; nil when the profile has no such
    /// space.
    static func hdrMarker(profileData: Data) -> [Float]? {
        guard let space = CGColorSpace(iccData: profileData as CFData),
              let linear = CGColorSpaceCreateExtendedLinearized(space),
              let color = DS.hdrMarker.cgColor.converted(to: linear, intent: .defaultIntent, options: nil),
              let components = color.components, components.count >= 3 else { return nil }
        return components.prefix(3).map { Float(min(1, max(0, $0))) }
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
