import AppKit

/// A Layer Style pane's gradient stops: the shared stop editor
/// (`GradientStopsPane`, compact) across the pane's grid, editing EVERY stop of
/// the effect's gradient — add, move, remove, recolour, fade, the midpoints and
/// a library preset — for the Gradient Overlay, the Stroke's gradient fill and
/// the two glows' gradient fill.
///
/// Only the stops cross: an edit here writes `setStops`, and Reverse, Style,
/// Angle, Scale, Dither and Align with Layer stay the pane's own rows. A preset
/// also hands its Smoothness to `smoothness` when the effect keeps one (the
/// Gradient Overlay and the Stroke); a glow keeps none (the core resets every
/// glow-gradient key but the stops), so there a preset sets the stops alone.
final class PaneGradientStopsRow: NSObject, PaneRow {
    private let pane: GradientStopsPane
    private let get: () -> GradientFill

    var views: [NSView] { [pane] }
    var span: PaneRowSpan { .all }

    /// The editor, for a probe.
    var stopsPane: GradientStopsPane { pane }

    /// `get` is the effect's gradient (its stops are what the editor shows);
    /// `smoothness` reads and writes the effect's Smoothness as a fraction, nil
    /// for an effect that keeps none.
    init(
        get: @escaping () -> GradientFill,
        setStops: @escaping ([GradientStop]) -> Void,
        showsMidpoints: Bool,
        smoothness: (get: () -> Double, set: (Double) -> Void)?
    ) {
        self.get = get
        // The grid's own width (label, slider and readout columns), so the
        // row does not widen the pane.
        let width = PaneRows.labelColumnWidth + PaneRows.sliderWidth + PaneRows.readoutWidth + 24
        pane = GradientStopsPane(
            fill: Self.shown(get()), width: width, compact: true, showsMidpoints: showsMidpoints)
        super.init()
        pane.onChange = { fill in setStops(fill.stops) }
        if let smoothness {
            pane.onPresetSmoothness = { value in smoothness.set(value) }
            pane.currentSmoothness = { smoothness.get() }
        }
    }

    func reload() {
        pane.fill = Self.shown(get())
    }

    /// `fill`, or the default two stops when it holds fewer than two (only a
    /// hand-built value can: the core requires two); the first edit writes
    /// the stops shown.
    private static func shown(_ fill: GradientFill) -> GradientFill {
        guard fill.stops.count < GradientEditorView.minStops else { return fill }
        var out = fill
        out.stops = GradientFill().stops
        return out
    }
}

extension PaneRows {
    /// The effect's gradient stops in the shared stop editor (`PaneGradientStopsRow`).
    static func gradientStops(
        get: @escaping () -> GradientFill,
        setStops: @escaping ([GradientStop]) -> Void,
        showsMidpoints: Bool = true,
        smoothness: (get: () -> Double, set: (Double) -> Void)? = nil
    ) -> PaneGradientStopsRow {
        PaneGradientStopsRow(
            get: get, setStops: setStops, showsMidpoints: showsMidpoints, smoothness: smoothness)
    }
}
