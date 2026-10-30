import AppKit

enum ScalarMorphology {
    static let kinds = ["invert", "feather", "smooth", "grow", "shrink", "border"]

    static func operation(kind: String, radius: Double) throws -> [String: Any] {
        guard kinds.contains(kind), radius.isFinite, (0...64).contains(radius) else {
            throw DoctoredCoreError(message: "Choose invert, feather, smooth, grow, shrink or border with radius 0…64 pixels.")
        }
        var operation: [String: Any] = ["kind": kind]
        if kind != "invert" { operation["radius"] = radius }
        return ["kind": "morphology", "operation": operation]
    }
}

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let scalarMorphologyHandlers: [String: ToolHandler] = [
        "modify_scalar": { $0.modifyScalar },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let scalarMorphologyToolFacts: [String: ToolFacts] = [
        "modify_scalar": [.commonTool],
    ]

    /// Mirrors Image > Channels > Modify Native Plane….
    func modifyScalar(_ a: [String: Any]) throws -> String {
        let allowed: Set<String> = ["document_id", "layer", "target", "operation", "radius"]
        guard Set(a.keys).isSubset(of: allowed) else { throw ToolError(message: "Unknown scalar morphology argument.") }
        let document = try target(a)
        let capture = try PrecisionEditorCapture(document)
        let target = try paintTarget(a, document), layer = try structuralLayerIndex(a, document)
        let kind = try requiredString(a, "operation"), radius = doubleArg(a, "radius") ?? 1
        let operation = try ScalarMorphology.operation(kind: kind, radius: radius)
        if !capture.quickMask, let reason = scopeRefusal(.filter, target: target,
            doc: capture.facade, layer: layer) { throw ToolError(message: reason) }
        let work = try PrecisionScalarOperations.edit(capture, target: target, layer: layer, operation: operation)
        let changed = try capture.publish(work.run(title: "Modify Plane"), to: document, name: "Modify Plane")
        return try jsonResult(["ok": true, "changed": changed, "operation": kind,
            "target": target.agentName(in: capture.facade), "layer": layer, "storage": "u16"])
    }

    @inline(never) func scalarMorphologyCatalog() -> [[String: Any]] {
        [tool("modify_scalar", "Modifies the raw native 16-bit mask, saved channel or RGBA plane through the current selection. Other planes, mask properties and channel identity remain exact. Neighborhoods read complete raw storage. Mirrors Image > Channels > Modify Native Plane.",
            ["document_id": Self.docIDProperty, "layer": ["type": "integer"],
             "target": ["type": "string", "description": "mask, red, green, blue, alpha or channel:<name>"],
             "operation": ["type": "string", "enum": ScalarMorphology.kinds],
             "radius": ["type": "number", "minimum": 0, "maximum": 64, "default": 1]],
            required: ["target", "operation"])]
    }
}

extension EditorViewController {
    @objc func modifyNativePlane(_ sender: Any?) {
        guard let document, let sheet = ScalarMorphologySheetController(document: document) else { NSSound.beep(); return }
        presentAsSheet(sheet)
    }
}

final class ScalarMorphologySheetController: NSViewController {
    private weak var document: ImageDocument?
    private let capture: PrecisionEditorCapture
    private let operation = NSPopUpButton(frame: .zero, pullsDown: false)
    private let radius = NSTextField(string: "1")

    init?(document: ImageDocument) {
        guard let capture = try? PrecisionEditorCapture(document), capture.target.isCoverage, !capture.quickMask else { return nil }
        self.document = document; self.capture = capture
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable) required init?(coder: NSCoder) { fatalError("ScalarMorphologySheetController does not support NSCoder") }

    override func loadView() {
        operation.addItems(withTitles: ScalarMorphology.kinds.map { $0.capitalized })
        operation.setAccessibilityIdentifier("scalar.morphology.operation")
        radius.setAccessibilityIdentifier("scalar.morphology.radius")
        DSField.style(radius); radius.widthAnchor.constraint(equalToConstant: 100).isActive = true
        let grid = NSGridView(views: [[fieldLabel("Operation:"), operation], [fieldLabel("Radius (px):"), radius]])
        grid.rowSpacing = 10; grid.columnSpacing = 12
        view = makeSheetView(title: "Modify native plane", hint: "Edits the captured scalar target through the current selection. Radius: 0–64 pixels.",
            content: grid, buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: sheetApplyButton(target: self, action: #selector(applyClicked(_:)))))
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard let document, let radius = Double(radius.stringValue),
              ScalarMorphology.kinds.indices.contains(operation.indexOfSelectedItem) else { NSSound.beep(); return }
        let kind = ScalarMorphology.kinds[operation.indexOfSelectedItem]
        do {
            try capture.requireCurrent(document)
            let operation = try ScalarMorphology.operation(kind: kind, radius: radius)
            if capture.target.isPlane, !document.confirmRasterize(layer: capture.layer) { return }
            try capture.requireCurrent(document)
            let work = try PrecisionScalarOperations.edit(capture, operation: operation)
            let step = ActionStep(tool: "modify_scalar", arguments: ["target": capture.target.agentName(in: capture.facade),
                "layer": capture.layer, "operation": kind, "radius": radius], note: "Modify Native Plane")
            let result = try work.run(title: "Modify Plane")
            _ = try capture.publish(result, to: document, name: "Modify Plane", record: [step])
            dismiss(self)
        } catch { document.presentError(error) }
    }

    @objc private func cancelClicked(_ sender: Any?) { dismiss(self) }
}
