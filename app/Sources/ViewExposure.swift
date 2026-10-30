import AppKit

// A view exposure is per-WINDOW view state, as view rotation is: never in `EditingState`, never
// persisted, never an undo step and never an Action step. It reaches only the canvas, through the
// display-only `canvas.proofImage` slot (`EditorViewController+ViewExposure.swift`); readouts,
// samplers, the clipping overlay, the Navigator, thumbnails, export and save never see it.

/// How one view looks at a 32-bit float projection: a tone map other
/// than the document's own, and an exposure in stops. The core's
/// `float_view::FloatView`, which `DoctoredCore+Float.swift` hands it to.
struct FloatDisplayView: Equatable {
    /// nil = the document's own map.
    var toneMap: ToneMap?
    /// `gain = 2^stops` on RGB before the map; alpha is untouched.
    var exposureStops: Double
    /// The EDR view (v1.x): linear light above white kept for an
    /// extended-range canvas, no tone map (the core's rule X). Set by the
    /// editor's engage rule (`EditorViewController+HDRPreview.swift`).
    var extendedRange = false

    /// The document's own map at 0 stops: its ordinary display.
    static let document = FloatDisplayView(toneMap: nil, exposureStops: 0)
}

/// One window's view exposure. The stored stops survive a depth change but
/// act only on a 32-bit float document.
struct ViewExposureState {
    /// The exposure in stops, −20…+20.
    var stops: Double = 0
    /// The Tone Map sheet's pending choice while it is open; nil = the
    /// document's own map.
    var candidateToneMap: ToneMap?
    /// What the image now in `canvas.proofImage` was rendered for; nil when
    /// the slot is empty.
    var shown: FloatDisplayView?
    /// View ▸ HDR Preview (v1.x), per window, on by default: a float
    /// document on an EDR screen shows values above white brighter than white.
    var hdrPreview = true
    /// The largest RGB component of the extended-range image now in the slot;
    /// nil when the slot holds none.
    var extendedPeak: Float?
    /// The window's colour space before HDR Preview engaged, restored when it
    /// disengages; nil while not engaged.
    var priorWindowColorSpace: NSColorSpace??

    /// What this window asks the display for.
    var view: FloatDisplayView {
        FloatDisplayView(toneMap: candidateToneMap, exposureStops: stops)
    }
}

/// The view exposure's range and its one spelling.
enum ViewExposure {
    /// The core's `VIEW_EXPOSURE_STOPS_MAX`, either way: 2^±20 brings either
    /// end of the stored range to mid-grey with room to spare.
    static let range: ClosedRange<Double> = -20...20

    /// "+1.50", "−3.00", "0.00": two decimals with an explicit sign.
    static func text(_ stops: Double) -> String {
        stops == 0 ? "0.00" : String(format: "%+.2f", stops)
    }
}
