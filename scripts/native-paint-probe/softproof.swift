import AppKit

// View ▸ Proof Setup… / Proof Colors / Gamut Warning at 16 bits,
// the canvas proof slot, its cancel-on-new-request contract (the slot has no
// MCP surface, so this is where that is checked), the float early return and
// the visible failure; and the File ▸ export items at depth.

private let genericCMYK = "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc"

/// A nil-target menu item resolved through AppKit's responder chain; the
/// resolved target must be this editor, and its validation must answer.
private func validates(_ editor: EditorViewController, _ name: String, label: String) throws -> Bool {
    let item = NSMenuItem(title: "Item", action: NSSelectorFromString(name), keyEquivalent: "")
    item.target = nil
    guard let resolved = NSApp.target(forAction: NSSelectorFromString(name), to: nil, from: item)
            as? EditorViewController else {
        throw DoctoredCoreError(message:
            "\(label): \(name) does not resolve to an editor through the responder chain")
    }
    try require(resolved === editor, "\(label): \(name) resolves to another editor")
    return resolved.validateUserInterfaceItem(item)
}

private let proofItems = ["showProofSetup:", "toggleProofColors:", "toggleGamutWarning:"]

private func cmykSetup() throws -> ProofSetup {
    guard let bytes = try? Data(contentsOf: URL(fileURLWithPath: genericCMYK)), !bytes.isEmpty else {
        throw DoctoredCoreError(message: "Owned proof profile missing at \(genericCMYK)")
    }
    return ProofSetup(profile: bytes, profileName: "Generic CMYK Profile", path: genericCMYK)
}

private func imageBytes(_ image: CGImage) -> Data? {
    image.dataProvider?.data as Data?
}

private func rasterBytes(_ image: RasterImage) -> Data? {
    image.withPlateBytes { Data(bytes: $0, count: image.width * image.height * 4) }
}

/// A 16-bit document filled with per-pixel noise, so a proof of it is a
/// real render rather than a memo hit per colour.
private func openNoiseDocument(width: Int, height: Int)
    throws -> (ImageDocument, EditorViewController, RasterDocument)
{
    var state: UInt64 = 0x9E37_79B9_7F4A_7C15
    var samples: [UInt16] = []
    samples.reserveCapacity(width * height * 4)
    for _ in 0..<(width * height) {
        for channel in 0..<4 {
            state = state &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
            samples.append(channel == 3 ? 65535 : UInt16(truncatingIfNeeded: state >> 48))
        }
    }
    let source = try RasterDocument.fromRawRGBA16(samples, width: width, height: height,
                                                  profile: RasterProfile.builtin(.sRGB))
    let document = ImageDocument(); document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: 16, exportDither: "none"), markSaved: true)
    NSDocumentController.shared.addDocument(document); document.makeWindowControllers(); document.showWindows()
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw DoctoredCoreError(message: "Actual editor missing")
    }
    _ = editor.view; editor.view.window?.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true); pump()
    return (document, editor, source)
}

/// The synchronous twin of the canvas's 16-bit proof: display codes, then
/// the byte proof of them.
private func synchronousDisplayProof(_ projection: RasterPrecisionImage, in doc: RasterDocument,
                                     setup: ProofSetup) throws -> Data {
    let display = try ProofRenderer.displayCodes(of: projection, in: doc)
    let context = try PrecisionWorkContext(byteImage: display)
    let rendered = try display.proofRender(
        documentProfile: doc.iccProfile ?? Data(), proof: setup.profile,
        setup: setup.jsonText(gamutWarning: false),
        workingLimit: context.limit, otherRetained: context.otherRetained)
    guard let bytes = rasterBytes(rendered.image) else {
        throw DoctoredCoreError(message: "Synchronous proof has no pixels")
    }
    return bytes
}

func softProofMenuUI() throws -> [String: Any] {
    let setup = try cmykSetup()
    var report: [String: Any] = ["case": "soft-proof-menu-ui", "passed": true]

    // 8 bits: all three items validate.
    do {
        let (document, editor, _) = try openByteDocument()
        defer { document.close(); pump() }
        for name in proofItems {
            try require(try validates(editor, name, label: "rgba8"), "rgba8: \(name) does not validate")
        }
    }

    // 16 bits: the items validate, the proof lands in the slot at the
    // canvas's own size, and `canvas.image` is never replaced.
    do {
        let (document, editor, _) = try openDocument()
        defer { document.close(); pump() }
        for name in proofItems {
            try require(try validates(editor, name, label: "rgba16"), "rgba16: \(name) does not validate")
        }
        let before = editor.canvas.image
        editor.applyProofSetup(setup, proofColors: true, gamutWarning: false)
        try waitUntil({ editor.canvas.proofImage != nil }, "rgba16: the proof never landed")
        guard let proof = editor.canvas.proofImage, let image = editor.canvas.image else {
            throw DoctoredCoreError(message: "rgba16: the proof or the canvas image is missing")
        }
        try require(proof.width == image.width && proof.height == image.height,
                    "rgba16: the proof is \(proof.width)×\(proof.height), the canvas \(image.width)×\(image.height)")
        try require(editor.canvas.image === before, "rgba16: proofing replaced canvas.image")
        try require(editor.proofStatusText?.hasPrefix("Proof: ") == true,
                    "rgba16: status says \(editor.proofStatusText ?? "nothing")")
        editor.toggleProofColors(nil)
        pump()
        try require(editor.canvas.proofImage == nil, "rgba16: toggling off left a proof")
        try require(editor.canvas.image === before, "rgba16: toggling off replaced canvas.image")

        // Forced failure: sRGB has no A2B/B2A tables, so the core refuses the
        // proof — which turns itself off visibly.
        var srgb = setup
        srgb.setProfile(RasterProfile.builtin(.sRGB), name: "sRGB", path: srgb.path)
        editor.applyProofSetup(srgb, proofColors: true, gamutWarning: false)
        try waitUntil({ editor.proofDisplay.failure != nil }, "rgba16: the sRGB proof never failed")
        try require(!editor.proofDisplay.proofColors, "rgba16: a failed proof left Proof Colors on")
        try require(editor.canvas.proofImage == nil, "rgba16: a failed proof left the slot filled")
        try require(editor.proofStatusText?.hasPrefix("Proof off — ") == true,
                    "rgba16: status after failure says \(editor.proofStatusText ?? "nothing")")
        report["failure"] = editor.proofDisplay.failure ?? ""
    }

    // Cancel-on-new-request: request A, then B while A may still be
    // rendering. A must never publish after B was issued; B publishes once
    // and is exactly B's own proof.
    do {
        let (document, editor, _) = try openNoiseDocument(width: 3000, height: 2000)
        defer { editor.proofRenderer.cancel(); document.close(); pump() }
        guard let doc = document.doc, let projection = document.retainedPrecisionProjection else {
            throw DoctoredCoreError(message: "cancel: the noise document has no retained projection")
        }
        var setupB = setup
        setupB.simulatePaperWhite = true
        var aCalls = 0
        var bOutcomes: [ProofOutcome] = []
        editor.proofRenderer.request(source: .display(doc, projection), setup: setup,
                                     gamutWarning: false) { _ in aCalls += 1 }
        pump(0.15)
        let aBeforeB = aCalls
        editor.proofRenderer.request(source: .display(doc, projection), setup: setupB,
                                     gamutWarning: false) { bOutcomes.append($0) }
        try waitUntil({ !bOutcomes.isEmpty }, "cancel: request B never delivered")
        pump(0.5)
        try require(aCalls == aBeforeB, "cancel: request A published after B was issued")
        try require(bOutcomes.count == 1, "cancel: request B published \(bOutcomes.count) times")
        guard case .rendered(let result)? = bOutcomes.first, let bBytes = imageBytes(result.image) else {
            throw DoctoredCoreError(message: "cancel: request B did not render")
        }
        let expectedB = try synchronousDisplayProof(projection, in: doc, setup: setupB)
        let expectedA = try synchronousDisplayProof(projection, in: doc, setup: setup)
        try require(bBytes == expectedB, "cancel: B's proof is not B's synchronous proof")
        try require(expectedA != expectedB, "cancel: A's and B's setups proof identically")
        report["cancel"] = ["a_undelivered_when_b_issued": aBeforeB == 0, "a_calls": aCalls,
                            "b_calls": bOutcomes.count]
    }

    // Float: proofing keeps its state but clears the slot; items disable;
    // Redo back to 16 bits proofs again.
    do {
        let (document, editor, _) = try openFloatDocument()
        defer { document.close(); pump() }
        for name in proofItems {
            try require(!(try validates(editor, name, label: "float")), "float: \(name) validates")
        }
        let conversion = try PrecisionModeConversion(document, mode: .rgba16, dither: .none)
        let prepared = try conversion.compute()
        try conversion.publish(prepared, to: document, record: .notACommand)
        pump()
        try require(document.doc?.isHighDepth == true && document.doc?.isFloat == false,
                    "float: the conversion did not reach 16 bits")
        editor.applyProofSetup(setup, proofColors: true, gamutWarning: false)
        try waitUntil({ editor.canvas.proofImage != nil }, "float→16: the proof never landed")
        document.undoManager?.undo()
        try waitUntil({ document.doc?.isFloat == true }, "float: undo did not return to float")
        pump()
        try require(editor.canvas.proofImage == nil, "float: the slot kept a proof")
        try require(editor.proofDisplay.proofColors, "float: Proof Colors turned off")
        try require(editor.proofStatusText == nil, "float: the status names a proof")
        for name in proofItems {
            try require(!(try validates(editor, name, label: "float undo")),
                        "float undo: \(name) validates")
        }
        document.undoManager?.redo()
        try waitUntil({ editor.canvas.proofImage != nil }, "redo: the proof never landed again")
        try require(document.doc?.isFloat == false, "redo: the document is still float")
    }
    return report
}

func exportItemsAtDepthUI() throws -> [String: Any] {
    let exports = ["showExportWorkspace:", "quickExport:", "exportLayers:", "showContactSheet:",
                   "exportAsPDF:"]
    do {
        let (document, editor, _) = try openDocument()
        defer { document.close(); pump() }
        for name in exports {
            try require(try validates(editor, name, label: "rgba16"), "rgba16: \(name) does not validate")
        }
        _ = try ExportWorkspaceSheetController(document: document)
        _ = try LayerExportSheetController(document: document)
        _ = try ContactSheetSheetController(document: document)
    }
    do {
        let (document, editor, _) = try openFloatDocument()
        defer { document.close(); pump() }
        // Contact Sheet… validates at float: its cells are each
        // document's tone-mapped 8-bit rendering.
        for name in exports {
            try require(try validates(editor, name, label: "float"), "float: \(name) does not validate")
        }
        _ = try ExportWorkspaceSheetController(document: document)
        _ = try LayerExportSheetController(document: document)
    }
    return ["case": "export-items-at-depth-ui", "passed": true]
}
