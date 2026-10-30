import AppKit

/// Local settings are ordinary numeric commands; coverage never leaves the
/// native mask owner except as an explicitly disposable red display overlay.
final class RawLocalControls: NSObject, NSTextFieldDelegate {
    let view = NSStackView()
    private(set) var selectedID: String?
    private(set) var state: RawLocalState
    var onOperation: (([String: Any], Int) throws -> Void)?
    var onSelection: (() -> Void)?
    var onTool: ((RawLocalGestureView.Tool) -> Void)?
    var onBrush: ((Double, Double, Double) -> Void)?
    var onError: ((String) -> Void)?
    private let list = NSPopUpButton()
    private let name = NSTextField()
    private let overlay = NSButton(checkboxWithTitle: "Show effective mask overlay", target: nil, action: nil)
    private var nodeRows: [CardControlRow] = []
    private let rangeRows = RawLocalRangeControls()
    private let toneRows = RawLocalToneControls()
    /// False at the 32-bit float (linear) encoding: a local tone refuses there
    /// (`RawLocalTone.floatRefusal`), so its rows are disabled, as the global Presence rows are.
    var toneAvailable = true { didSet { setReady(ready) } }
    private var toolRow: CardControlRow?
    private var buttons: [NSButton] = []
    private var radius: Double = 24
    private var hardness: Double = 0.7
    private var flow: Double = 1
    private var newGradient = false
    private var ready = false
    private static var copiedSettings: [String: Any]?

    var showsOverlay: Bool { overlay.state == .on }
    var selectedNode: RawLocalNode? { state.nodes.first { $0.id == selectedID } }
    var canDraw: Bool { selectedNode != nil || newGradient }

    func clear() {
        onOperation = nil; onSelection = nil; onTool = nil; onBrush = nil; onError = nil
        state = RawLocalState(descriptor: state.descriptor, width: state.width, height: state.height,
            nodes: [], masks: [])
        selectedID = nil; newGradient = false
        rangeRows.selectionChanged()
        setReady(false)
    }

    init(state: RawLocalState) {
        self.state = state
        selectedID = state.nodes.first?.id
        super.init()
        view.orientation = .vertical; view.alignment = .leading; view.spacing = 10
        let note = NSTextField(wrappingLabelWithString:
            "Canonical masks use the full oriented source before explicit lens geometry and layer transforms. Drags edit these coordinates; Global displays the final optically mapped result.")
        note.font = DS.sans(11); note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = 370
        note.widthAnchor.constraint(equalToConstant: 370).isActive = true
        view.addArrangedSubview(note)
        list.widthAnchor.constraint(equalToConstant: 370).isActive = true
        list.target = self; list.action = #selector(selected(_:))
        list.setAccessibilityIdentifier("raw-local-list")
        view.addArrangedSubview(list)
        let creation = NSStackView(views: [button("New Radial", "new-radial", #selector(newRadial(_:))),
            button("New Linear", "new-linear", #selector(newLinear(_:))),
            button("New Empty", "new-empty", #selector(newEmpty(_:)))])
        creation.spacing = 8
        view.addArrangedSubview(creation)
        // A whole-frame local limited by a luminance or colour range instead of a drawn shape.
        view.addArrangedSubview(NSStackView(views: [button("New Range", "new-range", #selector(newRange(_:)))]))
        // A local whose coverage a model generates from the global development (`RawLocalGenerated`).
        let generation = NSStackView(views: [button("New Subject", "new-subject", #selector(newSubject(_:))),
            button("New People", "new-people", #selector(newPeople(_:))),
            button("New Sky", "new-sky", #selector(newSky(_:)))])
        generation.spacing = 8
        view.addArrangedSubview(generation)
        name.placeholderString = "Local name"; name.delegate = self; DSField.style(name)
        name.widthAnchor.constraint(equalToConstant: 370).isActive = true
        name.setAccessibilityIdentifier("raw-local-name")
        view.addArrangedSubview(name)
        let enabled = cardCheckboxRow(label: "Enabled:", title: "Apply this local", on: true) { [weak self] _ in self?.changed() }
        nodeRows.append(enabled); view.addArrangedSubview(enabled)
        addNodeSlider("Density:", "density", 0...1, 1)
        addNodeSlider("Feather px:", "feather", 0...256, 0)
        addNodeSlider("Camera exposure EV:", "exposure", -4...4, 0)
        let white = cardPopupRow(label: "White balance:", titles: ["Inherit Global", "Camera As Shot", "Custom"],
            selected: 0) { [weak self] _ in self?.changed() }
        white.setAccessibilityIdentifier("raw-local-white-balance")
        nodeRows.append(white); view.addArrangedSubview(white)
        addNodeSlider("Temperature:", "temperature", RawDevelopRange.temperature, state.descriptor.recipe.values.temperature)
        addNodeSlider("Tint:", "tint", RawDevelopRange.tint, state.descriptor.recipe.values.tint)
        // The tone path's rows: applied to the development inside the mask, no camera variant.
        toneRows.onChange = { [weak self] in self?.changed() }
        view.addArrangedSubview(toneRows.view)
        rangeRows.onChange = { [weak self] in self?.changed() }
        rangeRows.onSampleTool = { [weak self] in self?.selectSampleTool() }
        view.addArrangedSubview(rangeRows.view)
        let tools = cardPopupRow(label: "Mask tool:",
            titles: ["Pan", "Brush Add", "Brush Subtract", "Replace Radial", "Replace Linear", "Sample Colour"],
            selected: 0) { [weak self] index in
                guard let self else { return }
                guard (0..<6).contains(index) else { return }
                self.newGradient = false
                self.onTool?([.pan, .add, .subtract, .radial, .linear, .sample][index])
            }
        tools.setAccessibilityIdentifier("raw-local-tool")
        toolRow = tools; view.addArrangedSubview(tools)
        addBrush("Radius px:", "radius", 0.5...1024, radius) { $0.radius = $1 }
        addBrush("Hardness:", "hardness", 0...1, hardness) { $0.hardness = $1 }
        addBrush("Flow:", "flow", 0...1, flow) { $0.flow = $1 }
        overlay.state = .on; overlay.target = self; overlay.action = #selector(overlayChanged(_:))
        overlay.font = DS.sans(12); overlay.setAccessibilityIdentifier("raw-local-overlay")
        view.addArrangedSubview(overlay)
        let actions = NSStackView(views: [button("Invert", "invert", #selector(invert(_:))),
            button("Remap…", "remap", #selector(remap(_:))),
            button("Earlier", "earlier", #selector(earlier(_:))),
            button("Later", "later", #selector(later(_:))), button("Remove", "remove", #selector(remove(_:)))])
        actions.spacing = 8; view.addArrangedSubview(actions)
        // The actions row's second line: a sixth button does not fit that row in the sidebar's width.
        view.addArrangedSubview(NSStackView(views: [button("Recompute", "recompute", #selector(recompute(_:)))]))
        let copy = NSStackView(views: [button("Copy Settings", "copy", #selector(copySettings(_:))),
            button("Paste Settings", "paste", #selector(pasteSettings(_:)))])
        copy.spacing = 8; view.addArrangedSubview(copy)
        update(state: state, ready: false)
    }

    func update(state: RawLocalState, ready: Bool) {
        self.state = state; self.ready = ready
        if let id = selectedID, !state.nodes.contains(where: { $0.id == id }) {
            selectedID = nil; rangeRows.selectionChanged()
        }
        list.removeAllItems()
        list.addItem(withTitle: "No local selected")
        list.addItems(withTitles: state.nodes.map { ($0.enabled ? "" : "Disabled · ") + $0.name })
        list.selectItem(at: state.nodes.firstIndex(where: { $0.id == selectedID }).map { $0 + 1 } ?? 0)
        fillControls()
        setReady(ready)
    }

    func setReady(_ value: Bool) {
        ready = value
        list.isEnabled = value; name.isEnabled = selectedNode != nil
        // Numeric controls can coalesce while a decoder finishes. Structural
        // changes and source gestures wait for the matching displayed result.
        for row in nodeRows { row.isEnabled = selectedNode != nil }
        let custom = nodeRows[4].popUp?.indexOfSelectedItem == 2
        nodeRows[5].isEnabled = selectedNode != nil && custom
        nodeRows[6].isEnabled = selectedNode != nil && custom
        rangeRows.setEnabled(selectedNode != nil)
        toneRows.setEnabled(selectedNode != nil && toneAvailable)
        toolRow?.isEnabled = value && selectedNode != nil
        for button in buttons {
            let identifier = button.accessibilityIdentifier() ?? ""
            let applies: Bool
            if identifier.contains("new-") {
                applies = state.nodes.count < RawEnhancementRecipe.maximumLocals
            } else if identifier.hasSuffix("recompute") {
                // Only a generated local has a request to run again.
                applies = selectedNode?.generated != nil
            } else { applies = selectedNode != nil }
            button.isEnabled = value && applies
        }
        overlay.isEnabled = value
    }

    func accept(_ command: [String: Any], dabs: Int) {
        guard ready else { return }
        do {
            if newGradient {
                try create(mask: command)
            } else {
                guard let selectedID else { throw DoctoredCoreError(message: "Select or create a local mask first.") }
                try onOperation?(["op": "mask", "id": selectedID, "command": command], dabs)
            }
        } catch { report(error) }
    }

    /// The Sample Colour click on the canvas, in oriented source pixels.
    func sampled(at point: CGPoint) {
        guard ready, selectedNode != nil else { return }
        rangeRows.sampled(at: point)
    }
    private func selectSampleTool() {
        newGradient = false; toolRow?.setSelected(5); onTool?(.sample)
    }

    private func fillControls() {
        rangeRows.fill(selectedNode?.range)
        toneRows.fill(selectedNode?.tone)
        guard let node = selectedNode else { name.stringValue = ""; return }
        name.stringValue = node.name
        nodeRows[0].setOn(node.enabled)
        for (row, value) in zip(nodeRows[1...3], [node.density, node.feather, node.exposureOffset]) { row.setValue(value) }
        nodeRows[4].setSelected(RawLocalWhiteBalance.allCases.firstIndex(of: node.whiteBalance) ?? 0)
        nodeRows[5].setValue(node.temperature ?? state.descriptor.recipe.values.temperature)
        nodeRows[6].setValue(node.tint ?? state.descriptor.recipe.values.tint)
    }
    private func controls() throws -> [String: Any] {
        guard let node = selectedNode, !name.stringValue.isEmpty, name.stringValue.utf8.count <= 128 else {
            throw DoctoredCoreError(message: "A local needs a name of 1…128 UTF-8 bytes.")
        }
        let whiteIndex = nodeRows[4].popUp?.indexOfSelectedItem ?? 0
        let white = RawLocalWhiteBalance.allCases[whiteIndex]
        var fields: [String: Any] = ["op": "update", "id": node.id, "name": name.stringValue,
            "enabled": nodeRows[0].checkbox?.state == .on,
            "density": nodeRows[1].slider?.doubleValue ?? node.density,
            "feather": nodeRows[2].slider?.doubleValue ?? node.feather,
            "exposure_offset": nodeRows[3].slider?.doubleValue ?? node.exposureOffset,
            "white_balance": white.rawValue, "range": rangeRows.rangeJSON]
        fields["tone"] = toneRows.toneJSON
        if white == .custom {
            fields["temperature"] = nodeRows[5].slider?.doubleValue ?? state.descriptor.recipe.values.temperature
            fields["tint"] = nodeRows[6].slider?.doubleValue ?? state.descriptor.recipe.values.tint
        }
        return fields
    }
    private func changed() {
        guard selectedNode != nil else { return }
        do { try onOperation?(controls(), 0) } catch { report(error) }
    }
    func controlTextDidEndEditing(_ notification: Notification) { changed() }
    func controlTextDidChange(_ notification: Notification) { changed() }
    @objc private func selected(_ sender: NSPopUpButton) {
        let index = sender.indexOfSelectedItem - 1
        selectedID = state.nodes.indices.contains(index) ? state.nodes[index].id : nil
        newGradient = false; toolRow?.setSelected(0); onTool?(.pan)
        rangeRows.selectionChanged()
        fillControls(); setReady(ready); onSelection?()
    }
    @objc private func overlayChanged(_ sender: Any?) { onSelection?() }
    private func beginGradient(_ tool: RawLocalGestureView.Tool) {
        guard ready, state.nodes.count < RawEnhancementRecipe.maximumLocals else { return }
        selectedID = nil; newGradient = true; list.selectItem(at: 0)
        rangeRows.selectionChanged()
        setReady(ready); onTool?(tool); onSelection?()
    }
    @objc private func newRadial(_ sender: Any?) { beginGradient(.radial) }
    @objc private func newLinear(_ sender: Any?) { beginGradient(.linear) }
    @objc private func newEmpty(_ sender: Any?) {
        guard ready else { return }
        do { try create(mask: ["op": "empty"]) } catch { report(error) }
    }
    @objc private func newRange(_ sender: Any?) {
        guard ready else { return }
        // Whole frame, every tone: the range rows then narrow it. The tool stays on Pan
        // because there is no shape to paint.
        let all = RawLocalRange(luminance: RawLuminanceRange(low: 0, high: 1))
        do { try create(mask: ["op": "full"], range: all.json) } catch { report(error) }
    }
    @objc private func newSubject(_ sender: Any?) { newGenerated(.subject) }
    @objc private func newPeople(_ sender: Any?) { newGenerated(.people) }
    /// The sky model must be in the store: without it nothing is created and the sentence offers
    /// Settings ▸ Models, as Select ▸ Sky does.
    @objc private func newSky(_ sender: Any?) {
        guard ready else { return }
        guard ModelGate.ready("sky") != nil else {
            ModelGate.offerSettings(ModelGate.notInstalled("sky"))
            return
        }
        newGenerated(.sky)
    }
    private func newGenerated(_ kind: RawLocalGeneratedKind) {
        guard ready else { return }
        // Vision reads the base development when the operation is evaluated. When it finds
        // nothing the session drops the operation again and the status line says so. A dropped
        // generation ends with no local selected: `create` moved `selectedID` to the new id
        // before the asynchronous evaluation, and that id is gone. The tool
        // stays on Pan: the mask is already there, and a brush refines it only when chosen.
        do {
            try create(mask: ["op": kind.rawValue], name: "\(kind.title) \(state.nodes.count + 1)",
                paints: false)
        } catch { report(error) }
    }
    /// Recompute: the selected generated local's request run again on the current global
    /// development. It replaces the whole plane; brush strokes and Invert since are not replayed.
    @objc private func recompute(_ sender: Any?) {
        guard ready, let node = selectedNode, let kind = node.generated else { return }
        do {
            try onOperation?(["op": "mask", "id": node.id, "command": ["op": kind.rawValue]], 0)
        } catch { report(error) }
    }
    private func create(mask: [String: Any], range: [String: Any]? = nil, name: String? = nil,
                        paints: Bool = true) throws {
        guard state.nodes.count < RawEnhancementRecipe.maximumLocals else {
            throw DoctoredCoreError(message: "This source already has sixteen local masks.")
        }
        let previous = selectedID, id = UUID().uuidString.lowercased()
        selectedID = id
        do {
            var operation: [String: Any] = ["op": "create", "id": id,
                "name": name ?? "Local \(state.nodes.count + 1)", "mask": mask]
            if let range { operation["range"] = range }
            try onOperation?(operation, 0)
            // The new local is selected now: a Colour tick still waiting for its first sample
            // belonged to the last one. A refused create keeps the old selection and its tick.
            rangeRows.selectionChanged()
            newGradient = false
            let brush = paints && range == nil
            toolRow?.setSelected(brush ? 1 : 0); onTool?(brush ? .add : .pan)
        } catch { selectedID = previous; throw error }
    }
    @objc private func invert(_ sender: Any?) { accept(["op": "invert"], dabs: 0) }
    @objc private func remap(_ sender: Any?) {
        guard ready, let selectedID,
              let map = RawLocalRemapSheetController.run(width: state.width, height: state.height) else { return }
        do {
            let object = try JSONSerialization.jsonObject(with: Data(map.json().utf8))
            try onOperation?(["op": "remap", "id": selectedID, "map": object], 0)
        } catch { report(error) }
    }
    @objc private func remove(_ sender: Any?) {
        guard ready, let selectedID else { return }
        do { try onOperation?(["op": "remove", "id": selectedID], 0) } catch { report(error) }
    }
    private func move(_ delta: Int) {
        guard ready, let selectedID, let index = state.nodes.firstIndex(where: { $0.id == selectedID }),
              state.nodes.indices.contains(index + delta) else { return }
        do { try onOperation?(["op": "reorder", "id": selectedID, "index": index + delta], 0) } catch { report(error) }
    }
    @objc private func earlier(_ sender: Any?) { move(-1) }
    @objc private func later(_ sender: Any?) { move(1) }
    @objc private func copySettings(_ sender: Any?) {
        guard let node = selectedNode else { return }
        var fields: [String: Any] = ["exposure_offset": node.exposureOffset, "white_balance": node.whiteBalance.rawValue]
        if let temperature = node.temperature { fields["temperature"] = temperature }
        if let tint = node.tint { fields["tint"] = tint }
        fields["tone"] = node.tone?.json ?? NSNull()
        Self.copiedSettings = fields
    }
    @objc private func pasteSettings(_ sender: Any?) {
        guard let selectedID, let copied = Self.copiedSettings else { NSSound.beep(); return }
        do {
            var fields = try controls()
            fields.removeValue(forKey: "temperature"); fields.removeValue(forKey: "tint")
            fields.merge(copied) { _, new in new }; fields["id"] = selectedID
            try onOperation?(fields, 0)
        } catch { report(error) }
    }
    private func addNodeSlider(_ label: String, _ id: String, _ range: ClosedRange<Double>, _ value: Double) {
        let row = cardSliderRow(label: label, min: range.lowerBound, max: range.upperBound, value: value,
            format: { String(format: "%.2f", $0) }) { [weak self] _ in self?.changed() }
        row.setAccessibilityIdentifier("raw-local-" + id)
        nodeRows.append(row); view.addArrangedSubview(row)
    }
    private func addBrush(_ label: String, _ id: String, _ range: ClosedRange<Double>, _ value: Double,
                          _ write: @escaping (RawLocalControls, Double) -> Void) {
        let row = cardSliderRow(label: label, min: range.lowerBound, max: range.upperBound, value: value,
            format: { String(format: "%.2f", $0) }) { [weak self] value in
                guard let self else { return }; write(self, value)
                self.onBrush?(self.radius, self.hardness, self.flow)
            }
        row.setAccessibilityIdentifier("raw-local-" + id); view.addArrangedSubview(row)
    }
    private func button(_ title: String, _ id: String, _ action: Selector) -> NSButton {
        let button = StickerButton(title: title, style: .secondary, target: self, action: action)
        button.setAccessibilityIdentifier("raw-local-" + id); buttons.append(button); return button
    }
    private func report(_ error: Error) { onError?(error.localizedDescription); NSSound.beep() }
}
