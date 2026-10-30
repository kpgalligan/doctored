import AppKit

/// The Channel Mixer dialog: an Output popup, the three source sliders, a
/// Constant slider and the display-only Total readout, plus Monochrome.
///
/// Every parameter, range and default comes from `AdjustmentSchema`; the
/// lifecycle — the captured handles, the debounced in-context preview, one
/// undo step on Apply and nothing on Cancel — is `AdjustmentSheet`'s, in all
/// three modes (Image ▸ Adjustments, Layer ▸ New Adjustment Layer, and
/// Adjustment Options… on an existing layer).
///
/// The dialog edits FOUR rows and shows one: the popup only chooses which
/// of `red`, `green`, `blue` and `gray` the four sliders write. Monochrome
/// swaps the popup for a single Gray entry (the core then sends the gray
/// row to all three outputs) and leaves the colour rows untouched in the
/// params, so the checkbox is reversible without losing either setting.
///
/// Units follow the core's schema row: a slider runs −200…200 % on screen and
/// −2…2 in the params, quantized to whole percent — what makes an unchanged
/// re-commit byte-stable, since 40/100 is exactly the 0.4 the schema defaults
/// to. The matrix runs on the ENCODED values (no linearization), and the
/// constant is a fraction too: 0.5 adds 127.5 levels.
///
/// On a CMYK document (#547) the outputs are Cyan, Magenta, Yellow and Black
/// and the sources C, M, Y and K — the CMYK mixer, on the ink
/// (`cmyk_adjust_ink`) — and Monochrome's Gray row sends one mix to all four
/// plates.
final class ChannelMixerSheet: AdjustmentSheet {
    /// The rows the core parses, and the keys inside one row. Both orders are
    /// the core's (`adjust_mix::MIXER_ROWS`, `MIXER_KEYS`; on CMYK
    /// `CmykAdjustFilter.mixerRows`, `mixerSources`), so an index here is the
    /// same slot there; the gray row is last, the constant last in a row.
    private var cmyk: Bool { document.doc?.isCmyk == true }
    private var rowKeys: [String] {
        cmyk ? CmykAdjustFilter.mixerRows.map(\.key) : ["red", "green", "blue", "gray"]
    }
    private var sourceKeys: [String] {
        cmyk ? CmykAdjustFilter.mixerSources : ["r", "g", "b", "constant"]
    }
    private var sourceLabels: [String] {
        cmyk ? ["Cyan:", "Magenta:", "Yellow:", "Black:", "Constant:"]
            : ["Red:", "Green:", "Blue:", "Constant:"]
    }
    /// The colour outputs (every row but gray) and the sources summed in the Total.
    private var outputCount: Int { rowKeys.count - 1 }
    private var sourceCount: Int { sourceKeys.count - 1 }

    /// A row's defaults by source key: the schema's, or the CMYK identity rows.
    private func rowDefaults(_ key: String) -> [String: Any] {
        guard cmyk, let row = CmykAdjustFilter.mixerRows.first(where: { $0.key == key }) else {
            return AdjustmentSchema.objectDefaults(for: .channelMixer, path: [key])
        }
        return Dictionary(uniqueKeysWithValues: zip(CmykAdjustFilter.mixerSources, row.defaults)
            .map { ($0.0, $0.1 as Any) })
    }

    /// Every row's four values as the core's fractions, read defensively
    /// against the schema's own defaults so a params object that omits a row
    /// — the usual case — opens on exactly the numbers the core would use.
    private var rowValues: [String: [Double]] = [:]

    /// The rows the params the sheet opened on spelled out — see
    /// `currentParams()` for why that is worth remembering.
    private var explicitRows: Set<String> = []
    private var explicitMonochrome = false

    private var monochrome = false

    override func loadState() {
        var out: [String: [Double]] = [:]
        for key in rowKeys {
            let defaults = rowDefaults(key)
            let stored = initial.object(key)
            out[key] = sourceKeys.map { source in
                Self.number(stored?[source], defaults[source] as? Double ?? 0)
            }
        }
        rowValues = out
        explicitRows = Set(rowKeys.filter { initial.object($0) != nil })
        explicitMonochrome = initial.params["monochrome"] != nil
        monochrome = initial.bool("monochrome", default: false)
    }

    /// The COLOUR output the popup last showed. Monochrome hides it behind
    /// the Gray row rather than clearing it, so unchecking the box comes back
    /// to the channel the user was on.
    private var output = 0

    private var outputPopup: NSPopUpButton?
    /// The four house rows, kept by their parts: `AdjustmentSheet.sliderRow`
    /// hands back its views and only the row's own action updates the
    /// readout, so switching rows has to set both (the `SliderSheetController`
    /// arrangement).
    private var sliders: [NSSlider] = []
    private var readouts: [NSTextField] = []
    private let totalReadout = NSTextField(labelWithString: "")

    // MARK: - Content

    override func makeContent() -> NSView {
        sliders = []
        readouts = []
        var rows: [[NSView]] = []
        // A preset or Default load rebuilds these controls while `output`
        // keeps its channel, so the popup opens on the row the sliders write.
        let outputViews = popupRow(
            "Output:", titles: outputTitles(monochrome: monochrome),
            selected: monochrome ? 0 : output
        ) { [weak self] index in
            self?.output = index
            self?.showRow()
        }
        outputPopup = outputViews.compactMap { $0 as? NSPopUpButton }.first
        outputPopup?.isEnabled = !monochrome
        rows.append(outputViews)
        rows.append(
            checkboxRow("Monochrome", value: monochrome) { [weak self] on in
                self?.setMonochrome(on)
            })
        // The order: the three source channels, the Total they
        // sum to, then the Constant — which is an offset, not a share of the
        // input, and so is not in the Total.
        for index in 0..<sourceCount { rows.append(track(sourceRow(index))) }
        totalReadout.font = DS.mono(12)
        totalReadout.alignment = .right
        totalReadout.widthAnchor.constraint(equalToConstant: 52).isActive = true
        rows.append([fieldLabel("Total:"), NSGridCell.emptyContentView, totalReadout])
        rows.append(track(sourceRow(sourceCount)))

        let note = NSTextField(
            wrappingLabelWithString:
                "Each output channel is a weighted sum of the source channels plus a "
                + "constant. A total above 100 % can clip the output, which is all the "
                + "warning means — the mix is applied exactly as set either way.")
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2

        let stack = NSStackView(views: [AdjustmentSheet.grid(rows), note])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        updateTotal()
        return stack
    }

    private func sourceRow(_ index: Int) -> [NSView] {
        sliderRow(
            sourceLabels[index], range: -200...200, value: value(index) * 100,
            format: Self.percent
        ) { [weak self] percent in
            self?.setValue(index, percent.rounded() / 100)
        }
    }

    /// Keeps a house row's slider and readout so `showRow()` can retarget
    /// them, and returns the row unchanged for the grid.
    private func track(_ row: [NSView]) -> [NSView] {
        if let slider = row.compactMap({ $0 as? NSSlider }).first { sliders.append(slider) }
        if let readout = row.compactMap({ $0 as? NSTextField }).last { readouts.append(readout) }
        return row
    }

    // MARK: - State

    /// The row the sliders currently write: the gray one under Monochrome,
    /// otherwise whichever colour output the popup names.
    private var currentKey: String {
        monochrome ? "gray" : rowKeys[min(max(output, 0), outputCount - 1)]
    }

    private func value(_ index: Int) -> Double {
        let values = rowValues[currentKey] ?? []
        return index < values.count ? values[index] : 0
    }

    private func setValue(_ index: Int, _ value: Double) {
        guard var values = rowValues[currentKey], index < values.count else { return }
        values[index] = Self.clamped(value, -2, 2)
        rowValues[currentKey] = values
    }

    private func setMonochrome(_ on: Bool) {
        monochrome = on
        if let popup = outputPopup {
            popup.removeAllItems()
            popup.addItems(withTitles: outputTitles(monochrome: on))
            popup.selectItem(at: on ? 0 : output)
            // With one entry there is nothing to choose: the menu holds
            // Gray alone and is greyed out.
            popup.isEnabled = !on
        }
        showRow()
    }

    /// Reloads the four sliders from the row the popup now names. AppKit
    /// sends no action for a programmatic value change, so the readouts are
    /// set here too — and the caller's own `valuesChanged()` is the one
    /// preview the whole switch needs.
    private func showRow() {
        for index in 0..<min(sliders.count, readouts.count) {
            let percent = value(index) * 100
            sliders[index].doubleValue = percent
            readouts[index].stringValue = Self.percent(percent)
        }
        updateTotal()
    }

    /// The Total: the three SOURCE weights summed and shown signed.
    /// Above 100 % the output can clip, which is the only thing the warning
    /// colour says — rows may sum to anything, and nothing here clamps them.
    /// The colour is the amber the histogram's clipping wedge already uses
    /// (`HistogramView`); `DS` has no warning token, and inventing one for a
    /// single readout would be a worse kind of drift.
    private func updateTotal() {
        let values = rowValues[currentKey] ?? []
        let total = (values.prefix(sourceCount).reduce(0, +) * 100).rounded()
        totalReadout.stringValue = String(format: "%+.0f %%", total)
        totalReadout.textColor = total > 100 ? .systemOrange : DS.textMuted
    }

    override func valuesChanged() {
        updateTotal()
        super.valuesChanged()
    }

    // MARK: - Params

    /// One rule for every key here: **write it when its value differs from
    /// the core's default for it, or when the params the sheet opened on
    /// already carried it.** The core reads an absent key (a whole absent
    /// row included) as exactly those defaults, so the pixels are the same
    /// either way; what the rule buys is a small meta — it travels in `.rz`
    /// and in every `get_document` reply — and a byte-identical re-commit
    /// when nothing was touched.
    override func currentParams() -> [String: Any] {
        var params: [String: Any] = [:]
        if monochrome || explicitMonochrome {
            params["monochrome"] = monochrome
        }
        for key in rowKeys {
            guard let values = rowValues[key], values.count == sourceKeys.count else {
                continue
            }
            let defaults = rowDefaults(key)
            let isDefault = zip(sourceKeys, values).allSatisfy { source, value in
                value == (defaults[source] as? Double ?? 0)
            }
            guard !isDefault || explicitRows.contains(key) else { continue }
            params[key] = Dictionary(uniqueKeysWithValues: zip(sourceKeys, values))
        }
        return params
    }

    override var footnote: String { "total is advisory · one undo step" }

    // MARK: - Helpers

    private func outputTitles(monochrome: Bool) -> [String] {
        monochrome ? ["Gray"] : rowKeys.prefix(outputCount).map { $0.capitalized }
    }

    /// The readout: whole percent, the step the sliders quantize to.
    private static func percent(_ value: Double) -> String {
        String(format: "%.0f %%", value.rounded())
    }

    /// One nested value, defensively — missing, non-numeric and non-finite
    /// all read as `fallback`, the contract `AdjustmentLayerPayload.number`
    /// has for a top-level key — clamped into the schema's ±2.
    private static func number(_ value: Any?, _ fallback: Double) -> Double {
        guard let number = (value as? NSNumber)?.doubleValue, number.isFinite else {
            return fallback
        }
        return clamped(number, -2, 2)
    }

    private static func clamped(_ value: Double, _ low: Double, _ high: Double) -> Double {
        min(max(value, low), high)
    }
}
