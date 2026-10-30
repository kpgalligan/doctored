import AppKit

/// The Vision selections in a 32-bit float document, through the UI
/// paths. The picture is the licensed portrait decoded to linear light (IEC
/// 61966-2.1) with the suit's chest scaled above white, the picture
/// `scripts/verify_phase11f_float.py`'s F43 builds; Vision reads the
/// document's tone-mapped display at view exposure 0, so the window's exposure
/// must not change what the Subject tool selects.

/// The Subject tool's two commits (exposure 0 and −6 EV displayed) agree at
/// least this far: intersection over union of the mask bytes at or above 128,
/// over every pixel. Declared here, before the case runs; never widened.
private let floatSubjectIoU = 0.95
private let floatSubjectFace = (x: 225, y: 113)
/// The plain backdrop in the top-right corner.
private let floatSubjectCorner = (x: 501, y: 10)
/// x, y, width, height of the suit's chest, and its gain.
private let floatSubjectHot = (x: 110, y: 300, width: 180, height: 130)
private let floatSubjectGain: Float = 4

/// `portrait` as an uncompressed little-endian RGBA 32-bit float TIFF at
/// `url`: each RGB code decoded by IEC 61966-2.1, alpha 1, RGB times
/// `floatSubjectGain` inside `floatSubjectHot`. The TIFF layout is a copy of
/// `writeFloatTIFF` in `scripts/jobs-probe/thumbnail_costs.swift`, the master.
func writeFloatPortrait(_ portrait: RasterImage, to url: URL) throws {
    let width = portrait.width, height = portrait.height
    var decode = [Float](repeating: 0, count: 256)
    for code in 0..<256 {
        let c = Double(code) / 255
        decode[code] = Float(c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4))
    }
    let hot = floatSubjectHot
    var pixels = [Float](repeating: 1, count: width * height * 4)
    var aboveWhite = 0
    let read: Bool? = portrait.withPlateBytes { bytes in
        for y in 0..<height {
            for x in 0..<width {
                let i = (y * width + x) * 4
                let inside = x >= hot.x && x < hot.x + hot.width && y >= hot.y && y < hot.y + hot.height
                let gain: Float = inside ? floatSubjectGain : 1
                for c in 0..<3 {
                    pixels[i + c] = decode[Int(bytes[i + c])] * gain
                    if pixels[i + c] > 1 { aboveWhite += 1 }
                }
            }
        }
        return true
    }
    try require(read == true, "The portrait has pixels to decode")
    try require(aboveWhite >= 5000, "Part of the subject is above white: \(aboveWhite) samples")

    var data = Data()
    func u16(_ v: Int) { var x = UInt16(v).littleEndian; data.append(Data(bytes: &x, count: 2)) }
    func u32(_ v: Int) { var x = UInt32(v).littleEndian; data.append(Data(bytes: &x, count: 4)) }
    let entries = 12
    let ifdEnd = 8 + 2 + entries * 12 + 4
    let bitsOffset = ifdEnd, formatOffset = ifdEnd + 8, pixelOffset = ifdEnd + 16
    let byteCount = width * height * 16
    data.append(contentsOf: [0x49, 0x49, 0x2A, 0x00])
    u32(8)
    u16(entries)
    // (tag, type 3 SHORT / 4 LONG, count, value or offset), ascending tags.
    let fields: [(Int, Int, Int, Int)] = [
        (256, 4, 1, width), (257, 4, 1, height), (258, 3, 4, bitsOffset), (259, 3, 1, 1),
        (262, 3, 1, 2), (273, 4, 1, pixelOffset), (277, 3, 1, 4), (278, 4, 1, height),
        (279, 4, 1, byteCount), (284, 3, 1, 1), (338, 3, 1, 2), (339, 3, 4, formatOffset),
    ]
    for (tag, type, count, value) in fields {
        u16(tag); u16(type); u32(count)
        if type == 3 && count == 1 { u16(value); u16(0) } else { u32(value) }
    }
    u32(0)
    for _ in 0..<4 { u16(32) }
    for _ in 0..<4 { u16(3) }
    // Native f32 is little-endian on arm64, the only target.
    pixels.withUnsafeBytes { data.append(contentsOf: $0) }
    try require(data.count == pixelOffset + byteCount, "the float TIFF is \(data.count) bytes")
    try data.write(to: url)
}

/// The float TIFF at `url`, installed the way `floatFixture` in
/// `scripts/jobs-probe/info_analysis.swift` installs one.
func floatPortraitDocument(_ url: URL) throws -> ImageDocument {
    let context = try PrecisionWorkContext()
    let precision = try RasterPrecisionDocument.openPreserving(
        url: url, workingLimit: context.limit, otherRetained: context.otherRetained)
    let source = try RasterDocument(precision: precision)
    try require(source.isFloat, "The portrait opened as a 32-bit float document")
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: 16, exportDither: "none"), markSaved: true)
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

private func floatSubjectItem(_ selector: String) -> NSMenuItem {
    NSMenuItem(title: selector, action: NSSelectorFromString(selector), keyEquivalent: "")
}

/// The current selection's mask bytes after a commit that must cover the
/// face (at least 128) and leave the backdrop corner clear (0–2).
private func floatSubjectMask(_ document: ImageDocument, width: Int, _ what: String) throws -> [UInt8] {
    guard let mask = document.canvasSelection?.maskBytes() else {
        throw ProbeFailure(description: "\(what) left no selection")
    }
    let face = mask[floatSubjectFace.y * width + floatSubjectFace.x]
    let corner = mask[floatSubjectCorner.y * width + floatSubjectCorner.x]
    try require(face >= 128 && corner <= 2,
                "\(what) covers the face and not the backdrop corner (face \(face), corner \(corner))")
    return mask
}

func verifyFloatSubjectTargets() throws {
    guard let directory = ProcessInfo.processInfo.environment["DR_SELECTION_PHOTO_DIR"] else {
        throw ProbeFailure(description: "DR_SELECTION_PHOTO_DIR is not set; pass --photos-dir")
    }
    let portrait = try RasterImage.open(url: URL(fileURLWithPath: directory).appendingPathComponent("portrait.png"))
    let width = portrait.width, height = portrait.height
    try require(width == 512 && height == 512, "The portrait is 512 × 512 (got \(width) × \(height))")
    let folder = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"]
        ?? NSTemporaryDirectory())
    let url = folder.appendingPathComponent("float-portrait.tiff")
    try writeFloatPortrait(portrait, to: url)
    let document = try floatPortraitDocument(url)
    defer { document.close() }
    let controller = try editor(document), canvas = try editor(document).canvas
    // The exposed image is rendered only from the retained projection, so it
    // must exist before the exposure is set below.
    try pumpUntil({ document.retainedPrecisionProjection != nil && canvas.image != nil }, seconds: 10,
                  message: "the float document's retained projection and display")
    let native = document.doc
    let face = CGPoint(x: floatSubjectFace.x, y: floatSubjectFace.y)
    AutomationAlerts.reset()

    // Gates.
    try require(DocumentPrecision.allows(.subject, document: document)
                && DocumentPrecision.allows(.objectSelect, document: document),
                "The Subject and Object Selection tools are admitted at 32-bit float")
    // The Magic Wand and Quick Selection compare the display copy at
    // float, the Magnetic Lasso traces it (rule M) and Select and Mask's edge
    // stage refines on it (rule E).
    try require(DocumentPrecision.allows(.wand, document: document)
                && DocumentPrecision.allows(.quickSelect, document: document),
                "Magic Wand and Quick Selection are admitted at 32-bit float")
    try require(DocumentPrecision.allows(.magneticLasso, document: document),
                "The Magnetic Lasso is admitted at 32-bit float")
    for selector in ["selectSubject:", "showSubjectTargets:", "selectPeople:", "showColorRange:",
                     "selectSimilar:", "selectGrow:", "refineSelection:"] {
        try require(controller.validatePrecisionItem(floatSubjectItem(selector)) == nil,
                    "\(selector) passes the float precision gate")
    }

    // Select ▸ Subject.
    controller.selectSubject(nil)
    _ = try floatSubjectMask(document, width: width, "Select Subject")
    try require(document.doc === native && !document.isDocumentEdited && AutomationAlerts.recorded.isEmpty,
                "Select Subject changes no pixel, leaves the document clean and shows no alert: "
                    + "\(AutomationAlerts.recorded)")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil, "One Undo reverses Select Subject")

    // The Subject tool, through the canvas's own mouse events.
    controller.selectSubjectTool(nil)
    try require(canvas.tool == .subject, "The Subject tool is selectable at 32-bit float")
    canvas.selectionFeather = 0
    var step = document.history.cursor
    canvas.mouseDown(with: try event(.leftMouseDown, face, canvas: canvas))
    try require(canvas.subjectSession.subject != nil, "A press on the face finds a subject at exposure 0")
    canvas.mouseUp(with: try event(.leftMouseUp, face, canvas: canvas))
    let s0 = try floatSubjectMask(document, width: width, "The Subject tool at exposure 0")
    try require(document.history.cursor == step + 1 && document.doc === native && !document.isDocumentEdited
                && AutomationAlerts.recorded.isEmpty,
                "The Subject tool commits one clean history step with no alert: \(AutomationAlerts.recorded)")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil, "One Undo reverses the Subject tool's selection")

    let displayed = canvas.image
    try controller.setViewExposure(-6)
    try pumpUntil({ controller.viewExposureDisplayed && canvas.proofImage != nil }, seconds: 10,
                  message: "the −6 EV image to reach the canvas")
    try require(canvas.image === displayed && canvas.proofImage !== canvas.image,
                "The exposed image goes to the proof slot and the document's display stays in place")
    step = document.history.cursor
    canvas.mouseDown(with: try event(.leftMouseDown, face, canvas: canvas))
    try require(canvas.subjectSession.subject != nil, "A press on the face finds a subject at −6 EV")
    canvas.mouseUp(with: try event(.leftMouseUp, face, canvas: canvas))
    let s6 = try floatSubjectMask(document, width: width, "The Subject tool at −6 EV")
    try require(document.history.cursor == step + 1 && document.doc === native && !document.isDocumentEdited
                && AutomationAlerts.recorded.isEmpty,
                "The Subject tool at −6 EV commits one clean history step with no alert: "
                    + "\(AutomationAlerts.recorded)")
    var both = 0, either = 0
    for (a, b) in zip(s0, s6) {
        if a >= 128 && b >= 128 { both += 1 }
        if a >= 128 || b >= 128 { either += 1 }
    }
    let iou = either == 0 ? 0 : Double(both) / Double(either)
    try require(iou >= floatSubjectIoU,
                "The view exposure does not change what the Subject tool selects: IoU \(iou)")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil, "One Undo reverses the −6 EV selection")
    try controller.setViewExposure(0)

    // Select ▸ All People.
    controller.selectPeople(nil)
    guard let people = document.canvasSelection?.maskBytes() else {
        throw ProbeFailure(description: "All People left no selection: \(AutomationAlerts.recorded)")
    }
    try require(people[floatSubjectFace.y * width + floatSubjectFace.x] >= 128
                && AutomationAlerts.recorded.isEmpty, "All People covers the face with no alert")
    document.undoManager?.undo()

    // The Object Selection tool's commit: a rectangle around the subject.
    let box = CGRect(x: 20, y: 10, width: 360, height: 500)
    var session = ObjectSelectionSession(capture: try SelectionCapture(document: document),
        mode: .rectangle, combine: .replace, feather: 0, point: box.origin)
    session.append(CGPoint(x: box.maxX, y: box.maxY), minimumDistance: 0.5)
    ActionRecorder.shared.start()
    controller.finishObjectSelection(session)
    let recorded = ActionRecorder.shared.stop()
    try require(recorded.count == 1 && recorded.first?.tool == "object_select"
                && recorded.first?.arguments["refine"] as? Bool == false,
                "Object Selection records one object_select step with refine false: "
                    + "\(recorded.map { ($0.tool, $0.arguments["refine"] ?? "nil") })")
    guard let object = document.canvasSelection?.maskBytes() else {
        throw ProbeFailure(description: "Object Selection left no selection: \(AutomationAlerts.recorded)")
    }
    var outside = 0
    for y in 0..<height {
        for x in 0..<width where object[y * width + x] != 0 && !box.contains(CGPoint(x: x, y: y)) {
            outside += 1
        }
    }
    try require(object[floatSubjectFace.y * width + floatSubjectFace.x] >= 128 && outside == 0
                && AutomationAlerts.recorded.isEmpty,
                "Object Selection selects the subject inside the rectangle with no alert "
                    + "(\(outside) pixels outside)")
    document.undoManager?.undo()

    // Select ▸ Objects… opens.
    _ = try SubjectTargetsSheetController(editor: controller, document: document)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, message: "the Objects picker's analysis")
    try require(AutomationAlerts.recorded.isEmpty, "The Objects picker initialises with no alert")
    print(String(format: "PASS: float Select Subject, Subject tool (exposure 0 and −6 EV, IoU %.4f), "
                    + "All People, Object Selection without refinement and the Objects picker", iou))
}
