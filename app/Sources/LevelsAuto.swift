import AppKit

/// Which Levels Auto: per-channel black and white points (Tone), plus a
/// per-channel gamma that snaps near-neutral midtones (Color) — the math of
/// Image ▸ Auto Tone / Auto Color (`core/src/ops_auto.rs`).
enum LevelsAutoMode: String, CaseIterable {
    case tone, color

    var displayName: String {
        switch self {
        case .tone: return "Auto Tone"
        case .color: return "Auto Color"
        }
    }
}

/// Auto for a Levels ADJUSTMENT LAYER, shared by the dialog's Auto
/// buttons (`LevelsAdjustmentEditor`) and MCP's `auto` argument
/// (`AgentServer.levelsAutoArguments`). It measures the BACKDROP below the
/// layer, never the composite that already carries the correction, through the
/// scoped-analysis request's `auto` (`core/src/scoped_auto.rs`): the whole
/// visible backdrop (a clip base or an isolated group does not narrow it),
/// weighted only by the selection a new layer takes as its mask or, when
/// editing, by the layer's own mask. The answer replaces all four stages: the
/// composite stage stays identity, r/g/b take the derived points (an identity
/// channel is omitted by `LevelsMeta.params`).
enum LevelsAuto {
    /// 0.1 % at each end — the default in Auto Color Correction
    /// Options, and the menu commands' (`EditorViewController.autoClip`).
    static let defaultClip = 0.001
    static let nothingToMeasure = "Nothing visible below this layer to measure."

    /// The backdrop boundary a sheet plan names: the plan's own `below`, or
    /// the whole stack when a new layer lands on top (`.composite`).
    static func backdrop(_ plan: HistogramSource, layerCount: Int) -> Int? {
        switch plan {
        case .composite: return layerCount
        case .backdrop(let below): return below
        case .layer: return nil
        }
    }

    /// One stride-1 backdrop histogram with `auto`; nil when `empty`.
    /// `insert` is set for create (anchor + position) and nil for edit.
    /// Synchronous: the caller waits (`ScopedAnalysisRead.run`).
    static func derive(source: RasterDocument, below: Int,
                       insert: (anchor: Int, position: LayerInsertPosition)?,
                       selection: CanvasSelection?, coverage: [UInt8]?,
                       mode: LevelsAutoMode, clip: Double = defaultClip) throws -> [LevelsChannel: LevelsStage]? {
        var auto: [String: Any] = ["mode": mode.rawValue, "clip": clip]
        if let insert {
            auto["insert"] = ["anchor": insert.anchor, "position": insert.position.rawValue]
        }
        let read = try ScopedAnalysisRead(source: source, scope: .backdrop(below: below),
            selection: selection, coverage: coverage, auto: auto)
        guard let answer = try read.run()["auto"] as? [String: Any] else {
            throw DoctoredCoreError(message: "Auto returned no levels.")
        }
        if answer["empty"] as? Bool == true { return nil }
        guard let black = answer["black"] as? [NSNumber], black.count == 3,
              let white = answer["white"] as? [NSNumber], white.count == 3,
              let gamma = answer["gamma"] as? [NSNumber], gamma.count == 3 else {
            throw DoctoredCoreError(message: "Auto returned malformed levels.")
        }
        var stages: [LevelsChannel: LevelsStage] = [.rgb: .identity]
        for (i, channel) in [LevelsChannel.r, .g, .b].enumerated() {
            var stage = LevelsStage()
            stage.black = black[i].doubleValue
            stage.white = white[i].doubleValue
            stage.gamma = gamma[i].doubleValue
            stages[channel] = stage
        }
        return stages
    }
}
