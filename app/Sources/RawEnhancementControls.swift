import AppKit

/// Transferable globals remain separate from camera-specific calibration and
/// local mask positions. Unmoved rows produce no recipe-version change.
final class RawEnhancementControls {
    let view = NSStackView()
    /// Develop grain, placed by the window after the Detail knobs.
    let grainView = NSStackView()
    /// Texture, Clarity and Dehaze, placed by the window just before `grainView`.
    let presenceView = NSStackView()
    var onChange: (() -> Void)?
    private let originalBaseline: Double
    private let originalMoire: Double
    private let originalVignette: RawVignetteSettings
    private var baseline: Double
    private var moire: Double
    private var vignette: RawVignetteSettings
    private let originalGrain: RawGrainSettings
    private var grain: RawGrainSettings
    private let lensControls: RawSourceGeometryControls
    private var rows: [CardControlRow] = []
    private var grainRows: [CardControlRow] = []
    private let originalPresence: RawPresenceSettings
    private var presence: RawPresenceSettings
    private var presenceRows: [CardControlRow] = []
    /// False at the 32-bit float (linear) encoding: Presence is defined
    /// on display-referred values (`RawPresenceSettings.floatRefusal`), so its
    /// rows are disabled there and the request removes any Presence the
    /// recipe has saved.
    var presenceAvailable = true { didSet { updatePresenceEnablement() } }

    init(recipe: RawDevelopmentRecipe) {
        originalBaseline = recipe.enhancements?.baselineCompensation ?? 0
        originalMoire = recipe.enhancements?.moire ?? Double(recipe.moire)
        originalVignette = recipe.enhancements?.vignette ?? .neutral
        lensControls = RawSourceGeometryControls(geometry: recipe.sourceGeometry)
        originalGrain = recipe.enhancements?.grain ?? .neutral
        baseline = originalBaseline; moire = originalMoire; vignette = originalVignette
        grain = originalGrain
        originalPresence = recipe.enhancements?.presence ?? .neutral
        presence = originalPresence
        view.orientation = .vertical; view.alignment = .leading; view.spacing = 10
        let title = NSTextField(labelWithString: "Camera and manual lens controls")
        title.font = DS.sans(12, weight: .semibold)
        view.addArrangedSubview(title)
        let reference = Double(recipe.enhancements?.baselineReference ?? recipe.baselineExposure)
        let note = NSTextField(wrappingLabelWithString: String(format:
            "Camera EV uses this camera's captured baseline (%+.3f EV). Manual vignette center uses %@; it follows local development before explicit optics.",
            reference, recipe.vignetteReference.map { "\($0.referenceSize[0])×\($0.referenceSize[1]) saved reference" } ?? "current source dimensions"))
        note.font = DS.sans(11); note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = 370
        note.widthAnchor.constraint(equalToConstant: 370).isActive = true
        view.addArrangedSubview(note)
        add("Camera EV:", "baseline", -4...4, baseline, ev) { $0.baseline = $1 }
        add("Moire:", "moire", 0...1, moire, decimal) { $0.moire = $1 }
        rows[1].isEnabled = recipe.moireSupported
        rows[1].toolTip = recipe.moireSupported ? "Supported by this camera decoder." : "This camera decoder does not support moire reduction."
        add("Edge exposure:", "vignette-edge", -4...4, vignette.edgeExposure, ev) { $0.vignette.edgeExposure = $1 }
        add("Vignette X:", "vignette-x", 0...1, vignette.centerX, decimal) { $0.vignette.centerX = $1 }
        add("Vignette Y:", "vignette-y", 0...1, vignette.centerY, decimal) { $0.vignette.centerY = $1 }
        add("Falloff:", "vignette-falloff", 0.25...4, vignette.falloff, decimal) { $0.vignette.falloff = $1 }
        lensControls.onChange = { [weak self] in self?.onChange?() }
        view.addArrangedSubview(lensControls.view)
        grainView.orientation = .vertical; grainView.alignment = .leading; grainView.spacing = 10
        let grainTitle = NSTextField(labelWithString: "Grain")
        grainTitle.font = DS.sans(12, weight: .semibold)
        grainView.addArrangedSubview(grainTitle)
        addGrain("Amount:", "grain-amount", RawGrainSettings.amountRange, grain.amount,
                 { String(format: "%.0f", $0) }) { $0.grain.amount = $1.rounded() }
        addGrain("Size:", "grain-size", RawGrainSettings.sizeRange, grain.size,
                 { String(format: "%.2f px", $0) }) { $0.grain.size = $1 }
        addGrain("Roughness:", "grain-roughness", RawGrainSettings.roughnessRange, grain.roughness,
                 { String(format: "%.0f", $0) }) { $0.grain.roughness = $1 }
        updateGrainEnablement()
        presenceView.orientation = .vertical; presenceView.alignment = .leading; presenceView.spacing = 10
        let presenceTitle = NSTextField(labelWithString: "Presence")
        presenceTitle.font = DS.sans(12, weight: .semibold)
        presenceView.addArrangedSubview(presenceTitle)
        addPresence("Texture:", "texture", presence.texture) { $0.presence.texture = $1.rounded() }
        addPresence("Clarity:", "clarity", presence.clarity) { $0.presence.clarity = $1.rounded() }
        addPresence("Dehaze:", "dehaze", presence.dehaze) { $0.presence.dehaze = $1.rounded() }
        updatePresenceEnablement()
    }

    var settings: RawDevelopSettings {
        var value = RawDevelopSettings()
        if baseline != originalBaseline { value.baselineCompensation = baseline }
        if moire != originalMoire { value.moire = moire }
        if vignette != originalVignette { value.vignette = vignette }
        // Amount 0 is no grain whatever its inert size/roughness say.
        if grain != originalGrain, !(grain.isNeutral && originalGrain.isNeutral) {
            value.grain = grain
        }
        if !presenceAvailable {
            if !originalPresence.isNeutral { value.texture = 0; value.clarity = 0; value.dehaze = 0 }
        } else {
            if presence.texture != originalPresence.texture { value.texture = presence.texture }
            if presence.clarity != originalPresence.clarity { value.clarity = presence.clarity }
            if presence.dehaze != originalPresence.dehaze { value.dehaze = presence.dehaze }
        }
        value.sourceGeometry = lensControls.settings
        return value
    }

    func apply(recipe: RawDevelopmentRecipe) {
        baseline = recipe.enhancements?.baselineCompensation ?? 0
        moire = recipe.enhancements?.moire ?? Double(recipe.moire)
        vignette = recipe.enhancements?.vignette ?? .neutral
        grain = recipe.enhancements?.grain ?? .neutral
        presence = recipe.enhancements?.presence ?? .neutral
        lensControls.apply(recipe.sourceGeometry)
        updateRows()
    }
    func reset() {
        baseline = originalBaseline; moire = originalMoire; vignette = originalVignette
        grain = originalGrain
        presence = originalPresence
        lensControls.reset()
        updateRows()
    }
    private func updateRows() {
        for (row, value) in zip(rows, [baseline, moire, vignette.edgeExposure,
                                      vignette.centerX, vignette.centerY, vignette.falloff]) {
            row.setValue(value)
        }
        for (row, value) in zip(grainRows, [grain.amount, grain.size, grain.roughness]) {
            row.setValue(value)
        }
        for (row, value) in zip(presenceRows, [presence.texture, presence.clarity, presence.dehaze]) {
            row.setValue(value)
        }
        updateGrainEnablement()
        updatePresenceEnablement()
    }
    private func updatePresenceEnablement() {
        for row in presenceRows { row.isEnabled = presenceAvailable }
    }
    /// Size and Roughness are inert while Amount is 0.
    private func updateGrainEnablement() {
        for row in grainRows.dropFirst() { row.isEnabled = !grain.isNeutral }
    }
    private func addGrain(_ label: String, _ identifier: String, _ range: ClosedRange<Double>,
                          _ value: Double, _ format: @escaping (Double) -> String,
                          _ write: @escaping (RawEnhancementControls, Double) -> Void) {
        let row = cardSliderRow(label: label, min: range.lowerBound, max: range.upperBound,
            value: value, format: format) { [weak self] value in
                guard let self else { return }
                write(self, value); self.updateGrainEnablement(); self.onChange?()
            }
        row.setAccessibilityIdentifier("raw-develop-" + identifier)
        grainRows.append(row); grainView.addArrangedSubview(row)
    }
    private func addPresence(_ label: String, _ identifier: String, _ value: Double,
                             _ write: @escaping (RawEnhancementControls, Double) -> Void) {
        let range = RawPresenceSettings.range
        let row = cardSliderRow(label: label, min: range.lowerBound, max: range.upperBound,
            value: value, format: { String(format: "%+.0f", $0) }) { [weak self] value in
                guard let self else { return }
                write(self, value); self.onChange?()
            }
        row.setAccessibilityIdentifier("raw-develop-" + identifier)
        presenceRows.append(row); presenceView.addArrangedSubview(row)
    }
    private func add(_ label: String, _ identifier: String, _ range: ClosedRange<Double>,
                     _ value: Double, _ format: @escaping (Double) -> String,
                     _ write: @escaping (RawEnhancementControls, Double) -> Void) {
        let row = cardSliderRow(label: label, min: range.lowerBound, max: range.upperBound,
            value: value, format: format) { [weak self] value in
                guard let self else { return }
                write(self, value); self.onChange?()
            }
        row.setAccessibilityIdentifier("raw-develop-" + identifier)
        rows.append(row); view.addArrangedSubview(row)
    }
    private var ev: (Double) -> String { { String(format: "%+.2f EV", $0) } }
    private var decimal: (Double) -> String { { String(format: "%.3f", $0) } }
}
