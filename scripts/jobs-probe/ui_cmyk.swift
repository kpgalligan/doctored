import AppKit

// The CMYK document's in-process UI cases (#547 phase 1): Image ▸ Mode's sheet through its own
// Apply and Cancel with one undo step, the Channels panel's four ink rows (view only), and the
// open routing that decides a CMYK TIFF BEFORE the 16-bit prompt. The CMYK profile is macOS's
// Generic CMYK Profile, read from the system.

private let genericCMYK = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// A little-endian chunky CMYK TIFF (Photometric 5) of one repeated ink, `bits` 8 or 16, with
/// `icc` embedded — written here from the TIFF 6.0 specification.
private func cmykTIFF(width: Int, height: Int, bits: Int, ink: [UInt16], icc: Data) -> Data {
    var body = Data()
    for _ in 0..<(width * height) {
        for value in ink {
            if bits == 8 { body.append(UInt8(value)) } else {
                body.append(UInt8(value & 0xFF)); body.append(UInt8(value >> 8))
            }
        }
    }
    let dataLength = body.count
    func le16(_ v: Int) -> [UInt8] { [UInt8(v & 0xFF), UInt8((v >> 8) & 0xFF)] }
    func le32(_ v: Int) -> [UInt8] { le16(v & 0xFFFF) + le16(v >> 16) }
    func place(_ bytes: Data) -> Int {
        if body.count % 2 == 1 { body.append(0) }
        let at = 8 + body.count
        body.append(bytes)
        return at
    }
    let bitsAt = place(Data(le16(bits) + le16(bits) + le16(bits) + le16(bits)))
    let iccAt = place(icc)
    if body.count % 2 == 1 { body.append(0) }
    let entries: [(Int, Int, Int, [UInt8])] = [
        (256, 4, 1, le32(width)), (257, 4, 1, le32(height)), (258, 3, 4, le32(bitsAt)),
        (259, 3, 1, le16(1) + [0, 0]), (262, 3, 1, le16(5) + [0, 0]), (273, 4, 1, le32(8)),
        (277, 3, 1, le16(4) + [0, 0]), (278, 4, 1, le32(height)), (279, 4, 1, le32(dataLength)),
        (284, 3, 1, le16(1) + [0, 0]), (34675, 7, icc.count, le32(iccAt)),
    ]
    var out = Data([0x49, 0x49, 42, 0] + le32(8 + body.count))
    out.append(body)
    out.append(contentsOf: le16(entries.count))
    for (tag, kind, count, value) in entries {
        out.append(contentsOf: le16(tag) + le16(kind) + le32(count) + value)
    }
    out.append(contentsOf: le32(0))
    return out
}

/// `pumpUntil` with the step named in its failure.
private func waitFor(_ what: String, seconds: Double, _ predicate: () -> Bool) throws {
    do { try pumpUntil(predicate, seconds: seconds) } catch {
        throw ProbeFailure(description: "timed out: \(what)")
    }
}

/// "cmyk mode sheet": Image ▸ Mode ▸ CMYK Color… — Cancel changes nothing, Apply converts as ONE
/// undo step recorded as `convert_mode`, undo and redo walk it, and the items check the mode.
func verifyCmykModeSheet() throws {
    let tag = "[cmyk mode sheet]"
    WorkingCMYK.install()
    let document = try fixture(width: 160, height: 120)
    defer { document.close() }
    let controller = try editor(document)
    try require(uiValidates(controller, "convertToCMYKMode:"), "\(tag) CMYK Color… validates on RGB")
    try require(!uiValidates(controller, "convertToRGBMode:"), "\(tag) RGB Color… is the current mode")
    func openSheet() throws -> CmykModeSheetController {
        let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
        controller.convertToCMYKMode(nil)
        let opened = {
            (controller.presentedViewControllers ?? []).compactMap { $0 as? CmykModeSheetController }
                .last { !known.contains(ObjectIdentifier($0)) }
        }
        try waitFor("\(tag) the sheet appears", seconds: 20) { opened()?.view.window?.isVisible == true }
        guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Mode sheet") }
        return sheet
    }
    let entries = document.history.entries.count
    let base = document.doc
    // Cancel: nothing changes.
    let first = try openSheet()
    try uiSend(try uiControl(first.view, id: "cmyk-mode-cancel", as: NSButton.self), "\(tag) Cancel")
    try waitFor("\(tag) Cancel dismisses the sheet", seconds: 10) {
        first.view.window?.isVisible != true
    }
    try require(document.doc === base && document.history.entries.count == entries,
                "\(tag) Cancel leaves the document and its history unchanged")
    // Apply: one step, recorded.
    let recorded = try uiRecorded {
        let sheet = try openSheet()
        let popup = try uiControl(sheet.view, id: "cmyk-mode-profile", as: NSPopUpButton.self)
        try require(popup.titleOfSelectedItem?.hasPrefix("Working CMYK") == true,
                    "\(tag) the profile popup opens on the working CMYK: \(popup.titleOfSelectedItem ?? "nil")")
        try uiSend(try uiControl(sheet.view, id: "cmyk-mode-apply", as: NSButton.self), "\(tag) Apply")
        try waitFor("\(tag) Apply converts (\(uiAlerts()))", seconds: 60) { document.doc?.isCmyk == true }
    }
    try require(document.history.entries.count == entries + 1,
                "\(tag) Apply is one history step: \(document.history.entries.count - entries)")
    try require(recorded.map(\.tool) == ["convert_mode"], "\(tag) recorded: \(recorded.map(\.tool))")
    try require(recorded.first?.arguments["profile"] as? String == "working",
                "\(tag) the step records its destination: \(recorded.first?.arguments ?? [:])")
    try require(uiValidates(controller, "convertToRGBMode:") && !uiValidates(controller, "convertToCMYKMode:"),
                "\(tag) the items follow the mode")
    document.undoManager?.undo()
    try require(document.doc?.isCmyk == false, "\(tag) Undo restores RGB")
    document.undoManager?.redo()
    try require(document.doc?.isCmyk == true, "\(tag) Redo restores CMYK")
    try require(AutomationAlerts.recorded.isEmpty || !uiAlerts().contains("16-bit"),
                "\(tag) no 16-bit prompt: \(uiAlerts())")
}

/// "cmyk channels panel": a CMYK document lists CMYK, Cyan, Magenta, Yellow and Black; selecting
/// an ink row shows that plane on the canvas, ink dark, and targets that plate (the Move tool and
/// ⌘T on its selected pixels, #547's channels batch).
func verifyCmykChannelsPanel() throws {
    let tag = "[cmyk channels panel]"
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: genericCMYK), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard let doc = document.doc, doc.isCmyk else { throw ProbeFailure(description: "\(tag) not CMYK") }
    let rows = doc.channelRows(activeLayer: 0)
    try require(rows == [.composite] + CmykPlane.allCases.map { ChannelRow.ink($0) },
                "\(tag) the rows are the composite and four inks: \(rows)")
    guard let panel = controller.channelsPanel else { throw ProbeFailure(description: "\(tag) no panel") }
    _ = panel.view
    panel.reload()
    guard let table = Mirror(reflecting: panel).children.first(where: { $0.label == "tableView" })?
        .value as? NSTableView else { throw ProbeFailure(description: "\(tag) no table") }
    try require(table.numberOfRows == 5, "\(tag) five rows: \(table.numberOfRows)")
    table.selectRowIndexes(IndexSet(integer: 1), byExtendingSelection: false)
    try require(panel.visibility.base == .ink(.cyan), "\(tag) Cyan is shown: \(panel.visibility.base)")
    try require(panel.paintTarget == .ink(.cyan), "\(tag) an ink row targets its plate: \(panel.paintTarget)")
    controller.refreshChannelDisplay()
    try require(controller.canvas.channelDisplay?.base != nil,
                "\(tag) the canvas draws the cyan plane")
    guard let plane = doc.planeImage(for: .ink(.black), composite: nil, activeLayer: 0, maxSide: 0)
    else { throw ProbeFailure(description: "\(tag) no black plane") }
    try require(plane.width == 64 && plane.height == 48, "\(tag) the plane is canvas-sized")
}

/// "cmyk open routing": with the open mode `.ask`, an 8-bit and a 16-bit CMYK TIFF open as CMYK
/// documents with NO alert (the 16-bit prompt is never shown); `.rgba8` keeps today's sRGB path.
func verifyCmykOpenRouting() throws {
    let tag = "[cmyk open routing]"
    let folder = try uiCaseFolder("cmyk-open")
    let icc = try Data(contentsOf: genericCMYK)
    let previous = PrecisionImportRequest.mode
    defer { PrecisionImportRequest.mode = previous }
    for bits in [8, 16] {
        let url = folder.appendingPathComponent("ink\(bits).tif")
        try cmykTIFF(width: 20, height: 10, bits: bits,
                     ink: bits == 8 ? [200, 40, 10, 90] : [51400, 10280, 2570, 23130], icc: icc)
            .write(to: url)
        try require(RasterPrecisionDocument.cmykSource(at: url) == .tiff, "\(tag) \(bits)-bit is a CMYK TIFF")
        PrecisionImportRequest.mode = .ask
        AutomationAlerts.reset()
        guard let opened = try ImageDocument.openPrecisionDocumentIfRequested(at: url) else {
            throw ProbeFailure(description: "\(tag) the \(bits)-bit CMYK TIFF took the RGB path")
        }
        try require(opened.isCmyk, "\(tag) \(bits)-bit opens as CMYK")
        try require(AutomationAlerts.recorded.isEmpty, "\(tag) \(bits)-bit showed an alert: \(uiAlerts())")
        PrecisionImportRequest.mode = .rgba8
        let flat = try ImageDocument.openPrecisionDocumentIfRequested(at: url)
        try require(flat == nil, "\(tag) \(bits)-bit under rgba8 keeps the conversion to sRGB")
    }
}
