import AppKit
import UniformTypeIdentifiers

/// Image ▸ Mode ▸ Duotone… (#547 Duotone phase 1a): Type (Monotone, Duotone, Tritone, Quadtone),
/// and per ink a name, its D50 L*, a*, b* with a swatch drawn through a D50 Lab colour space, and
/// Curve… (`DuotoneCurveSheetController`); then Overprint Colors… for two or more inks
/// (`DuotoneOverprintSheetController`). Apply asks the core first (`refusal`: the options
/// normalized, the plan's refusal): a refusal beeps and shows its sentence here; the edit and its
/// one undo step are `EditorViewController+DuotoneMode.swift`'s. Since phase
/// 1b Load… and Save… read and write a Duotone Options (`.ado`) file through the core
/// (`readDuotoneADO`, `writeDuotoneADO`; a failure beeps and shows its sentence in the note), and a
/// L*, a* or b* field that is not a number beeps, takes focus with its text selected and keeps the
/// sheet open (the Curve sheet's rule). Since phase 2a the Preview box (on at open) shows the canvas
/// through the sheet's inks (`DuotonePreview`), refreshed after every Type, field, curve, overprint
/// and Load change and ended before Apply's and Cancel's dismiss; a sheet opened without a preview
/// disables the box. Colour libraries are not here.
final class DuotoneSheetController: NSViewController {
    /// The options last applied this session: the next first-use sheet opens at them.
    static var lastOptions: DuotoneOptions?

    /// Inks a larger Type starts from when the user has not entered them yet.
    private static let defaultInks: [DuotoneInk] = [
        DuotoneInk(name: "Black", lab: [0, 0, 0], curve: DuotoneInk.straightCurve),
        DuotoneInk(name: "Ink 2", lab: [55, 45, 55], curve: DuotoneInk.straightCurve),
        DuotoneInk(name: "Ink 3", lab: [80, -25, 70], curve: DuotoneInk.straightCurve),
        DuotoneInk(name: "Ink 4", lab: [50, -35, -20], curve: DuotoneInk.straightCurve),
    ]

    /// One ink row's controls.
    private struct InkRow {
        let label: NSTextField
        let name: NSTextField
        let lab: [NSTextField]
        let swatch: NSView
        let curve: NSButton
        var views: [NSView] { [label, name] + lab + [swatch, curve] }
    }

    private let refusal: (DuotoneOptions) -> String?
    private let commit: (DuotoneOptions) -> Void
    /// All four inks: the first `count` are the description, the rest kept for a larger Type.
    private var inks: [DuotoneInk]
    private var count: Int
    private var overprints: [String: [Double]]
    private let typePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private var rows: [InkRow] = []
    private var overprintButton: NSButton!
    private let note = NSTextField(wrappingLabelWithString: "")
    private let preview: DuotonePreview?
    private var previewBox: NSButton!

    init(options: DuotoneOptions, refusal: @escaping (DuotoneOptions) -> String?,
         commit: @escaping (DuotoneOptions) -> Void, preview: DuotonePreview?) {
        self.refusal = refusal
        self.commit = commit
        self.preview = preview
        count = max(1, min(4, options.inks.count))
        inks = options.inks + Self.defaultInks.dropFirst(options.inks.count)
        overprints = options.overprints
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("DuotoneSheetController does not support NSCoder")
    }

    /// The D50 Lab space the swatches are drawn in.
    static let labSpace = CGColorSpace(labWhitePoint: [0.9642, 1.0, 0.8249], blackPoint: [0, 0, 0],
                                       range: [-128, 127, -128, 127])

    /// `lab` as a colour for a swatch layer (sRGB, relative colorimetric); nil if it cannot convert.
    static func swatchColor(_ lab: [Double]) -> CGColor? {
        guard lab.count == 3, let space = labSpace,
              let srgb = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        let components = lab.map { CGFloat($0) } + [1]
        return CGColor(colorSpace: space, components: components)?
            .converted(to: srgb, intent: .relativeColorimetric, options: nil)
    }

    static func numberField(_ value: Double, width: CGFloat = 52) -> NSTextField {
        let field = NSTextField(string: format(value))
        field.font = DS.sans(13)
        field.alignment = .right
        field.widthAnchor.constraint(equalToConstant: width).isActive = true
        return field
    }

    static func format(_ value: Double) -> String {
        value.rounded() == value ? String(Int(value)) : String(format: "%.2f", value)
    }

    static func swatch() -> NSView {
        let view = NSView()
        view.wantsLayer = true
        view.layer?.cornerRadius = 3
        view.layer?.borderWidth = 1
        view.layer?.borderColor = DS.textFaint.cgColor
        view.widthAnchor.constraint(equalToConstant: 22).isActive = true
        view.heightAnchor.constraint(equalToConstant: 22).isActive = true
        return view
    }

    override func loadView() {
        typePopup.addItems(withTitles: DuotoneOptions.typeTitles)
        typePopup.font = DS.sans(13)
        typePopup.target = self
        typePopup.action = #selector(typeChanged(_:))
        typePopup.selectItem(at: count - 1)
        typePopup.setAccessibilityIdentifier("duotone-type")
        var gridRows: [[NSView]] = [[fieldLabel("Type:"), typePopup]]
        for index in 0..<4 {
            let ink = inks[index]
            let name = NSTextField(string: ink.name)
            name.font = DS.sans(13)
            name.widthAnchor.constraint(equalToConstant: 96).isActive = true
            name.setAccessibilityIdentifier("duotone-ink\(index + 1)-name")
            let lab = ink.lab.map { Self.numberField($0) }
            for (field, axis) in zip(lab, ["l", "a", "b"]) {
                field.setAccessibilityIdentifier("duotone-ink\(index + 1)-\(axis)")
            }
            for field in [name] + lab {
                field.target = self
                field.action = #selector(fieldsChanged(_:))
            }
            let curve = NSButton(title: "Curve…", target: self, action: #selector(curveClicked(_:)))
            curve.tag = index
            curve.setAccessibilityIdentifier("duotone-ink\(index + 1)-curve")
            let row = InkRow(label: fieldLabel("Ink \(index + 1):"), name: name, lab: lab,
                             swatch: Self.swatch(), curve: curve)
            rows.append(row)
            let fields = NSStackView(views: [row.name] + row.lab + [row.swatch, row.curve])
            fields.spacing = 6
            gridRows.append([row.label, fields])
        }
        let grid = NSGridView(views: gridRows)
        grid.rowSpacing = 10
        grid.columnSpacing = 10
        grid.column(at: 0).xPlacement = .trailing
        let axes = NSTextField(labelWithString: "Name, then L*, a*, b* (D50)")
        axes.font = DS.sans(11)
        axes.textColor = DS.textMuted
        overprintButton = NSButton(title: "Overprint Colors…", target: self,
                                   action: #selector(overprintClicked(_:)))
        overprintButton.setAccessibilityIdentifier("duotone-overprints")
        let load = NSButton(title: "Load…", target: self, action: #selector(loadClicked(_:)))
        let save = NSButton(title: "Save…", target: self, action: #selector(saveClicked(_:)))
        load.setAccessibilityIdentifier("duotone-load")
        save.setAccessibilityIdentifier("duotone-save")
        previewBox = NSButton(checkboxWithTitle: "Preview", target: self,
                              action: #selector(previewToggled(_:)))
        previewBox.font = DS.sans(13)
        previewBox.state = preview == nil ? .off : .on
        previewBox.isEnabled = preview != nil
        previewBox.setAccessibilityIdentifier("duotone-preview")
        let buttons = NSStackView(views: [overprintButton, load, save, previewBox])
        buttons.spacing = 8
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = 520 - sheetInset * 2
        note.setAccessibilityIdentifier("duotone-note")
        let content = NSStackView(views: [axes, grid, buttons, note])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        cancelButton.setAccessibilityIdentifier("duotone-cancel")
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("duotone-apply")
        syncControls()
        view = makeSheetView(
            title: "Duotone Options",
            hint: "Prints the gray channel through one to four inks: each "
                + "ink's curve maps the gray to its coverage, and the inks overprint on paper. The "
                + "pixels stay the Grayscale document's.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton,
                                     leading: [sheetFootnote("one undo step")]),
            width: 520)
        refreshPreview()
    }

    /// The canvas through the sheet's current inks while Preview is on; cleared when it is off.
    private func refreshPreview() {
        guard let preview else { return }
        preview.request(previewBox.state == .on ? options : nil)
    }

    @objc private func previewToggled(_ sender: Any?) {
        // A field that does not parse keeps the box off: no preview is drawn until it does.
        if previewBox.state == .on { guard readFields() else { previewBox.state = .off; return } }
        refreshPreview()
    }

    /// The current options: the first `count` inks as the fields hold them, and the authored
    /// overprints that still name only those inks.
    private var options: DuotoneOptions {
        let keys = Set(DuotoneOptions.overprintKeys(count: count))
        return DuotoneOptions(inks: Array(inks.prefix(count)),
                              overprints: overprints.filter { keys.contains($0.key) })
    }

    /// The fields into `inks`; false (after a beep, with the field focused and its text selected)
    /// when an L*, a* or b* field is not a number, so the sheet stays open as the Curve sheet does.
    @discardableResult
    private func readFields() -> Bool {
        for (index, row) in rows.enumerated() {
            var lab: [Double] = []
            for field in row.lab {
                let text = field.stringValue.trimmingCharacters(in: .whitespaces)
                guard let value = Double(text), value.isFinite else {
                    NSSound.beep()
                    view.window?.makeFirstResponder(field)
                    field.selectText(nil)
                    return false
                }
                lab.append(value)
            }
            inks[index].name = row.name.stringValue
            inks[index].lab = lab
        }
        return true
    }

    /// The rows' fields from `inks` (after a Load).
    private func writeFields() {
        for (index, row) in rows.enumerated() {
            row.name.stringValue = inks[index].name
            for (field, value) in zip(row.lab, inks[index].lab) {
                field.stringValue = Self.format(value)
            }
        }
        typePopup.selectItem(at: count - 1)
    }

    private func syncControls() {
        for (index, row) in rows.enumerated() {
            let shown = index < count
            row.views.forEach { $0.isHidden = !shown }
            row.swatch.layer?.backgroundColor = Self.swatchColor(inks[index].lab)
        }
        overprintButton.isEnabled = count >= 2
    }

    @objc private func typeChanged(_ sender: Any?) {
        guard readFields() else { typePopup.selectItem(at: count - 1); return }
        count = max(1, min(4, typePopup.indexOfSelectedItem + 1))
        syncControls()
        refreshPreview()
    }

    @objc private func fieldsChanged(_ sender: Any?) {
        guard readFields() else { return }
        syncControls()
        refreshPreview()
    }

    @objc private func loadClicked(_ sender: Any?) {
        guard let window = view.window else { NSSound.beep(); return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "ado") ?? .data]
        panel.allowsMultipleSelection = false
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            self?.load(url)
        }
    }

    /// The `.ado` at `url` into the sheet: its inks, Type and authored overprints.
    private func load(_ url: URL) {
        do {
            let loaded = try DuotoneOptions(json: RasterPrecisionDocument.readDuotoneADO(at: url))
            count = max(1, min(4, loaded.inks.count))
            inks = loaded.inks + Self.defaultInks.dropFirst(loaded.inks.count)
            overprints = loaded.overprints
            writeFields()
            syncControls()
            refreshPreview()
            note.stringValue = "Loaded \(url.lastPathComponent)."
        } catch {
            NSSound.beep()
            note.stringValue = error.localizedDescription
        }
    }

    @objc private func saveClicked(_ sender: Any?) {
        guard readFields(), let window = view.window else { return }
        let options = options
        let panel = NSSavePanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "ado") ?? .data]
        panel.nameFieldStringValue = "\(options.typeTitle).ado"
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            do {
                try RasterPrecisionDocument.writeDuotoneADO(options, to: url)
                self?.note.stringValue = "Saved \(url.lastPathComponent)."
            } catch {
                NSSound.beep()
                self?.note.stringValue = error.localizedDescription
            }
        }
    }

    @objc private func curveClicked(_ sender: NSButton) {
        guard readFields() else { return }
        let index = sender.tag
        guard inks.indices.contains(index) else { return }
        presentAsSheet(DuotoneCurveSheetController(
            inkName: inks[index].name, curve: inks[index].curve,
            commit: { [weak self] in self?.curveChosen($0, ink: index) }))
    }

    private func curveChosen(_ curve: [Double?], ink index: Int) {
        guard inks.indices.contains(index) else { return }
        inks[index].curve = curve
        refreshPreview()
    }

    @objc private func overprintClicked(_ sender: Any?) {
        guard readFields() else { return }
        guard count >= 2 else { NSSound.beep(); return }
        let options = options
        let shown = (try? RasterPrecisionDocument.normalizeDuotone(options.json))?["overprint_display"]
            as? [String: Any] ?? [:]
        presentAsSheet(DuotoneOverprintSheetController(
            keys: DuotoneOptions.overprintKeys(count: count), authored: options.overprints,
            effective: shown.compactMapValues { ($0 as? [NSNumber])?.map(\.doubleValue) },
            commit: { [weak self] in self?.overprintsChosen($0) }))
    }

    private func overprintsChosen(_ authored: [String: [Double]]) {
        overprints = authored
        refreshPreview()
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard readFields() else { return }
        let options = options
        if let reason = refusal(options) {
            NSSound.beep()
            note.stringValue = reason
            return
        }
        preview?.end()
        dismiss(self)
        commit(options)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        preview?.end()
        dismiss(self)
    }
}
