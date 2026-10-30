import AppKit

// Filters ▸ Lens Correction…'s Lens Profile popup (58f5cc9c) through the
// sheet's own popup and Apply: on a JPEG whose EXIF names a Lensfun lens
// (`verify_v1_lensfun.py`'s camera and lens: Z6.NEF, a quarter the size, as
// JPEG), Auto names the camera, lens and database and corrects the picture as
// `apply_filter` does; the lens picked by name records its id; None changes
// nothing; and a picture with no EXIF shows "no match" and Auto is a no-op.
//
// That drive develops Z6.NEF in the app (8 bits, no profile corrections) and
// saves a quarter-size JPEG. Here the development cannot run: it plans 2.46 GB
// of working memory, over the 2 GiB this probe's process is pinned to, and the
// limit is resolved once per process. So ImageIO writes the JPEG instead, from
// the camera file's own embedded preview at a quarter of its long side, with
// the camera file's own EXIF (its make, model and lens) and the new pixel size.
// The JPEG is then opened as `open_document` opens it.

/// A quarter-size JPEG of `raw`'s embedded preview carrying `raw`'s metadata.
private func lensJPEG(from raw: URL, to url: URL) throws {
    guard let source = CGImageSourceCreateWithURL(raw as CFURL, nil),
          var properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
          let wide = properties[kCGImagePropertyPixelWidth] as? Int,
          let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
              kCGImageSourceCreateThumbnailFromImageIfAbsent: true,
              kCGImageSourceCreateThumbnailWithTransform: true,
              kCGImageSourceThumbnailMaxPixelSize: max(wide, properties[kCGImagePropertyPixelHeight]
                  as? Int ?? 0) / 4,
          ] as CFDictionary) else {
        throw ProbeFailure(description: "\(raw.lastPathComponent) has no preview to write")
    }
    var exif = properties[kCGImagePropertyExifDictionary] as? [CFString: Any] ?? [:]
    exif[kCGImagePropertyExifPixelXDimension] = image.width
    exif[kCGImagePropertyExifPixelYDimension] = image.height
    properties[kCGImagePropertyExifDictionary] = exif
    properties[kCGImagePropertyOrientation] = 1
    properties[kCGImagePropertyPixelWidth] = image.width
    properties[kCGImagePropertyPixelHeight] = image.height
    if var tiff = properties[kCGImagePropertyTIFFDictionary] as? [CFString: Any] {
        tiff[kCGImagePropertyTIFFOrientation] = 1
        properties[kCGImagePropertyTIFFDictionary] = tiff
    }
    guard let destination = CGImageDestinationCreateWithURL(url as CFURL, "public.jpeg" as CFString,
                                                            1, nil) else {
        throw ProbeFailure(description: "no JPEG destination at \(url.path)")
    }
    CGImageDestinationAddImage(destination, image, properties as CFDictionary)
    guard CGImageDestinationFinalize(destination) else {
        throw ProbeFailure(description: "the JPEG at \(url.path) was not written")
    }
}

/// `url` opened as `open_document` opens it (8 bits; a camera file would be
/// developed headlessly without profile corrections), installed off screen.
private func lensOpen(_ url: URL) throws -> ImageDocument {
    let precision = PrecisionImportRequest.mode
    PrecisionImportRequest.mode = .rgba8
    defer { PrecisionImportRequest.mode = precision }
    var settings = RawDevelopSettings()
    settings.lensCorrection = false
    let raw = RawImportRequest.mode
    RawImportRequest.mode = .headless(settings)
    defer { RawImportRequest.mode = raw }
    let type = try NSDocumentController.shared.typeForContents(of: url)
    let document = try ImageDocument(contentsOf: url, ofType: type)
    document.undoManager?.groupsByEvent = false
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    document.undoManager?.removeAllActions()
    return document
}

/// The Lens Correction sheet `showLensCorrection(_:)` just presented.
private func lensSheet(_ controller: EditorViewController,
                       _ tag: String) throws -> SliderSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showLensCorrection(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? SliderSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Lens Correction sheet") }
    return sheet
}

private func lensPopup(_ sheet: SliderSheetController) throws -> NSPopUpButton {
    try uiControl(sheet.view, id: "lens-correction.profile", as: NSPopUpButton.self)
}

private func lensStatus(_ sheet: SliderSheetController) throws -> String {
    try uiControl(sheet.view, id: "lens-correction.profile-status", as: NSTextField.self).stringValue
}

private func lensChoose(_ sheet: SliderSheetController, _ index: Int, _ tag: String) throws {
    let popup = try lensPopup(sheet)
    popup.selectItem(at: index)
    try uiSend(popup, "\(tag) the Lens Profile popup")
}

/// Apply, pumped until the sheet has closed and every worker is done.
private func lensApply(_ sheet: SliderSheetController, _ tag: String) throws {
    guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) the sheet has no Apply")
    }
    try uiSend(apply, "\(tag) Apply")
    try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied
                    && DocumentJobs.active == nil }, seconds: 120)
}

/// One RGB channel's mean over a `side`-pixel square at (x0, y0).
private func lensPatchMean(_ bytes: [UInt8], width: Int, x0: Int, y0: Int, side: Int,
                           channel: Int) -> Double {
    var sum = 0.0
    for y in y0..<(y0 + side) {
        for x in x0..<(x0 + side) { sum += Double(bytes[(y * width + x) * 4 + channel]) }
    }
    return sum / Double(side * side)
}

/// "lens correction profile": Auto, a lens by name and None on the Z6
/// JPEG, and "no match" on a picture with no EXIF.
func verifyLensCorrectionProfile() throws {
    let tag = "[lens correction profile]"
    // The fixture: Z6.NEF's preview and EXIF as a quarter-size JPEG.
    let jpeg = directory.appendingPathComponent("lens-correction-z6.jpg")
    try lensJPEG(from: try uiFixture("raw-pixls/nikon-z6/Z6.NEF"), to: jpeg)
    let document = try lensOpen(jpeg)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc, !base.isHighDepth, let original = base.layerPixels(0),
          base.lensIdentity.lensModel != nil else {
        throw ProbeFailure(description: "\(tag) the JPEG is an 8-bit document whose EXIF names a "
            + "lens: \(String(describing: document.doc?.lensIdentity))")
    }
    let (width, height) = (base.width, base.height)
    let match = try LensProfileMatch.resolve(base.lensIdentity, .auto, all: true,
                                             dimensions: (width, height))
    guard let camera = match.camera, let lensModel = match.selected?.model,
          match.profile != nil else {
        throw ProbeFailure(description: "\(tag) the database matched no camera and lens: "
            + match.summary)
    }
    try require(lensModel == "NIKKOR Z 24-70mm f/4 S",
                "\(tag) the database selects NIKKOR Z 24-70mm f/4 S: \(lensModel)")
    let entries = document.history.cursor
    let alerts = AutomationAlerts.recorded.count
    let centre = ((height / 2) * width + width / 2) * 4

    // Auto: the sheet names the match; Apply corrects the picture.
    var status = ""
    let autoSteps = try uiRecorded {
        let sheet = try lensSheet(controller, tag)
        let popup = try lensPopup(sheet)
        try require(popup.indexOfSelectedItem == 0
                    && popup.titleOfSelectedItem == "Auto \u{2014} \(lensModel)",
                    "\(tag) the popup opens on Auto with the lens: "
                    + "\(popup.titleOfSelectedItem ?? "-")")
        status = try lensStatus(sheet)
        try require(status.contains(camera) && status.contains(lensModel)
                    && status.contains("Lensfun v0.3.4"),
                    "\(tag) the status names \(camera), the lens and Lensfun v0.3.4: \(status)")
        try lensApply(sheet, "\(tag) Auto")
    }
    guard let corrected = document.doc, corrected !== base,
          let autoPixels = corrected.layerPixels(0) else {
        throw ProbeFailure(description: "\(tag) Auto changed nothing: \(uiAlerts())")
    }
    try require(document.history.cursor == entries + 1
                && document.undoManager?.undoActionName == "Lens Correction",
                "\(tag) Auto is one \"Lens Correction\" step: \(document.history.entries.map(\.name))")
    try require(autoSteps.count == 1 && autoSteps[0].tool == "apply_filter"
                && autoSteps[0].arguments["filter"] as? String == "lens_correction"
                && autoSteps[0].arguments["profile"] as? String == "auto",
                "\(tag) Auto records apply_filter lens_correction profile \"auto\": "
                + "\(autoSteps.map { "\($0.tool) \($0.arguments)" })")
    try require((0..<3).allSatisfy { abs(Int(autoPixels[centre + $0]) - Int(original[centre + $0])) <= 1 },
                "\(tag) the centre pixel stays within 1 code")
    let cornerShift = (0..<3).map {
        abs(lensPatchMean(autoPixels, width: width, x0: 0, y0: 0, side: 16, channel: $0)
            - lensPatchMean(original, width: width, x0: 0, y0: 0, side: 16, channel: $0))
    }.max() ?? 0
    try require(cornerShift > 1, "\(tag) the corner patch's mean moves by more than 1 code: "
                + "\(cornerShift)")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the JPEG")
    _ = try agentOK("apply_filter", document, ["filter": "lens_correction", "profile": "auto"])
    try require(document.doc?.layerPixels(0) == autoPixels,
                "\(tag) the sheet's Auto equals apply_filter lens_correction profile auto")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) the MCP twin undoes")

    // The lens by name: its id is recorded, the pixels are Auto's.
    var lensID = ""
    let named = try uiRecorded {
        let sheet = try lensSheet(controller, tag)
        let popup = try lensPopup(sheet)
        let index = popup.indexOfItem(withTitle: lensModel)
        try require(index > 1, "\(tag) the popup lists \(lensModel) as a candidate: \(popup.itemTitles)")
        lensID = popup.item(at: index)?.representedObject as? String ?? ""
        try lensChoose(sheet, index, tag)
        try lensApply(sheet, "\(tag) \(lensModel)")
    }
    try require(!lensID.isEmpty && named.count == 1
                && named[0].arguments["profile"] as? String == lensID,
                "\(tag) the named lens records its id \(lensID): "
                + "\(named.map { "\($0.tool) \($0.arguments)" })")
    try require(document.doc?.layerPixels(0) == autoPixels
                && document.history.cursor == entries + 1,
                "\(tag) the named lens is one step with Auto's pixels")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) the named lens undoes")

    // None: no step, nothing recorded, the pixels unchanged.
    let none = try uiRecorded {
        let sheet = try lensSheet(controller, tag)
        let index = try lensPopup(sheet).indexOfItem(withTitle: "None")
        try lensChoose(sheet, index, tag)
        try lensApply(sheet, "\(tag) None")
    }
    try require(document.doc === base && document.undoManager?.canUndo != true && none.isEmpty
                && document.history.cursor == entries,
                "\(tag) None adds no step and records nothing: \(none.map(\.tool))")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")

    // A picture with no EXIF: "no match", and Auto is a no-op.
    let plain = try fixture(width: 320, height: 240)
    defer { plain.close() }
    let plainController = try editor(plain)
    guard let plainBase = plain.doc else { throw ProbeFailure(description: "\(tag) no plain fixture") }
    var unmatched = ""
    let plainSteps = try uiRecorded {
        let sheet = try lensSheet(plainController, tag)
        let popup = try lensPopup(sheet)
        try require(popup.itemTitles.first == "Auto \u{2014} no match"
                    && popup.titleOfSelectedItem == "None",
                    "\(tag) no EXIF: Auto has no match and None is selected: \(popup.itemTitles)")
        // None's own line; Auto's line says why nothing matched.
        let noneStatus = try lensStatus(sheet)
        try require(noneStatus == "No lens profile.", "\(tag) None's status: \(noneStatus)")
        try lensChoose(sheet, 0, tag)
        unmatched = try lensStatus(sheet)
        try require(unmatched.hasPrefix("No Lensfun profile:"),
                    "\(tag) Auto's status says why nothing matched: \(unmatched)")
        try lensApply(sheet, "\(tag) Auto with no match")
    }
    try require(plain.doc === plainBase && plain.undoManager?.canUndo != true && plainSteps.isEmpty,
                "\(tag) Auto with no match adds no step and records nothing: \(plainSteps.map(\.tool))")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    print("PASS: lens correction profile — on the Z6 JPEG the Lens Profile popup opens on "
        + "\"Auto — "
        + "\(lensModel)\" with the status \"\(status)\"; Apply is one \"Lens Correction\" step "
        + "recorded profile \"auto\" that keeps the centre within 1 code and moves a corner patch "
        + String(format: "%.1f", cornerShift) + " codes, the same bytes as apply_filter "
        + "lens_correction profile auto; the lens picked by name records \"\(lensID)\" with Auto's "
        + "pixels; None adds no step; with no EXIF the popup reads \"Auto — no match\" on None, "
        + "Auto says \"\(unmatched)\" and its Apply adds no step (58f5cc9c)")
}
