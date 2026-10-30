import AppKit

/// The stop editor two dialogs share: `GradientEditorView`'s ramp and
/// stop bar, with its midpoint diamonds, over the selected stop's
/// Position, Opacity and Colour and the selected diamond's Midpoint. The
/// Gradient tool's stops sheet (`GradientStopsSheetController`) and the
/// Gradient fill-layer dialog (`FillLayerSheetController`) both hold one, so
/// a ramp is edited the same way in both.
///
/// Stop colours here are AUTHORED sRGB (`GradientEditorView`'s colour rule):
/// the tool converts them into the document when it paints, and a fill layer
/// stores them authored and the core converts them when it renders. The
/// Gradient Map dialog keeps its own controls: its stops are the document's
/// own numbers.
///
/// One value in (`fill`), one closure out (`onChange`, fired for an edit made
/// here and never for an assignment from outside).
final class GradientStopsPane: NSView, NSTextFieldDelegate {
    private let editor = GradientEditorView(frame: NSRect(x: 0, y: 0, width: 376, height: 72))
    private let positionField = NSTextField(string: "")
    private let opacityField = NSTextField(string: "")
    /// The selected diamond's midpoint, 5…95 % of its segment; disabled with
    /// no diamond selected.
    private let midpointField = NSTextField(string: "")
    private let colorWell = NSColorWell()
    /// True while the controls are refilled from the model, so the writes
    /// that causes (a colour well sends its action when its colour is set)
    /// are not read back as edits.
    private var isSyncing = false

    /// Fires for a stop added, moved, removed, recoloured or faded here, or a midpoint moved.
    var onChange: ((GradientFill) -> Void)?

    var fill: GradientFill {
        get { editor.fill }
        set {
            editor.fill = newValue
            syncStopControls()
        }
    }

    init(fill: GradientFill, width: CGFloat = sheetWidth - sheetInset * 2) {
        super.init(frame: .zero)
        editor.colorSpace = .sRGB
        editor.showsMidpoints = true
        editor.fill = fill
        editor.selectedStop = 0
        editor.onFillChanged = { [weak self] _ in self?.editorChanged() }
        editor.onSelectionChanged = { [weak self] _ in self?.syncStopControls() }
        editor.onEditColor = { [weak self] _ in self?.colorWell.activate(true) }
        editor.onMidpointSelectionChanged = { [weak self] _ in self?.syncStopControls() }
        editor.translatesAutoresizingMaskIntoConstraints = false
        for field in [positionField, opacityField, midpointField] { styleField(field) }
        if let formatter = midpointField.formatter as? NumberFormatter {
            formatter.minimum = 5
            formatter.maximum = 95
            formatter.maximumFractionDigits = 0
        }
        positionField.identifier = NSUserInterfaceItemIdentifier("gradient-stop-position")
        opacityField.identifier = NSUserInterfaceItemIdentifier("gradient-stop-opacity")
        midpointField.identifier = NSUserInterfaceItemIdentifier("gradient-midpoint-location")
        colorWell.target = self
        colorWell.action = #selector(colorPicked(_:))
        colorWell.identifier = NSUserInterfaceItemIdentifier("gradient-stop-color")
        colorWell.translatesAutoresizingMaskIntoConstraints = false

        let grid = NSGridView(views: [
            [fieldLabel("Position:"), percentRow(positionField)],
            [fieldLabel("Opacity:"), percentRow(opacityField)],
            [fieldLabel("Color:"), colorWell],
            [fieldLabel("Midpoint:"), percentRow(midpointField)],
        ])
        grid.rowSpacing = 8
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let stack = NSStackView(views: [editor, grid])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            editor.widthAnchor.constraint(equalToConstant: width),
            editor.heightAnchor.constraint(equalToConstant: 72),
            colorWell.widthAnchor.constraint(equalToConstant: 44),
            colorWell.heightAnchor.constraint(equalToConstant: 24),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        syncStopControls()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("GradientStopsPane does not support NSCoder")
    }

    // MARK: - Editing

    private func editorChanged() {
        syncStopControls()
        onChange?(editor.fill)
    }

    @objc private func colorPicked(_ sender: NSColorWell) {
        guard !isSyncing, selectedStop() != nil else { return }
        editor.setColor(LayerStyleColor.hex(sender.color), forStop: editor.selectedStop)
        onChange?(editor.fill)
    }

    /// The two fields edit live: every keystroke moves or fades the stop, and
    /// the field being typed in is never rewritten under the caret. An empty
    /// field is a keystroke on the way to a number.
    func controlTextDidChange(_ obj: Notification) {
        guard !isSyncing, let field = obj.object as? NSTextField,
              !field.stringValue.trimmingCharacters(in: .whitespaces).isEmpty
        else { return }
        // The Midpoint field edits the selected DIAMOND, never whichever
        // stop happens to be selected.
        if field === midpointField {
            guard let midpoint = editor.selectedMidpoint else { return }
            editor.setMidpoint(min(max(field.doubleValue, 5), 95) / 100, for: midpoint)
            syncStopControls(except: field)
            onChange?(editor.fill)
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
        onChange?(editor.fill)
    }

    func controlTextDidEndEditing(_ obj: Notification) {
        syncStopControls()
    }

    // MARK: - Keeping the controls and the ramp in step

    private func selectedStop() -> GradientStop? {
        let stops = editor.fill.stops
        let index = editor.selectedStop
        return stops.indices.contains(index) ? stops[index] : nil
    }

    private func syncStopControls(except editing: NSTextField? = nil) {
        isSyncing = true
        defer { isSyncing = false }
        syncMidpointField(except: editing)
        guard let stop = selectedStop() else {
            for field in [positionField, opacityField] {
                field.stringValue = ""
                field.isEnabled = false
            }
            colorWell.isEnabled = false
            return
        }
        for field in [positionField, opacityField] { field.isEnabled = true }
        colorWell.isEnabled = true
        if positionField !== editing { positionField.doubleValue = stop.position * 100 }
        if opacityField !== editing { opacityField.doubleValue = stop.opacity * 100 }
        colorWell.color = LayerStyleColor.color(fromHex: stop.color) ?? .black
    }

    /// The Midpoint field follows the selected diamond: cleared and disabled
    /// without one.
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

    // MARK: - Layout pieces

    /// A percentage field, 0…100 with one decimal — the Gradient Map
    /// dialog's.
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
}
