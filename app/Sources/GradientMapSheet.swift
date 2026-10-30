import AppKit

/// The Gradient Map dialog: the reusable GradientEditorView, a stop position and opacity field, Reverse and Dither.
///
/// Every parameter, range and default comes from `AdjustmentSchema`; the
/// lifecycle — the captured handles, the debounced in-context preview, one
/// undo step on Apply and nothing on Cancel — is `AdjustmentSheet`'s, in all
/// three modes (Image ▸ Adjustments, Layer ▸ New Adjustment Layer, and
/// Adjustment Options… on an existing layer).
///
/// The op maps each pixel's Rec. 709 luma through the ramp, and a stop's
/// opacity blends back toward the pixel's ORIGINAL colour — an adjustment
/// may not touch alpha, so that is the only meaning transparency can have
/// here.
///
/// **The stop colours are this document's own numbers**, not authored sRGB:
/// an adjustment is a function of the pixels it transforms, so its colours
/// live in their space and convert nowhere. The editor is handed
/// `documentColorSpace` for exactly that reason, and the named presets below
/// — which really are sRGB colours — convert ONCE, at the moment they are
/// picked, through `AdjustmentColor.documentHex(forSRGBHex:in:)`.
///
/// On a CMYK document (#547) a stop is an INK (`{position, ink, opacity}`):
/// the editor shows each ink's colour, a stop it opened on or a swatch with a
/// stored ink keeps that ink exactly, and any other colour is separated
/// through the document's profile (`CmykPaint.ink`) when the params are read.
///
/// The sheet writes the Method its "Method:" popup holds:
/// Classic (`"method": "classic"`, the default) or Perceptual (`"method":
/// "perceptual"`, the same law over Oklab stops), with its Smoothness
/// (0…100 %, `FillLayerSheetController`'s field) and the editor's midpoint
/// diamonds and Midpoint field (`GradientStopsPane`'s). The popup is absent on
/// a CMYK document, which refuses Perceptual and always writes Classic. Linear
/// and Smooth are not offered (refused for good). A legacy map (no `method`,
/// the linear ramp) opens as what it draws, Classic at Smoothness 0 % with
/// every midpoint at 50 %, so Apply on an untouched legacy layer commits
/// nothing (`AdjustmentSchema.effective`).
final class GradientMapSheet: AdjustmentSheet, NSTextFieldDelegate {
    /// The preset ramps, in popup order. "Custom" is index 0 and picking it
    /// changes nothing: it is what the popup shows whenever the ramp matches
    /// no preset, which is the moment the user edits a stop.
    private static let presetTitles = [
        "Custom", "Black → White", "White → Black", "Paint → Background",
    ]

    /// The Method popup's titles and the `method` each writes, in popup
    /// order (class doc).
    private static let methodTitles = ["Classic", "Perceptual"]
    private static let methodValues = ["classic", "perceptual"]

    private let editor = GradientEditorView(frame: NSRect(x: 0, y: 0, width: 376, height: 72))
    private let positionField = NSTextField(string: "")
    private let opacityField = NSTextField(string: "")
    /// The selected diamond's midpoint, 5…95 % of its segment; disabled with
    /// no diamond selected.
    private let midpointField = NSTextField(string: "")
    /// The map's Smoothness, 0…100 %.
    private let smoothnessField = NSTextField(string: "")
    private weak var presetPopup: NSPopUpButton?
    private weak var colorWell: NSColorWell?
    private var dither = true
    /// The Smoothness as a fraction (class doc): a legacy map's is 0.
    private var smoothness = 1.0
    /// The `method` the sheet writes on an RGB document (class doc).
    private var method = "classic"
    /// Set once `makeContent()` has run. `currentParams()` can then read the
    /// controls; before that the sheet has nothing but what it opened on.
    private var didBuildControls = false
    /// True while the controls are being refilled FROM the model, so the
    /// writes that refill triggers (an `NSColorWell` sends its action when
    /// its colour is set) are not read back as user edits.
    private var isSyncing = false
    /// A CMYK document's known inks by the colour the editor shows
    /// (`CmykShownInks`), made on first use.
    private var shownInks: CmykShownInks?

    /// The CMYK document's typed handle when this sheet maps to ink.
    private var cmyk: RasterPrecisionDocument? {
        document.doc?.isCmyk == true ? document.doc?.precision : nil
    }

    private func inks(in doc: RasterPrecisionDocument) -> CmykShownInks {
        shownInks ?? CmykShownInks(document: doc, space: documentColorSpace)
    }

    /// The editor's colour of `ink`, remembered so the same stop keeps its ink.
    private func shownHex(ofInk ink: [Double], in doc: RasterPrecisionDocument) -> String {
        var inks = inks(in: doc)
        let hex = inks.shownHex(ofInk: ink)
        shownInks = inks
        return hex
    }

    /// The ink of a colour the editor shows: a known one, else separated.
    private func ink(ofShown hex: String, in doc: RasterPrecisionDocument) -> [Double] {
        inks(in: doc).ink(ofShown: hex)
    }

    /// The colour rule, at the surface that carries a swatch: what a
    /// stop stores is the DOCUMENT's numbers, not an authored sRGB colour.
    override var footnote: String { "document-space colours · 1 undo" }

    // MARK: - Controls

    override func loadState() {
        editor.fill = Self.fill(from: initial)
        if let doc = cmyk { editor.fill = inkFill(from: initial, in: doc) }
        editor.selectedStop = 0
        dither = initial.bool("dither", default: true)
        smoothness = Self.smoothness(of: initial)
        method = initial.params["method"] as? String == "perceptual" ? "perceptual" : "classic"
    }

    /// Whether `payload` names a method, Classic or Perceptual, and so
    /// reads its Smoothness and midpoints; a legacy one has no `method`.
    private static func namesMethod(_ payload: AdjustmentLayerPayload) -> Bool {
        payload.params["method"] != nil
    }

    /// The Smoothness a map draws with (class doc): a legacy map's 0, a
    /// Classic map's its `gradient.smoothness` (absent: 1), clamped.
    private static func smoothness(of payload: AdjustmentLayerPayload) -> Double {
        guard namesMethod(payload) else { return 0 }
        let raw = (payload.object("gradient")?["smoothness"] as? NSNumber)?.doubleValue ?? 1
        return raw.isFinite ? min(max(raw, 0), 1) : 1
    }

    /// A stop's stored midpoint: nil at 50 %, absent or out of range,
    /// and always nil on a legacy map, which draws none.
    private static func midpoint(_ stop: [String: Any], _ key: String, classic: Bool) -> Double? {
        guard classic, let value = (stop[key] as? NSNumber)?.doubleValue,
              GradientStop.midpointRange.contains(value), value != 0.5
        else { return nil }
        return value
    }

    /// A CMYK gradient's stops as the editor's colours (an absent or short
    /// list: K 100 % → paper, the default map in CMYK).
    private func inkFill(from payload: AdjustmentLayerPayload, in doc: RasterPrecisionDocument) -> GradientFill {
        var out = GradientFill()
        let object = payload.object("gradient")
        let classic = Self.namesMethod(payload)
        if let reverse = object?["reverse"] as? NSNumber { out.reverse = reverse.boolValue }
        var stops: [GradientStop] = []
        for entry in object?["stops"] as? [Any] ?? [] {
            guard let stop = entry as? [String: Any],
                  let ink = (stop["ink"] as? [Any])?.compactMap({ ($0 as? NSNumber)?.doubleValue }),
                  ink.count == 4 else { continue }
            let position = (stop["position"] as? NSNumber)?.doubleValue ?? 0
            let opacity = (stop["opacity"] as? NSNumber)?.doubleValue ?? 1
            stops.append(GradientStop(
                position: position.isFinite ? min(max(position, 0), 1) : 0,
                color: shownHex(ofInk: ink.map { min(max($0, 0), 1) }, in: doc),
                opacity: opacity.isFinite ? min(max(opacity, 0), 1) : 1,
                midpoint: Self.midpoint(stop, "midpoint", classic: classic),
                opacityMidpoint: Self.midpoint(stop, "opacity_midpoint", classic: classic)))
        }
        if stops.count < GradientEditorView.minStops {
            stops = [GradientStop(position: 0, color: shownHex(ofInk: [0, 0, 0, 1], in: doc), opacity: 1),
                     GradientStop(position: 1, color: shownHex(ofInk: [0, 0, 0, 0], in: doc), opacity: 1)]
        }
        out.stops = stops
        return out
    }

    override func makeContent() -> NSView {
        editor.colorSpace = documentColorSpace
        editor.showsMidpoints = true
        editor.onFillChanged = { [weak self] _ in self?.editorChanged() }
        editor.onSelectionChanged = { [weak self] _ in self?.stopSelectionChanged() }
        editor.onEditColor = { [weak self] _ in self?.editStopColor() }
        editor.onMidpointSelectionChanged = { [weak self] _ in self?.stopSelectionChanged() }
        editor.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            editor.widthAnchor.constraint(equalToConstant: sheetWidth - sheetInset * 2),
            editor.heightAnchor.constraint(equalToConstant: 72),
        ])

        for field in [positionField, opacityField, midpointField, smoothnessField] {
            styleField(field)
        }
        if let formatter = midpointField.formatter as? NumberFormatter {
            formatter.minimum = 5
            formatter.maximum = 95
            formatter.maximumFractionDigits = 0
        }
        midpointField.identifier = NSUserInterfaceItemIdentifier("gradient-map-midpoint")
        smoothnessField.identifier = NSUserInterfaceItemIdentifier("gradient-map-smoothness")
        smoothnessField.doubleValue = smoothness * 100

        let presetRow = popupRow(
            "Preset:", titles: Self.presetTitles, selected: presetIndex()
        ) { [weak self] index in
            self?.applyPreset(index)
        }
        presetPopup = presetRow.compactMap { $0 as? NSPopUpButton }.first
        let colorViews = colorRow("Color:", hex: selectedStop()?.color ?? "#000000") {
            [weak self] hex in
            self?.colorPicked(hex)
        }
        colorWell = colorViews.compactMap { $0 as? NSColorWell }.first
        // RGB only (class doc): a CMYK document refuses Perceptual.
        let methodRows: [[NSView]] = cmyk != nil ? [] : [
            popupRow(
                "Method:", titles: Self.methodTitles,
                selected: Self.methodValues.firstIndex(of: method) ?? 0
            ) { [weak self] index in
                self?.method = Self.methodValues[index]
            },
        ]

        let rows: [[NSView]] = methodRows + [
            presetRow,
            [NSGridCell.emptyContentView, microHeader("Selected stop")],
            [fieldLabel("Position:"), percentRow(positionField)],
            [fieldLabel("Opacity:"), percentRow(opacityField)],
            colorViews,
            [fieldLabel("Midpoint:"), percentRow(midpointField)],
            [fieldLabel("Smoothness:"), percentRow(smoothnessField)],
            checkboxRow("Reverse", value: editor.fill.reverse) { [weak self] on in
                self?.setReverse(on)
            },
            // On by default: an 8-bit map over a smooth sky bands visibly
            // without it, and the offset is a deterministic sub-level nudge
            // (the Dissolve blend mode's own threshold), not noise.
            checkboxRow("Dither", value: dither) { [weak self] on in
                self?.dither = on
            },
        ]
        let content = NSStackView(views: [editor, Self.grid(rows)])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12
        didBuildControls = true
        syncStopControls()
        return content
    }

    override func currentParams() -> [String: Any] {
        // The lifecycle always builds the controls first (loadView runs
        // before viewDidAppear and before Apply); answering with the params
        // the sheet opened on is the honest fallback if that ever changes.
        guard didBuildControls else { return initial.params }
        var fill = editor.fill
        fill.smoothness = smoothness >= 1 ? nil : smoothness
        if let doc = cmyk {
            var gradient = fill.mapParams
            gradient["stops"] = fill.stops.map { stop -> [String: Any] in
                var out: [String: Any] = [
                    "position": stop.position, "ink": ink(ofShown: stop.color, in: doc),
                    "opacity": stop.opacity,
                ]
                if let m = stop.midpoint { out["midpoint"] = m }
                if let m = stop.opacityMidpoint { out["opacity_midpoint"] = m }
                return out
            }
            return ["method": "classic", "gradient": gradient, "dither": dither]
        }
        return ["method": method, "gradient": fill.mapParams, "dither": dither]
    }

    // MARK: - Editing

    private func editorChanged() {
        syncStopControls()
        updatePresetSelection()
        valuesChanged()
    }

    private func stopSelectionChanged() {
        syncStopControls()
    }

    /// A double-click on a stop opens the colour panel — by activating the
    /// sheet's own well, which is already bound to this document's space, so
    /// the panel is driven by AppKit rather than by a second hand-rolled
    /// target on a shared window.
    private func editStopColor() {
        colorWell?.activate(true)
    }

    private func colorPicked(_ hex: String) {
        guard !isSyncing else { return }
        editor.setColor(hex, forStop: editor.selectedStop)
        updatePresetSelection()
    }

    /// The two fields edit live, as `ImageSizeSheetController`'s do: every
    /// keystroke moves the stop and re-previews, and the field being typed
    /// in is never rewritten underneath the caret (`except:`). An empty
    /// field is a keystroke on the way to a number, not a request to put
    /// the stop at zero.
    func controlTextDidChange(_ obj: Notification) {
        guard !isSyncing, let field = obj.object as? NSTextField,
              !field.stringValue.trimmingCharacters(in: .whitespaces).isEmpty
        else { return }
        // Smoothness belongs to the map, the Midpoint field to the selected
        // DIAMOND, neither to whichever stop is selected.
        if field === smoothnessField {
            let value = field.doubleValue / 100
            guard value.isFinite else { return }
            smoothness = min(max(value, 0), 1)
            valuesChanged()
            return
        }
        if field === midpointField {
            guard let midpoint = editor.selectedMidpoint else { return }
            editor.setMidpoint(min(max(field.doubleValue, 5), 95) / 100, for: midpoint)
            syncStopControls(except: field)
            valuesChanged()
            return
        }
        guard selectedStop() != nil else { return }
        let index = editor.selectedStop
        let percent = min(max(field.doubleValue, 0), 100)
        if field === positionField {
            editor.setPosition(percent / 100, forStop: index)
        } else if field === opacityField {
            editor.setOpacity(percent / 100, forStop: index)
        } else {
            return
        }
        syncStopControls(except: field)
        updatePresetSelection()
        valuesChanged()
    }

    /// Reformats both fields once the caret leaves — a dragged stop's
    /// position is a fraction of a percent, and this is where the field
    /// stops showing what was typed and starts showing what is stored.
    func controlTextDidEndEditing(_ obj: Notification) {
        syncStopControls()
    }

    private func setReverse(_ on: Bool) {
        var fill = editor.fill
        fill.reverse = on
        editor.fill = fill
    }

    /// Replaces the ramp with a preset's stops, keeping Reverse as the user
    /// left it (it is its own control, and a preset should not reach across
    /// and flip it). Index 0 is "Custom" and changes nothing.
    private func applyPreset(_ index: Int) {
        guard let stops = Self.presetStops(index, in: documentColorSpace) else { return }
        // CMYK: the paint pair keeps the swatches' stored inks.
        if index == 3, cmyk != nil, stops.count == 2 {
            let shared = ToolOptionsStore.shared.sharedState
            if let doc = cmyk {
                var inks = inks(in: doc)
                if let ink = shared.foregroundInk { inks.remember(ink.unit, shownAs: stops[0].color) }
                if let ink = shared.backgroundInk { inks.remember(ink.unit, shownAs: stops[1].color) }
                shownInks = inks
            }
        }
        var fill = editor.fill
        fill.stops = stops
        editor.fill = fill
        editor.selectedStop = 0
        syncStopControls()
    }

    // MARK: - Keeping the controls and the ramp in step

    private func selectedStop() -> GradientStop? {
        let stops = editor.fill.stops
        let index = editor.selectedStop
        return stops.indices.contains(index) ? stops[index] : nil
    }

    /// Refills the per-stop controls from the ramp. `except` is the field
    /// the user is typing in, which must keep its own text (and its caret).
    private func syncStopControls(except editing: NSTextField? = nil) {
        guard didBuildControls else { return }
        isSyncing = true
        defer { isSyncing = false }
        syncMidpointField(except: editing)
        guard let stop = selectedStop() else {
            for field in [positionField, opacityField] {
                field.stringValue = ""
                field.isEnabled = false
            }
            colorWell?.isEnabled = false
            return
        }
        for field in [positionField, opacityField] { field.isEnabled = true }
        colorWell?.isEnabled = true
        // Written through the field's own formatter, so the decimal
        // separator is the user's and what is shown parses back.
        if positionField !== editing { positionField.doubleValue = stop.position * 100 }
        if opacityField !== editing { opacityField.doubleValue = stop.opacity * 100 }
        colorWell?.color =
            AdjustmentColor.color(fromHex: stop.color, in: documentColorSpace) ?? .black
    }

    /// The Midpoint field follows the selected diamond (`GradientStopsPane`'s
    /// rule): cleared and disabled without one.
    private func syncMidpointField(except editing: NSTextField?) {
        let stops = editor.fill.stops
        guard let midpoint = editor.selectedMidpoint, stops.indices.contains(midpoint.segment)
        else {
            midpointField.stringValue = ""
            midpointField.isEnabled = false
            return
        }
        midpointField.isEnabled = true
        if midpointField !== editing {
            midpointField.doubleValue = (GradientEditorView.midpointValue(
                of: stops[midpoint.segment], opacity: midpoint.opacity) * 100).rounded()
        }
    }

    /// The preset the ramp currently spells, or 0 ("Custom"). Compared on
    /// the CONVERTED colours, so a preset stays recognized on a document
    /// whose numbers are not sRGB's.
    private func presetIndex() -> Int {
        let stops = editor.fill.stops
        for index in 1..<Self.presetTitles.count
        where Self.presetStops(index, in: documentColorSpace) == stops {
            return index
        }
        return 0
    }

    private func updatePresetSelection() {
        presetPopup?.selectItem(at: presetIndex())
    }

    // MARK: - Layout pieces

    private func microHeader(_ title: String) -> NSTextField {
        NSTextField(labelWithAttributedString: DS.microLabel(title))
    }

    /// A percentage field in the house style: 0…100 with one decimal, which
    /// is as fine as the ~376 pt ramp resolves (a point is about a quarter
    /// of a percent).
    private func styleField(_ field: NSTextField) {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.allowsFloats = true
        formatter.maximumFractionDigits = 1
        formatter.minimum = 0
        formatter.maximum = 100
        field.formatter = formatter
        field.delegate = self
        field.alignment = .right
        DSField.style(field)
        field.widthAnchor.constraint(equalToConstant: 62).isActive = true
    }

    private func percentRow(_ field: NSTextField) -> NSView {
        let suffix = NSTextField(labelWithString: "%")
        suffix.font = DS.sans(13)
        suffix.textColor = DS.textMuted
        let row = NSStackView(views: [field, suffix])
        row.orientation = .horizontal
        row.alignment = .centerY
        row.spacing = 6
        return row
    }

    // MARK: - Presets and params

    /// A preset's stops in the DOCUMENT's numbers. The tables are sRGB —
    /// `#000000` and `#ffffff` are sRGB spellings, and the paint colours are
    /// stored as sRGB by `TextLayer.hex` — so each converts ONCE here, at
    /// the moment it is picked, and what gets stored is the document's own
    /// spelling like every other adjustment colour. nil for "Custom".
    private static func presetStops(_ index: Int, in space: NSColorSpace) -> [GradientStop]? {
        let shared = ToolOptionsStore.shared.sharedState
        let pair: (String, String)
        switch index {
        case 1: pair = ("#000000", "#ffffff")
        case 2: pair = ("#ffffff", "#000000")
        // The paint pair is spelled "#RRGGBBAA"; the alpha is dropped, since
        // a stop's own opacity means something else here (how much of the
        // original colour survives) and the paint swatch's does not.
        case 3: pair = (shared.foreground, shared.background)
        default: return nil
        }
        return [
            GradientStop(
                position: 0, color: AdjustmentColor.documentHex(forSRGBHex: pair.0, in: space),
                opacity: 1),
            GradientStop(
                position: 1, color: AdjustmentColor.documentHex(forSRGBHex: pair.1, in: space),
                opacity: 1),
        ]
    }

    /// The ramp a Gradient Map's params describe, defensively: a missing or
    /// malformed `gradient`, or one with fewer than two stops, falls back to
    /// the schema's black→white default rather than refusing to open — the
    /// contract every `AdjustmentLayerPayload` reader keeps.
    private static func fill(from payload: AdjustmentLayerPayload) -> GradientFill {
        var out = GradientFill()
        guard let object = payload.object("gradient") else { return out }
        let classic = namesMethod(payload)
        if let reverse = object["reverse"] as? NSNumber { out.reverse = reverse.boolValue }
        guard let list = object["stops"] as? [Any] else { return out }
        var stops: [GradientStop] = []
        for entry in list {
            guard let stop = entry as? [String: Any] else { continue }
            let position = (stop["position"] as? NSNumber)?.doubleValue ?? 0
            let opacity = (stop["opacity"] as? NSNumber)?.doubleValue ?? 1
            stops.append(
                GradientStop(
                    position: position.isFinite ? min(max(position, 0), 1) : 0,
                    color: normalizedHex(stop["color"]),
                    opacity: opacity.isFinite ? min(max(opacity, 0), 1) : 1,
                    midpoint: midpoint(stop, "midpoint", classic: classic),
                    opacityMidpoint: midpoint(stop, "opacity_midpoint", classic: classic)))
        }
        if stops.count >= GradientEditorView.minStops { out.stops = stops }
        return out
    }

    /// A stored `"#rrggbb"`, or black for anything that is not six hex
    /// digits behind a `#` — the spelling check `AdjustmentLayerPayload`
    /// does, and nothing more: an adjustment's colour converts nowhere.
    private static func normalizedHex(_ value: Any?) -> String {
        guard let text = value as? String, text.count == 7, text.hasPrefix("#"),
              text.dropFirst().allSatisfy({ $0.isHexDigit })
        else { return "#000000" }
        return text.lowercased()
    }
}

extension GradientFill {
    /// The `gradient` object a `gradient_map`'s params carry: `stops` and
    /// `reverse`, a Classic map's `smoothness` when not 1 and each stop's
    /// midpoints when not 50 %, and nothing else.
    ///
    /// The core takes the SAME object a layer style's gradient is written
    /// as, through the same parser — but a map has no geometry, so `style`,
    /// `angle`, `scale` and `align_with_layer` are accepted and ignored, and
    /// `AdjustmentSchema.defaultGradient` writes only `stops` and `reverse`.
    /// Writing the rest would lengthen a meta that travels in `.rz` and in
    /// every `get_document` reply, and would make re-committing an unchanged
    /// gradient a fresh undo step — the sheet would spell the same picture
    /// differently from the way it read it.
    var mapParams: [String: Any] {
        var out: [String: Any] = [
            "stops": stops.map { stop -> [String: Any] in
                var out: [String: Any] = [
                    "position": stop.position, "color": stop.color, "opacity": stop.opacity,
                ]
                if let m = stop.midpoint { out["midpoint"] = m }
                if let m = stop.opacityMidpoint { out["opacity_midpoint"] = m }
                return out
            },
            "reverse": reverse,
        ]
        if let smoothness = smoothness { out["smoothness"] = smoothness }
        return out
    }
}
