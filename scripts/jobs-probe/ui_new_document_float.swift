import AppKit

// File ▸ New… at 32 bits (698b2d10) through the New Document card's own
// controls: the Depth popup at "32 bits/channel (float)" and each background
// fill read back at its exact linear value, and a grayscale profile refused
// by name at 32 bits by the model and by `new_document`.

/// How closely a float value matches `uiLinearSRGB`: 1e-5 absolute, the core's
/// own bound (`core/tests/float_promote.rs`), because the built-in sRGB TRC is
/// a 1024-entry 16-bit `curv`, so its quantization is at most 0.5/65535 plus
/// interpolation.
let uiLinearSRGBTolerance = 1e-5

/// The piecewise sRGB EOTF of an 8-bit code taken through 16 bits
/// (`code·257/65535`): what a float document's background holds.
func uiLinearSRGB(_ code: Int) -> Double {
    let v = Double(code * 257) / 65535
    return v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4)
}

/// The control beside the row label `title` (the card's `row(_:_:)` stacks).
private func newDocumentRowControl<T: NSView>(_ root: NSView, _ title: String,
                                              as type: T.Type) throws -> T {
    for stack in uiViews(root, as: NSStackView.self) {
        let views = stack.arrangedSubviews
        guard let label = views.first as? NSTextField, label.stringValue == title,
              let control = views.dropFirst().compactMap({ $0 as? T }).first else { continue }
        return control
    }
    throw ProbeFailure(description: "the New Document card has no \(T.self) in its \(title) row")
}

/// The one popup under `root` that lists `item`.
private func newDocumentPopup(_ root: NSView, listing item: String) throws -> NSPopUpButton {
    guard let popup = uiViews(root, as: NSPopUpButton.self).first(where: {
        $0.itemTitles.contains(item)
    }) else {
        throw ProbeFailure(description: "the New Document card has no popup listing \(item)")
    }
    return popup
}

/// File ▸ New… with the card set to 64 × 48 px, sRGB, 32 bits and the
/// background `choose` picks; answers the one document the Create opened.
private func newFloatDocument(_ tag: String,
                              choose: @escaping (NSView, NSButton) throws -> Void) throws
    -> ImageDocument
{
    let known = Set(NSDocumentController.shared.documents.map(ObjectIdentifier.init))
    try ModalSteps(title: "New Document", steps: [
        ("the size, profile, depth and background; Create", { window in
            guard let card = window.contentView else { return false }
            let unit = try newDocumentPopup(card, listing: NewDocumentUnit.px.displayName)
            unit.selectItem(withTitle: NewDocumentUnit.px.displayName)
            try uiSend(unit, "\(tag) the unit popup")
            let width = try newDocumentRowControl(card, "Width", as: NSTextField.self)
            width.stringValue = "64"
            try uiSend(width, "\(tag) the Width field")
            let height = try newDocumentRowControl(card, "Height", as: NSTextField.self)
            height.stringValue = "48"
            try uiSend(height, "\(tag) the Height field")
            let profile = try newDocumentPopup(card, listing: "Profile File…")
            profile.selectItem(withTitle: "sRGB")
            try uiSend(profile, "\(tag) the Profile popup")
            let depth = try newDocumentPopup(card, listing: "32 bits/channel (float)")
            try require(depth.indexOfItem(withTitle: "32 bits/channel (float)") == 2,
                        "\(tag) the Depth popup's third item is 32 bits/channel (float)")
            depth.selectItem(at: 2)
            if let action = depth.action { NSApp.sendAction(action, to: depth.target, from: depth) }
            let create = try uiControl(card, id: "new-document-create", as: NSButton.self)
            try choose(card, create)
            try uiSend(create, "\(tag) Create")
            return true
        }),
    ]).run { AppDelegate().newDocument(nil) }
    let opened = NSDocumentController.shared.documents.filter {
        !known.contains(ObjectIdentifier($0))
    }
    guard opened.count == 1, let document = opened.first as? ImageDocument else {
        throw ProbeFailure(description: "\(tag) Create opened \(opened.count) documents: "
            + "\(uiAlerts())")
    }
    document.windowControllers.first?.window?.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    return document
}

/// Every pixel of the composite reads `want` exactly (`tolerance` 0) or
/// within `tolerance`.
private func newDocumentEveryPixel(_ document: ImageDocument, _ want: [Double],
                                   tolerance: Double, _ tag: String) throws {
    guard let doc = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    for y in 0..<doc.height {
        for x in 0..<doc.width {
            guard let got = try FloatReadout.at((x: x, y: y), document: document)?.values else {
                throw ProbeFailure(description: "\(tag) no float sample at (\(x), \(y))")
            }
            let same = got.count == 4 && zip(got, want).allSatisfy {
                tolerance == 0 ? $0 == $1 : abs($0 - $1) <= tolerance
            }
            try require(same, "\(tag) pixel (\(x), \(y)) reads \(got), not \(want)")
        }
    }
}

/// "new document float": four backgrounds at 32 bits, and gray refused.
func verifyNewDocumentFloat() throws {
    let tag = "[new document float]"
    let alerts = AutomationAlerts.recorded.count
    let custom = (r: 0x33, g: 0x66, b: 0x99)
    let runs: [(name: String, index: Int, want: [Double], tolerance: Double, layer: String?)] = [
        ("White", 1, [1, 1, 1, 1], 0, "Background"),
        ("Black", 2, [0, 0, 0, 1], 0, nil),
        ("Transparent", 0, [0, 0, 0, 0], 0, "Layer 1"),
        ("Custom #336699", 3,
         [uiLinearSRGB(custom.r), uiLinearSRGB(custom.g), uiLinearSRGB(custom.b), 1],
         uiLinearSRGBTolerance, nil),
    ]
    for run in runs {
        let what = "\(tag) \(run.name)"
        var created: ImageDocument?
        defer { created?.close() }
        let recorded = try uiRecorded {
            created = try newFloatDocument(what) { card, create in
                let background = try newDocumentPopup(card, listing: "Custom Colour…")
                if run.index == 3 {
                    // selectItem only: the popup's own action would order the
                    // colour panel on screen. The pick is then delivered to
                    // the card exactly as the panel delivers it.
                    background.selectItem(at: 3)
                    NSColorPanel.shared.color = NSColor(
                        srgbRed: CGFloat(custom.r) / 255, green: CGFloat(custom.g) / 255,
                        blue: CGFloat(custom.b) / 255, alpha: 1)
                    let pick = NSSelectorFromString("colorPicked:")
                    guard let card = create.target as? NSObject, card.responds(to: pick) else {
                        throw ProbeFailure(description: "\(what) the Create button's target "
                            + "(\(String(describing: create.target))) takes no colorPicked:")
                    }
                    // The message itself: inside the card's modal session
                    // `NSApp.sendAction` from the unshown panel answers false,
                    // so the pick is sent straight to the card; the popup's
                    // retitled item shows it landed.
                    _ = card.perform(pick, with: NSColorPanel.shared)
                    try require(background.item(at: 3)?.title == "#336699",
                                "\(what) the pick names the colour in the popup: "
                                + "\(background.item(at: 3)?.title ?? "-")")
                } else {
                    background.selectItem(at: run.index)
                    try uiSend(background, "\(what) the Background popup")
                }
            }
        }
        guard let document = created, let doc = document.doc else {
            throw ProbeFailure(description: "\(what) no document")
        }
        try require(doc.isFloat && doc.width == 64 && doc.height == 48,
                    "\(what) a 64 × 48 32-bit float document: float \(doc.isFloat), "
                    + "\(doc.width) × \(doc.height)")
        if let layer = run.layer {
            try require(doc.layerInfo(0)?.name == layer,
                        "\(what) the layer is \(layer): \(doc.layerInfo(0)?.name ?? "-")")
        }
        try newDocumentEveryPixel(document, run.want, tolerance: run.tolerance, what)
        try require(document.undoManager?.canUndo != true, "\(what) no undo step")
        try require(recorded.isEmpty, "\(what) nothing recorded: \(recorded.map(\.tool))")
        try require(AutomationAlerts.recorded.count == alerts, "\(what) no alert: \(uiAlerts())")
    }

    // Gray at 32: the model's sentence names the path; new_document says it.
    let gray = "/System/Library/ColorSync/Profiles/Generic Gray Profile.icc"
    var sentence = ""
    do {
        _ = try NewDocumentProfile(name: gray)
        throw ProbeFailure(description: "\(tag) the gray profile was accepted")
    } catch let error as DoctoredCoreError {
        sentence = error.message
    }
    try require(sentence.contains(gray) && sentence.hasSuffix("choose the Grayscale colour mode for a gray document."),
                "\(tag) the gray refusal names the path and ends as declared: \(sentence)")
    let before = NSDocumentController.shared.documents.count
    let reply = try uiExecute("new_document", [
        "width": 64, "height": 48, "depth": 32, "profile": gray,
    ])
    try require(reply.isError && reply.text.contains(sentence),
                "\(tag) new_document refuses in band with the sentence: \(reply.text)")
    try require(NSDocumentController.shared.documents.count == before,
                "\(tag) the refused new_document opened no document")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    print("PASS: new document float — File ▸ New… with Depth \"32 bits/channel (float)\" at 64 × 48 "
        + "px sRGB opens a 32-bit float document whose every pixel reads White [1, 1, 1, 1] "
        + "(\"Background\"), Black [0, 0, 0, 1], Transparent [0, 0, 0, 0] (\"Layer 1\") and Custom "
        + "#336699 its linear sRGB decode within 1e-5 (the built-in TRC's bound), each with no undo step, nothing recorded and "
        + "no alert; the gray profile is refused by name at 32 bits by the model and in band by "
        + "new_document, which opens nothing")
}
