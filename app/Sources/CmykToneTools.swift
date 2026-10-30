import AppKit

// Levels and Curves' tools on a CMYK document (#1 phase 5): the histogram per ink and composite
// (`CmykHistogram`, of L = 1 − ink, read through `dr_precision_doc_cmyk_tone_analysis`), Auto and
// Auto Options (`CmykAutoOptions`, persisted, and `CmykAutoOptionsSheetController`), the black,
// grey and white eyedroppers (a canvas click while the dialog is up), and the turning of the
// core's answers into stage values (`CmykToneTools`): Auto's L endpoints become Levels'
// black/white (the composite when all four pairs are equal — Monochromatic's one pair — else per
// ink, where Per Channel's K pair is (0, 1), holding the black plate) or Curves' two end points on
// the ink axes; an eyedropper's sample and target become Levels' input and output points (grey: the
// gamma that takes the sample's L to the target's) or a Curves point (255·sample, 255·target) per
// ink. `CmykToneControls` is the row both dialogs show.

/// The histogram of one CMYK scope: 256 bins per plate and the composite, of `L = 1 − ink`.
struct CmykHistogram {
    /// `cmyk`, `c`, `m`, `y`, `k` → 256 bins.
    let plates: [String: [Double]]

    init?(_ analysis: [String: Any]) {
        guard let histogram = analysis["histogram"] as? [String: Any] else { return nil }
        var plates: [String: [Double]] = [:]
        for key in ["cmyk", "c", "m", "y", "k"] {
            guard let bins = (histogram[key] as? [NSNumber])?.map(\.doubleValue), bins.count == 256 else {
                return nil
            }
            plates[key] = bins
        }
        self.plates = plates
    }

    /// The plot of `key` as `HistogramView` draws it (its luminosity bars). `inkAxis` reverses it
    /// for Curves, whose x axis is ink (0 % at the left), where L's 255 is ink's 0.
    func bins(for key: String, inkAxis: Bool) -> HistogramBins? {
        guard var plate = plates[key] else { return nil }
        if inkAxis { plate.reverse() }
        let total = plate.reduce(0, +)
        return HistogramBins(values: plate + plate + plate + plate, total: total, sampled: false)
    }
}

/// Auto Options on a CMYK document: Enhance Per Channel Contrast (the default) or
/// Enhance Monochromatic Contrast, and the Shadows and Highlights clip percentages (0.10 % each).
struct CmykAutoOptions: Equatable {
    /// `per_channel` or `monochromatic`.
    var algorithm: String = "per_channel"
    var shadows: Double = 0.1
    var highlights: Double = 0.1

    private static let algorithmKey = "CmykAutoAlgorithm"
    private static let shadowsKey = "CmykAutoShadows"
    private static let highlightsKey = "CmykAutoHighlights"

    /// The options Save as defaults stored, else the factory defaults.
    static var saved: CmykAutoOptions {
        get {
            let defaults = UserDefaults.standard
            var options = CmykAutoOptions()
            if let algorithm = defaults.string(forKey: algorithmKey),
               ["per_channel", "monochromatic"].contains(algorithm) { options.algorithm = algorithm }
            if let shadows = defaults.object(forKey: shadowsKey) as? Double,
               (0..<10).contains(shadows) { options.shadows = shadows }
            if let highlights = defaults.object(forKey: highlightsKey) as? Double,
               (0..<10).contains(highlights) { options.highlights = highlights }
            return options
        }
        set {
            UserDefaults.standard.set(newValue.algorithm, forKey: algorithmKey)
            UserDefaults.standard.set(newValue.shadows, forKey: shadowsKey)
            UserDefaults.standard.set(newValue.highlights, forKey: highlightsKey)
        }
    }

    /// The core's `auto` request object.
    var request: [String: Any] {
        ["algorithm": algorithm, "shadows": shadows, "highlights": highlights]
    }

    /// An MCP `auto` object (`{"algorithm","shadows","highlights"}`, every key optional) over
    /// the saved options.
    init(arguments: Any?) throws {
        self = CmykAutoOptions.saved
        guard let arguments, !(arguments is NSNull) else { return }
        if arguments as? Bool == true { return }
        guard let object = arguments as? [String: Any] else {
            throw DoctoredCoreError(message: "On a CMYK document auto is true or "
                + "{\"algorithm\":\"per_channel\"|\"monochromatic\",\"shadows\",\"highlights\"}.")
        }
        if let algorithm = object["algorithm"] as? String {
            guard ["per_channel", "monochromatic"].contains(algorithm) else {
                throw DoctoredCoreError(message: "auto algorithm must be per_channel or monochromatic.")
            }
            self.algorithm = algorithm
        }
        if let shadows = (object["shadows"] as? NSNumber)?.doubleValue { self.shadows = shadows }
        if let highlights = (object["highlights"] as? NSNumber)?.doubleValue { self.highlights = highlights }
    }

    init() {}
}

/// The three eyedropper kinds.
enum CmykEyedropper: String, CaseIterable {
    case black, gray, white

    var title: String {
        switch self {
        case .black: return "Black Point"
        case .gray: return "Gray Point"
        case .white: return "White Point"
        }
    }
}

/// The core's answers as stage values (file comment).
enum CmykToneTools {
    /// The ink keys in C, M, Y, K order.
    static let inks = ["c", "m", "y", "k"]

    /// Auto's `{"black":[4],"white":[4],"algorithm"}` as Levels stages: the composite when all
    /// four pairs are equal, else one per ink (Per Channel's K pair is (0, 1), the identity).
    /// nil for `{"empty":true}`.
    static func levelsStages(fromAuto auto: [String: Any]) -> [LevelsChannel: LevelsStage]? {
        guard let black = (auto["black"] as? [NSNumber])?.map(\.doubleValue),
              let white = (auto["white"] as? [NSNumber])?.map(\.doubleValue),
              black.count == 4, white.count == 4 else { return nil }
        var stages: [LevelsChannel: LevelsStage] = [:]
        if Set(black).count == 1 && Set(white).count == 1 {
            var stage = LevelsStage()
            stage.black = black[0]
            stage.white = white[0]
            stages[.rgb] = stage
        } else {
            for (index, channel) in [LevelsChannel.c, .m, .y, .k].enumerated() {
                var stage = LevelsStage()
                stage.black = black[index]
                stage.white = white[index]
                stages[channel] = stage
            }
        }
        return stages
    }

    /// Auto as Curves points on the ink axes: L black `lo` and white `hi` are the line from ink
    /// `255(1 − hi)` at 0 to ink `255(1 − lo)` at 255 (flat beyond both). The composite curve
    /// when all four pairs are equal, else one per ink.
    static func curvePoints(fromAuto auto: [String: Any]) -> [CurveChannel: [CurvePoint]]? {
        guard let black = (auto["black"] as? [NSNumber])?.map(\.doubleValue),
              let white = (auto["white"] as? [NSNumber])?.map(\.doubleValue),
              black.count == 4, white.count == 4 else { return nil }
        func line(_ lo: Double, _ hi: Double) -> [CurvePoint] {
            guard hi > lo else { return CurvePoint.identity }
            return [CurvePoint(x: (255 * (1 - hi)).rounded(), y: 0),
                    CurvePoint(x: (255 * (1 - lo)).rounded(), y: 255)]
        }
        if Set(black).count == 1 && Set(white).count == 1 {
            return [.cmyk: line(black[0], white[0])]
        }
        var out: [CurveChannel: [CurvePoint]] = [:]
        for (index, channel) in [CurveChannel.cyan, .magenta, .yellow, .black].enumerated() {
            out[channel] = line(black[index], white[index])
        }
        return out
    }

    /// An eyedropper's sample and target (ink, 0…1) onto the per-ink Levels stages (file
    /// comment): black and white move the input point to the sample's L and the output point to
    /// the target's; grey solves each ink's gamma so the sample's L lands on the target's.
    static func levelsStages(_ kind: CmykEyedropper, sample: [Double], target: [Double],
                             onto stages: [LevelsChannel: LevelsStage]) -> [LevelsChannel: LevelsStage] {
        var out = stages
        for (index, channel) in [LevelsChannel.c, .m, .y, .k].enumerated()
        where sample.indices.contains(index) && target.indices.contains(index) {
            var stage = out[channel] ?? LevelsStage()
            let lSample = 1 - sample[index], lTarget = 1 - target[index]
            switch kind {
            case .black:
                stage.black = min(max(lSample, 0), stage.white - 0.01)
                stage.outputBlack = min(max(lTarget, 0), 1)
            case .white:
                stage.white = max(min(lSample, 1), stage.black + 0.01)
                stage.outputWhite = min(max(lTarget, 0), 1)
            case .gray:
                let u = (lSample - stage.black) / (stage.white - stage.black)
                let t = (lTarget - stage.outputBlack) / (stage.outputWhite - stage.outputBlack)
                // t = u^(1/γ): γ = ln u / ln t, defined only strictly inside (0, 1).
                guard u > 0, u < 1, t > 0, t < 1 else { continue }
                stage.gamma = min(max(log(u) / log(t), 0.1), 9.99)
            }
            out[channel] = stage
        }
        return out
    }

    /// An eyedropper's sample and target as one Curves point per ink: (255·sample, 255·target),
    /// replacing a point within ±2 of it.
    static func curvePoints(sample: [Double], target: [Double],
                            onto points: [CurveChannel: [CurvePoint]]) -> [CurveChannel: [CurvePoint]] {
        var out = points
        for (index, channel) in [CurveChannel.cyan, .magenta, .yellow, .black].enumerated()
        where sample.indices.contains(index) && target.indices.contains(index) {
            let x = (255 * min(max(sample[index], 0), 1)).rounded()
            let y = (255 * min(max(target[index], 0), 1)).rounded()
            var list = (out[channel] ?? CurvePoint.identity).filter { abs($0.x - x) > 2 }
            guard list.count < CurveEditorView.maxPoints else { continue }
            list.append(CurvePoint(x: x, y: y))
            list.sort { $0.x < $1.x }
            out[channel] = list
        }
        return out
    }

    /// Whether `params` are a CMYK Levels or Curves preset's (`cmyk_adjust`'s keys): the five
    /// stage keys and c/m/y/k stage objects, or cmyk/c/m/y/k point lists.
    static func acceptsPreset(_ op: AdjustmentLayerOp, _ params: [String: Any]) -> Bool {
        let stageKeys: Set<String> = ["black", "white", "gamma", "output_black", "output_white"]
        func stage(_ object: [String: Any]) -> Bool {
            Set(object.keys).isSubset(of: stageKeys) && object.values.allSatisfy { $0 is NSNumber }
        }
        switch op {
        case .levels:
            return params.allSatisfy { key, value in
                if stageKeys.contains(key) { return value is NSNumber }
                if inks.contains(key), let object = value as? [String: Any] { return stage(object) }
                return false
            }
        case .curves:
            return params.allSatisfy { key, value in
                guard (["cmyk"] + inks).contains(key), let points = value as? [[NSNumber]] else { return false }
                return points.allSatisfy { $0.count == 2 }
            }
        default:
            return false
        }
    }
}

/// The CMYK row of Levels and Curves: Auto, Options… and the three eyedroppers, plus a status
/// line. The dialog supplies what each answer does to its stages.
final class CmykToneControls: NSObject {
    let row: NSStackView
    private let status = NSTextField(labelWithString: "")
    private weak var document: ImageDocument?
    private weak var canvas: ImageCanvasView?
    /// The analysis scope (`source`, `layer`, `below`) and its selection coverage.
    private let scope: [String: Any]
    private let coverage: [UInt16]
    private var buttons: [CmykEyedropper: NSButton] = [:]
    private var monitor: CanvasPointerMonitor?
    private var options = CmykAutoOptions.saved

    /// Auto's `auto` object.
    var onAuto: (([String: Any]) -> Void)?
    /// An eyedropper's kind, sample and target (ink 0…1).
    var onEyedropper: ((CmykEyedropper, [Double], [Double]) -> Void)?

    init(document: ImageDocument, canvas: ImageCanvasView?, scope: [String: Any], coverage: [UInt16]) {
        self.document = document
        self.canvas = canvas
        self.scope = scope
        self.coverage = coverage
        row = NSStackView()
        super.init()
        let auto = StickerButton(title: "Auto", style: .secondary, target: self, action: #selector(autoClicked(_:)))
        auto.setAccessibilityIdentifier("cmyk-tone-auto")
        let optionsButton = StickerButton(title: "Options…", style: .secondary, target: self,
                                          action: #selector(optionsClicked(_:)))
        optionsButton.setAccessibilityIdentifier("cmyk-tone-auto-options")
        var views: [NSView] = [auto, optionsButton]
        for kind in CmykEyedropper.allCases {
            let button = NSButton(title: kind.title, target: self, action: #selector(eyedropperClicked(_:)))
            button.setButtonType(.pushOnPushOff)
            button.bezelStyle = .rounded
            button.font = DS.sans(11)
            button.toolTip = "Click the image to set the \(kind.title.lowercased())"
            button.setAccessibilityIdentifier("cmyk-tone-eyedropper-\(kind.rawValue)")
            buttons[kind] = button
            views.append(button)
        }
        row.setViews(views, in: .leading)
        row.orientation = .horizontal
        row.alignment = .centerY
        row.spacing = 6
        status.font = DS.sans(11)
        status.textColor = DS.textMuted
        status.setAccessibilityIdentifier("cmyk-tone-status")
    }

    /// The status line, under the row.
    var statusLabel: NSTextField { status }

    /// The histogram of the dialog's scope, or nil with the reason in the status line.
    func loadHistogram() -> CmykHistogram? {
        do {
            return CmykHistogram(try analysis([:]))
        } catch {
            status.stringValue = error.localizedDescription
            return nil
        }
    }

    private func analysis(_ extra: [String: Any]) throws -> [String: Any] {
        guard let typed = document?.doc?.precision, typed.isCmyk else {
            throw DoctoredCoreError(message: "The CMYK document is no longer open.")
        }
        var request = scope
        request.merge(extra) { _, new in new }
        let coverage = self.coverage
        let task = try WorkerTask { () throws -> [String: Any] in
            let context = try PrecisionWorkContext(source: typed)
            return try typed.cmykToneAnalysis(request: request, coverage: coverage, workingLimit: context.limit,
                                              otherRetained: context.otherRetained, control: nil)
        }
        return try BackgroundWork.wait(task, title: "Reading CMYK Tones")
    }

    /// Runs Auto with the current options and hands the result over.
    func runAuto() {
        do {
            let result = try analysis(["auto": options.request])
            guard let auto = result["auto"] as? [String: Any], auto["empty"] == nil else {
                NSSound.beep()
                status.stringValue = "Nothing to measure: the scope holds no visible pixels."
                return
            }
            status.stringValue = ""
            onAuto?(auto)
        } catch is JobCancelled {
            return
        } catch {
            status.stringValue = error.localizedDescription
        }
    }

    @objc private func autoClicked(_ sender: Any?) { runAuto() }

    @objc private func optionsClicked(_ sender: Any?) {
        guard let chosen = CmykAutoOptionsSheetController.run(options) else { return }
        options = chosen
        runAuto()
    }

    @objc private func eyedropperClicked(_ sender: Any?) {
        guard let button = sender as? NSButton,
              let kind = buttons.first(where: { $0.value === button })?.key else { return }
        for (other, otherButton) in buttons where other != kind { otherButton.state = .off }
        monitor?.stop()
        monitor = nil
        guard button.state == .on, let canvas else { return }
        monitor = CanvasPointerMonitor(canvas: canvas) { [weak self] event in
            guard event.phase == .down else { return }
            self?.sample(kind, at: event.point)
        }
    }

    /// One eyedropper click: the composite's ink under `point` (a 3 × 3 average) and the target.
    func sample(_ kind: CmykEyedropper, at point: CGPoint) {
        let x = Int(floor(point.x)), y = Int(floor(point.y))
        do {
            let result = try analysis(["eyedropper": [
                "kind": kind.rawValue, "x": x, "y": y, "reach": 1,
                "intent": ColorSettings.intent.rawValue,
                "black_point_compensation": ColorSettings.blackPointCompensation,
            ]])
            guard let pick = result["eyedropper"] as? [String: Any],
                  let sample = (pick["sample"] as? [NSNumber])?.map(\.doubleValue),
                  let target = (pick["target"] as? [NSNumber])?.map(\.doubleValue) else {
                NSSound.beep()
                return
            }
            status.stringValue = ""
            onEyedropper?(kind, sample, target)
        } catch is JobCancelled {
            return
        } catch {
            NSSound.beep()
            status.stringValue = error.localizedDescription
        }
    }

    /// Stops the eyedropper monitor (the dialog's teardown).
    func stop() {
        monitor?.stop()
        monitor = nil
    }
}

/// Auto Color Correction Options on a CMYK document: the algorithm, the Shadows and Highlights
/// clip percentages and Save as defaults. An app-modal card over the dialog's sheet.
final class CmykAutoOptionsSheetController: NSObject {
    private let monochromatic = NSButton(radioButtonWithTitle: "Enhance Monochromatic Contrast",
                                         target: nil, action: nil)
    private let perChannel = NSButton(radioButtonWithTitle: "Enhance Per Channel Contrast",
                                      target: nil, action: nil)
    private let shadowsField = NSTextField()
    private let highlightsField = NSTextField()
    private let saveDefaults = NSButton(checkboxWithTitle: "Save as defaults", target: nil, action: nil)
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var result: CmykAutoOptions?
    private let initial: CmykAutoOptions

    private init(_ options: CmykAutoOptions) {
        initial = options
        super.init()
    }

    /// The chosen options, or nil when cancelled.
    static func run(_ options: CmykAutoOptions) -> CmykAutoOptions? {
        CmykAutoOptionsSheetController(options).present()
    }

    private func present() -> CmykAutoOptions? {
        for button in [monochromatic, perChannel] {
            button.target = self
            button.action = #selector(radioChanged(_:))
            button.font = DS.sans(13)
        }
        monochromatic.setAccessibilityIdentifier("cmyk-auto-monochromatic")
        perChannel.setAccessibilityIdentifier("cmyk-auto-per-channel")
        (initial.algorithm == "monochromatic" ? monochromatic : perChannel).state = .on
        for (field, value, id) in [(shadowsField, initial.shadows, "cmyk-auto-shadows"),
                                   (highlightsField, initial.highlights, "cmyk-auto-highlights")] {
            DSField.style(field)
            field.stringValue = String(format: "%.2f", value)
            field.widthAnchor.constraint(equalToConstant: 64).isActive = true
            field.setAccessibilityIdentifier(id)
        }
        saveDefaults.font = DS.sans(13)
        saveDefaults.setAccessibilityIdentifier("cmyk-auto-save-defaults")
        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 320
        let grid = NSGridView(views: [
            [fieldLabel("Algorithms:"), perChannel],
            [NSGridCell.emptyContentView, monochromatic],
            [fieldLabel("Shadows clip %:"), shadowsField],
            [fieldLabel("Highlights clip %:"), highlightsField],
            [NSGridCell.emptyContentView, saveDefaults],
        ])
        grid.rowSpacing = 8
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        let content = NSStackView(views: [grid, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let ok = StickerButton(title: "OK", style: .primary, target: self, action: #selector(okClicked(_:)))
        ok.setAccessibilityIdentifier("cmyk-auto-ok")
        modal = ModalCardWindowController(
            title: "Auto Color Correction Options",
            card: makeSheetView(
                title: "Auto Color Correction Options",
                hint: "Auto sets the cyan, magenta and yellow inks' black and white points, black "
                    + "keeping its ink (Per Channel), or one pair from all four inks for all four "
                    + "(Monochromatic), clipping these percentages of the pixels at each end.",
                content: content, buttonRow: makeButtonRow(cancel: cancel, apply: ok)))
        guard modal?.runModal() == .OK else { return nil }
        return result
    }

    @objc private func radioChanged(_ sender: Any?) {
        monochromatic.state = (sender as AnyObject) === monochromatic ? .on : .off
        perChannel.state = (sender as AnyObject) === perChannel ? .on : .off
    }

    @objc private func cancelClicked(_ sender: Any?) { modal?.end(.cancel) }

    @objc private func okClicked(_ sender: Any?) {
        guard let shadows = Double(shadowsField.stringValue), let highlights = Double(highlightsField.stringValue),
              (0..<10).contains(shadows), (0..<10).contains(highlights) else {
            message.stringValue = "Each clip is a percentage from 0 to 9.99."
            NSSound.beep()
            return
        }
        var options = CmykAutoOptions()
        options.algorithm = monochromatic.state == .on ? "monochromatic" : "per_channel"
        options.shadows = shadows
        options.highlights = highlights
        if saveDefaults.state == .on { CmykAutoOptions.saved = options }
        result = options
        modal?.end(.OK)
    }
}

extension CmykTone {
    /// The analysis scope of a Levels or Curves dialog on a CMYK document: the layer a
    /// destructive edit rewrites, else the backdrop below the adjustment (where a new one lands,
    /// above the active entry). The coverage is the selection a destructive edit or a new layer
    /// is confined to.
    static func scope(_ document: ImageDocument, mode: AdjustmentSheetMode,
                      activeLayer: Int) -> (request: [String: Any], coverage: [UInt16]) {
        switch mode {
        case .destructive:
            return (["source": "layer", "layer": activeLayer],
                    document.canvasSelection?.maskSamples16() ?? [])
        case .create(let selection):
            return (["source": "backdrop", "below": activeLayer + 1],
                    selection.map { $0.map { UInt16($0) * 257 } } ?? [])
        case .edit(let index, _):
            return (["source": "backdrop", "below": index], [])
        }
    }
}
