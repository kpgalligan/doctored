import AppKit

/// Filters ▸ Develop Filter…: the Develop panel's global controls on an ordinary layer
/// or plane at 8 and 16 bits, through one core kernel (`dr_image_filter_op` "develop_filter" at 8 bits,
/// the selected-filter request of the same name at 16; `sampling_develop_filter.rs` states the fixed
/// stage order and every formula), so the sheet, its recorded Action step, Repeat Last Filter and
/// `apply_filter` agree, as `PresenceSheet.swift`'s do. Destructive, cancellable and NOT
/// re-editable: running it again starts from the current pixels. It takes no frame, so a selection
/// confines the result by coverage. All thirteen at 0 is the core's exact bypass: OK then records
/// no undo step. In a 32-bit float document Temperature, Tint and Exposure act on the
/// stored linear values, Contrast, Highlights, Shadows, Whites, Blacks, Texture, Clarity and
/// Dehaze on the layer's display encoding, and Vibrance and Saturation on the decoded linear value
/// scaled about white (`sampling_develop_filter.rs`'s `# 32 bits` table states each rule).
///
/// The sheet is a `SliderSheetController` with no sliders of its own and one accessory,
/// `DevelopFilterPanel`, built from Develop's row widgets (`cardSliderRow` /
/// `CardControlRow.setValue` in `Controls.swift`). Develop's RAW-specific views are NOT reused:
/// `RawDevelopPreviewPanel` / `RawDevelopCanvasView` preview the camera file in source
/// coordinates (this filter previews on the canvas like every filter sheet), the knob table
/// (`RawDevelopValues`) and `RawEnhancementControls` are bound to a decoder recipe, and Develop's
/// preset popup writes `RawSettingsPatch`. Develop controls left out, with no core equivalent
/// outside the decoder: Tone curve, shadow boost, Local contrast, Sharpness, Detail, Luminance and
/// Colour noise, Profile corrections, Highlight recovery, Moiré and Baseline compensation
/// (CIRAWFilter camera-space processing); output space and decoder (RAW-only); Vignette and Grain
/// (Filters ▸ Lens Correction… and Add Grain…).
///
/// Every global value is scale-free — the kernel's radii are a share of the target's longer side —
/// so the preview's proxy takes them unchanged. Two mask controls are not: a mask's Sharpness and
/// Noise Reduction run at radii fixed in target pixels (`local_detail.rs`), which a proxy of k×
/// fewer pixels a side would show k times wider. The proxy map appends a marker value, and the
/// params closure then asks the store for the masks without those two keys
/// (`DevelopFilterMaskStore.params(_:revision:proxy:)`): the proxy shows every other mask change and
/// the exact preview adds the detail. Steps, Apply and Actions never see the marker.
///
/// Masks: the panel embeds `DevelopFilterMaskPanel`, ordered local adjustments over the
/// globals (radial, linear, brush, range, subject, people and sky, each with the twelve local
/// controls, Sharpness and Noise Reduction; `develop_filter_masks.rs` states the order). `values` are
/// the thirteen globals plus the masks' revision (plus the proxy's marker), and the params closure
/// adds that revision's resolved masks and, with a range, the document's `range_profile` from the
/// sheet's `DevelopFilterMaskStore`, so the worker-side preview never reads panel state. Reset clears
/// the masks; presets keep the thirteen globals only. A recorded step keeps a generated mask as
/// the shape it asked for (`DevelopFilterMasks.recorded`), so a replay runs the model again.
extension SliderSheetController {
    static func developFilter(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let masks = DevelopFilterMaskPanel(document: document)
        let store = masks.store
        let count = DevelopFilterControls.keys.count
        let params: ([Double]) -> [String: Any] = { values in
            let globals = DevelopFilterControls(values: Dictionary(
                uniqueKeysWithValues: zip(DevelopFilterControls.keys, values))).params
            let revision = values.count > count ? Int(values[count]) : 0
            return store.params(globals, revision: revision, proxy: values.count > count + 1)
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Develop Filter", actionName: "Develop Filter",
            sliders: [],
            accessories: [DevelopFilterPanel(masks: masks)],
            step: { values in .filter("develop_filter", params(values), target: target) },
            proxy: { values, _ in values + [1] },
            request: { ByteFilterRequest(op: "develop_filter", params: params($0)) },
            compute: { image, values, control in
                do {
                    return try image.filterOp("develop_filter", params: params(values), control: control)
                        ?? image
                } catch { return nil }
            })
    }
}

extension EditorViewController {
    @objc func showDevelopFilter(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.developFilter(document: document, canvas: canvas))
    }
}

/// The sheet's one accessory: the preset row, the not-re-editable note, the thirteen controls in
/// Develop-style groups (White Balance, Tone, Presence) and Reset, beside the Masks section. Its
/// `values` are the thirteen in `DevelopFilterControls.keys` order, then the masks' revision.
final class DevelopFilterPanel: NSObject, FilterSheetAccessory {
    let view: NSView
    var onChange: (() -> Void)? {
        didSet { masks.onChange = onChange }
    }
    private let stack: NSStackView
    private let masks: DevelopFilterMaskPanel
    private let status = NSTextField(wrappingLabelWithString: "")
    private var rows: [String: CardControlRow] = [:]
    private var controls = DevelopFilterControls()
    private var presetControl: DevelopFilterPresetControl?

    private static let groups: [(String, [(String, String)])] = [
        ("White Balance", [("temperature", "Temp:"), ("tint", "Tint:")]),
        ("Tone", [("exposure", "Exposure:"), ("contrast", "Contrast:"), ("highlights", "Highlights:"),
                  ("shadows", "Shadows:"), ("whites", "Whites:"), ("blacks", "Blacks:")]),
        ("Presence", [("texture", "Texture:"), ("clarity", "Clarity:"), ("dehaze", "Dehaze:"),
                      ("vibrance", "Vibrance:"), ("saturation", "Saturation:")]),
    ]

    init(masks: DevelopFilterMaskPanel) {
        let stack = NSStackView()
        self.stack = stack
        self.masks = masks
        let columns = NSStackView(views: [stack, masks.view])
        columns.orientation = .horizontal
        columns.alignment = .top
        columns.spacing = 20
        view = columns
        super.init()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        let presets = DevelopFilterPresetControl(
            current: { [weak self] in self?.controls ?? DevelopFilterControls() },
            load: { [weak self] in self?.presetChosen($0, status: $1) })
        presetControl = presets
        stack.addArrangedSubview(presets.row)
        let note = NSTextField(
            wrappingLabelWithString: "Not re-editable: running the filter again starts from the current pixels.")
        note.font = DS.sans(11)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = 380
        stack.addArrangedSubview(note)
        for (title, members) in Self.groups {
            let heading = NSTextField(labelWithString: title)
            heading.font = DS.sans(12, weight: .semibold)
            heading.textColor = DS.textMuted
            stack.addArrangedSubview(heading)
            for (key, label) in members {
                let range = DevelopFilterControls.ranges[key] ?? -100.0...100.0
                let row = cardSliderRow(
                    label: label, min: range.lowerBound, max: range.upperBound, value: 0,
                    format: Self.format(key)
                ) { [weak self] value in
                    self?.slid(key, value)
                }
                row.setAccessibilityIdentifier("develop-filter-\(key)")
                rows[key] = row
                stack.addArrangedSubview(row)
            }
        }
        status.font = DS.sans(11)
        status.textColor = DS.textMuted
        status.preferredMaxLayoutWidth = 380
        status.isHidden = true
        stack.addArrangedSubview(status)
        let reset = StickerButton(title: "Reset", style: .secondary, target: self, action: #selector(resetClicked(_:)))
        stack.addArrangedSubview(reset)
    }

    var values: [Double] { DevelopFilterControls.keys.map { controls.value($0) } + masks.values }

    /// Sets every control to `controls`' value (0 where it has none), silently, then previews
    /// once.
    func load(_ controls: DevelopFilterControls) {
        self.controls = controls
        for key in DevelopFilterControls.keys { rows[key]?.setValue(controls.value(key)) }
        onChange?()
    }

    private static func format(_ key: String) -> (Double) -> String {
        key == "exposure"
            ? { String(format: "%+.2f EV", DevelopFilterControls.recorded(key, $0)) }
            : { String(format: "%+.0f", $0.rounded()) }
    }

    private func slid(_ key: String, _ value: Double) {
        let recorded = DevelopFilterControls.recorded(key, value)
        guard recorded != controls.value(key) else { return }
        controls.values[key] = recorded
        status.isHidden = true
        presetControl?.markCustom()
        onChange?()
    }

    private func presetChosen(_ controls: DevelopFilterControls, status message: String?) {
        status.stringValue = message ?? ""
        status.isHidden = message == nil
        load(controls)
    }

    @objc private func resetClicked(_ sender: Any?) {
        status.isHidden = true
        masks.reset()
        load(DevelopFilterControls())
        presetControl?.selectDefault()
    }
}
