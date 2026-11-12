import AppKit

/// The Pencil's options bar: the Brush's paint clusters less the tip — Size,
/// Opacity, Blend, Smoothing and the two pressure toggles. Its tip is the one
/// `ToolOptionsStore.pencilTip` pins (hard, round, the aliased rhythm), so the
/// Preset, Hardness, Flow, Spacing, Angle, Roundness and Airbrush controls
/// would only show values the tool cannot use. The stroke itself is the
/// Brush's (`EditorTool.strokeFamily`) with `BrushTip.aliased`.
extension EditorViewController {
    private static let pencilOptionIDs: Set<String> = [
        "paint.size", "paint.opacity", "paint.blend", "paint.blend.gray", "paint.blend.lab",
        "paint.smoothing", "paint.pressure", "paint.pressureOpacity",
    ]

    func pencilClusters() -> [OptionCluster] {
        paintClusters(.pencil).compactMap { cluster in
            let kept = cluster.descriptors.filter { Self.pencilOptionIDs.contains($0.id) }
            return kept.isEmpty ? nil : OptionCluster(kept)
        }
    }
}
