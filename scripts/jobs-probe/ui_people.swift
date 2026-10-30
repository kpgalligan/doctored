import AppKit

// Select ▸ Select People… and Select ▸ All People (0af0e921) through the
// picker sheet's own rows, preview click and buttons, at 8 bits and on a
// 32-bit float conversion of the same photo.
//
// PRIVACY (the user's rule): the photos are the user's own. They are read in
// place, nothing derived from them is written anywhere, and the case prints and
// asserts numbers only — counts, centroids, coverage fractions and booleans.

/// The two face pins, as fractions of the photo's width and height.
private let peopleFirstPin = (x: 0.313, y: 0.594)
private let peopleSecondPin = (x: 0.719, y: 0.320)

/// A photo as an off-screen 8-bit document (`fixture`'s window handling).
private func peopleDocument(_ url: URL) throws -> ImageDocument {
    guard let document = ImageDocument.makeUntitled(with: try RasterImage.open(url: url)) else {
        throw ProbeFailure(description: "the photo did not open as a document")
    }
    document.undoManager?.groupsByEvent = false
    document.updateChangeCount(.changeCleared)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    document.undoManager?.removeAllActions()
    return document
}

/// The people picker `showPeopleTargets(_:)` just presented.
private func peopleSheet(_ controller: EditorViewController,
                         _ tag: String) throws -> SubjectTargetsSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showPeopleTargets(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? SubjectTargetsSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 120)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no people picker") }
    try require(sheet.model == .people, "\(tag) the picker is the people model")
    return sheet
}

private func peopleTable(_ sheet: SubjectTargetsSheetController, _ tag: String) throws -> NSTableView {
    guard let table = uiViews(sheet.view, as: NSTableView.self).first else {
        throw ProbeFailure(description: "\(tag) the picker has no table")
    }
    return table
}

private func peoplePress(_ sheet: SubjectTargetsSheetController, _ title: String?,
                         key: String? = nil, _ tag: String) throws {
    guard let button = buttons(sheet.view).first(where: {
        (title != nil && $0.title == title) || (key != nil && $0.keyEquivalent == key)
    }) else {
        throw ProbeFailure(description: "\(tag) the picker has no \(title ?? "default") button")
    }
    try uiSend(button, "\(tag) \(title ?? "Apply")")
}

/// The x centroid of a byte coverage, in pixels.
private func peopleCentroid(_ mask: [UInt8], width: Int) -> Double {
    var sum = 0.0, weight = 0.0
    for (i, value) in mask.enumerated() where value > 0 {
        sum += Double(i % width) * Double(value)
        weight += Double(value)
    }
    return weight > 0 ? sum / weight : -1
}

private struct PeopleReading {
    var count = 0
    var pins: [Int?] = []
    var centroids: [Double] = []
    var allFraction = 0.0
}

/// The picker and All People on `document`, at whatever depth it has.
private func peopleFlow(_ document: ImageDocument, _ tag: String) throws -> PeopleReading {
    let controller = try editor(document)
    guard let doc = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    let (width, height) = (doc.width, doc.height)
    let first = CGPoint(x: peopleFirstPin.x * Double(width), y: peopleFirstPin.y * Double(height))
    let second = CGPoint(x: peopleSecondPin.x * Double(width), y: peopleSecondPin.y * Double(height))
    let alerts = AutomationAlerts.recorded.count
    var reading = PeopleReading()

    /// One selection step: the undo name, the selection and the recorded step.
    func oneStep(_ name: String, _ steps: [ActionStep], tool: String, _ what: String) throws {
        try require(document.undoManager?.undoActionName == name,
                    "\(tag) \(what) is one \"\(name)\" step: "
                    + "\(document.undoManager?.undoActionName ?? "-")")
        try require(steps.count == 1 && steps[0].tool == tool,
                    "\(tag) \(what) records one \(tool) step: \(steps.map(\.tool))")
        document.undoManager?.undo()
        try require(document.doc === doc && document.canvasSelection == nil
                    && document.undoManager?.canUndo != true,
                    "\(tag) \(what): one undo restores the document and no selection")
    }

    // Rows, then Person 1 alone.
    var analysisMask: [UInt8] = []
    let picked = try uiRecorded {
        let sheet = try peopleSheet(controller, tag)
        let table = try peopleTable(sheet, tag)
        reading.count = sheet.numberOfRows(in: table)
        try require(reading.count == 2, "\(tag) the picker lists two people: \(reading.count)")
        for row in 0..<reading.count {
            let label = (sheet.tableView(table, viewFor: nil, row: row) as? NSTextField)?.stringValue
            try require(label?.hasPrefix("Person \(row + 1) ") == true,
                        "\(tag) row \(row) reads Person \(row + 1): \(label ?? "-")")
        }
        reading.pins = [sheet.analysis.subject(at: first), sheet.analysis.subject(at: second)]
        try require(reading.pins == [1, 2],
                    "\(tag) the pins fall on person 1 and person 2: \(reading.pins)")
        reading.centroids = try (1...reading.count).map { person in
            guard let mask = sheet.analysis.mask(subjects: [person]) else {
                throw ProbeFailure(description: "\(tag) person \(person) has no mask")
            }
            return peopleCentroid(mask, width: width)
        }
        table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        try require(sheet.selectedInstances == [1], "\(tag) only person 1 is chosen")
        analysisMask = sheet.analysis.mask(subjects: [1]) ?? []
        try peoplePress(sheet, nil, key: "\r", tag)
        try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied },
                      seconds: 120)
    }
    guard let selection = document.canvasSelection else {
        throw ProbeFailure(description: "\(tag) Apply set no selection: \(uiAlerts())")
    }
    let bytes = selection.maskBytes()
    try require(!analysisMask.isEmpty && bytes == analysisMask,
                "\(tag) the selection is person 1's analysis mask")
    let other = bytes[Int(second.y) * width + Int(second.x)]
    try require(other < 128, "\(tag) person 2's face pin is outside the selection: \(other)")
    try require(picked.first?.arguments["model"] as? String == "people"
                && picked.first?.arguments["instances"] as? [Int] == [1],
                "\(tag) the step records model people and instances [1]: "
                + "\(picked.map { "\($0.tool) \($0.arguments)" })")
    try oneStep("Select People", picked, tool: "select_subjects", "Apply with person 1")

    // The preview click toggles, and All People commits the aggregate.
    var aggregate: [UInt8] = []
    let all = try uiRecorded {
        let sheet = try peopleSheet(controller, tag)
        let table = try peopleTable(sheet, tag)
        guard let preview = uiViews(sheet.view, as: SelectionPreviewView.self).first,
              let click = preview.onPoint else {
            throw ProbeFailure(description: "\(tag) the picker has no clickable preview")
        }
        try require(sheet.selectedInstances == [1, 2], "\(tag) the picker opens on both people")
        click(first)
        try require(sheet.selectedInstances == [2], "\(tag) a click on person 1 takes them off: "
                    + "\(sheet.selectedInstances)")
        click(first)
        try require(sheet.selectedInstances == [1, 2], "\(tag) a second click puts them back: "
                    + "\(sheet.selectedInstances)")
        table.selectRowIndexes(IndexSet(integer: 1), byExtendingSelection: false)
        try require(sheet.selectedInstances == [2], "\(tag) row 2 alone chooses person 2")
        click(first)
        try require(sheet.selectedInstances == [1, 2], "\(tag) the click adds person 1 to [2]: "
                    + "\(sheet.selectedInstances)")
        aggregate = sheet.analysis.aggregateMask ?? []
        try peoplePress(sheet, "All People", tag)
        try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied },
                      seconds: 120)
    }
    try require(!aggregate.isEmpty && document.canvasSelection?.maskBytes() == aggregate,
                "\(tag) All People selects the analysis' aggregate")
    reading.allFraction = Double(aggregate.filter { $0 >= 128 }.count) / Double(aggregate.count)
    try oneStep("Select All People", all, tool: "select_people", "the picker's All People")

    // Select ▸ All People, the one-shot.
    let oneShot = uiRecorded { controller.selectPeople(nil) }
    try require(document.canvasSelection?.maskBytes() == aggregate,
                "\(tag) Select ▸ All People selects the same aggregate")
    try oneStep("Select All People", oneShot, tool: "select_people", "Select ▸ All People")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    return reading
}

/// "select people": the picker and the one-shot at 8 bits and at 32, and
/// a photo with no people refused.
func verifySelectPeople() throws {
    let tag = "[select people]"
    let photo = try uiFixture("multiple-people/IMG_9639.jpg")
    let document = try peopleDocument(photo)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    guard let width = document.doc?.width else { throw ProbeFailure(description: "\(tag) no photo") }
    let eight = try peopleFlow(document, "\(tag) 8-bit")
    _ = try agentOK("convert_precision", document, ["mode": "rgba32", "structure": "preserve"])
    document.undoManager?.removeAllActions()
    try require(document.doc?.isFloat == true, "\(tag) the photo converted to 32-bit float")
    let float = try peopleFlow(document, "\(tag) 32-bit")
    try require(float.count == eight.count && float.pins == eight.pins,
                "\(tag) the float picker finds the same people at the same pins: "
                + "\(float.count) \(float.pins)")
    for (a, b) in zip(eight.centroids, float.centroids) {
        try require(abs(a - b) <= 0.01 * Double(width),
                    "\(tag) each person's x centroid stays within 1% of the width: \(a) vs \(b)")
    }

    // A photo with no people: each command says so once and does nothing.
    let fur = try peopleDocument(try uiFixture("phase6/photos/fur.png"))
    defer { fur.close() }
    let furController = try editor(fur)
    guard let furBase = fur.doc else { throw ProbeFailure(description: "\(tag) no fur photo") }
    for (name, run) in [("Select People…", { furController.showPeopleTargets(nil) }),
                        ("All People", { furController.selectPeople(nil) })] {
        let alerts = AutomationAlerts.recorded.count
        let known = (furController.presentedViewControllers ?? []).count
        let steps = uiRecorded { run() }
        let said = AutomationAlerts.recorded.dropFirst(alerts)
        try require(said.count == 1 && said.first?.messageText == SubjectTargeting.noPeopleMessage,
                    "\(tag) \(name) on fur says \"\(SubjectTargeting.noPeopleMessage)\" once: "
                    + "\(said.map { "\($0.messageText) | \($0.informativeText)" })")
        try require((furController.presentedViewControllers ?? []).count == known,
                    "\(tag) \(name) on fur presents no sheet")
        try require(fur.doc === furBase && fur.canvasSelection == nil
                    && fur.undoManager?.canUndo != true && steps.isEmpty,
                    "\(tag) \(name) on fur sets no selection, adds no step and records nothing")
    }
    let fraction = { (value: Double) in String(format: "%.3f", value) }
    let centroid = { (values: [Double]) in
        values.map { String(format: "%.3f", $0 / Double(width)) }.joined(separator: ", ")
    }
    print("PASS: select people — on the 8-bit photo the picker lists \(eight.count) rows (Person 1, "
        + "Person 2) with the pins on persons \(eight.pins.map { $0 ?? 0 }); Apply with row 1 alone "
        + "selects person 1's analysis mask (the second pin outside) as one step recorded "
        + "select_subjects {model people, instances [1]}; the preview click toggles person 1 off, on "
        + "and onto [2]; All People and Select ▸ All People select the aggregate (coverage fraction "
        + "\(fraction(eight.allFraction))) as one \"Select All People\" step each; at 32 bits the "
        + "same \(float.count) people at the same pins, x centroids \(centroid(eight.centroids)) → "
        + "\(centroid(float.centroids)) of the width; a photo with no people says \""
        + "\(SubjectTargeting.noPeopleMessage)\" once for each command and changes nothing (0af0e921)")
}
