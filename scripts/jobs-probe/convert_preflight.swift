import AppKit

/// A private stored field of the sheet, by reflection (no product-only API).
private func sheetField(_ sheet: ConvertProfileSheetController, _ label: String) throws -> Any {
    guard let value = Mirror(reflecting: sheet).children.first(where: { $0.label == label })?.value
    else { throw ProbeFailure(description: "Convert sheet field \(label) is missing") }
    return value
}

/// Runs `body`, naming the step in a failure.
private func labelled(_ step: String, _ body: () throws -> Void) throws {
    do { try body() } catch { throw ProbeFailure(description: "\(step): \(error)") }
}

/// Whether an optional read through `Mirror` is nil.
private func isNil(_ value: Any) -> Bool {
    let mirror = Mirror(reflecting: value)
    return mirror.displayStyle == .optional && mirror.children.isEmpty
}

/// Selects `tag` in the sheet's real adjustment-policy popup and fires the
/// popup's own action, as a menu choice does.
private func choosePolicy(_ popup: NSPopUpButton, tag: Int) throws {
    guard let action = popup.action else { throw ProbeFailure(description: "Policy popup has no action") }
    popup.selectItem(withTag: tag)
    try require(NSApp.sendAction(action, to: popup.target, from: popup), "Policy popup action was not delivered")
}

/// An 8-bit preserve-appearance preflight that can bake runs on the
/// worker while the Convert sheet stays up, and runs inline when the worker
/// is already occupied rather than refusing; Esc cancels that wait,
/// leaving the sheet up with Apply disabled.
func verifyConvertPreflightSheet() throws {
    let document = try fixture(width: 3000, height: 2000)
    defer { document.close() }
    let controller = try editor(document)
    guard let meta = AdjustmentLayerPayload(op: .curves, params: [:]).json() else {
        throw ProbeFailure(description: "Curves meta")
    }
    for index in 0..<3 {
        withUndoGroup(document.undoManager) {
            document.applyEdit("Add Curves", record: .notACommand) {
                $0.addingAdjustmentLayer(active: index, name: "Curves \(index + 1)", meta: meta,
                                         selection: nil)
            }
        }
    }
    try require(document.doc?.layerCount == 4, "Three curves layers were added")
    controller.convertToProfile(nil)
    try labelled("the Convert sheet was never presented") {
        try pumpUntil({ controller.presentedViewControllers?.contains { $0 is ConvertProfileSheetController } == true },
                      seconds: 5)
    }
    guard let sheet = controller.presentedViewControllers?.first(where: { $0 is ConvertProfileSheetController })
        as? ConvertProfileSheetController else { throw ProbeFailure(description: "No convert sheet") }
    _ = sheet.view
    // The sheet opens on the document's own profile, which is no change at
    // any policy — so pick a different destination first.
    guard let destinations = try sheetField(sheet, "profilePopup") as? NSPopUpButton,
          let destinationAction = destinations.action,
          let p3 = WorkingSpace.allCases.firstIndex(of: .displayP3) else {
        throw ProbeFailure(description: "Convert sheet destination popup")
    }
    destinations.selectItem(withTag: p3)
    try require(NSApp.sendAction(destinationAction, to: destinations.target, from: destinations),
                "Destination popup action was not delivered")
    try labelled("the Display P3 preflight never settled") {
        try pumpUntil({ !BackgroundWork.isWaiting && !BackgroundWork.occupied }, seconds: 10)
    }
    guard let popup = try sheetField(sheet, "adjustmentsPopup") as? NSPopUpButton,
          let policyNote = try sheetField(sheet, "policyNote") as? NSTextField,
          let apply = try sheetField(sheet, "applyButton") as? NSButton else {
        throw ProbeFailure(description: "Convert sheet controls")
    }
    let keepTag = AdjustmentPolicy.allCases.firstIndex(of: .keep) ?? 0
    let preserveTag = AdjustmentPolicy.allCases.firstIndex(of: .preserveAppearance) ?? 1
    let baked = "will be baked into raster layers"

    // (a) The worker wait, with the sheet attached.
    var ticks = 0
    var sawWait = false
    let timer = Timer(timeInterval: 0.005, repeats: true) { _ in
        ticks += 1
        if BackgroundWork.isWaiting && BackgroundWork.occupied { sawWait = true }
    }
    RunLoop.main.add(timer, forMode: .common)
    try choosePolicy(popup, tag: preserveTag)
    timer.invalidate()
    try require(sawWait, "(a) the preflight never ran as a worker wait")
    try require(ticks > 1, "(a) main run loop stayed responsive: ticks=\(ticks)")
    let refusal = try sheetField(sheet, "preflightRefusal")
    try require(isNil(refusal), "(a) preflight refused: \(refusal)")
    try require(policyNote.stringValue.contains(baked), "(a) baked notice: \(policyNote.stringValue)")
    try require(apply.isEnabled, "(a) Apply is enabled")
    try require(controller.presentedViewControllers?.contains { $0 === sheet } == true,
                "(a) the sheet is still presented")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "(a) the worker slot was released")

    // (b) An occupied worker: the preflight runs inline, never refuses.
    let hold = try RasterJobControl()
    let holder = try WorkerTask<Bool> {
        while !hold.isCancelled { Thread.sleep(forTimeInterval: 0.005) }
        return true
    }
    var sawWaitWhileHeld = false
    let watch = Timer(timeInterval: 0.005, repeats: true) { _ in
        if BackgroundWork.isWaiting { sawWaitWhileHeld = true }
    }
    RunLoop.main.add(watch, forMode: .common)
    do {
        try choosePolicy(popup, tag: keepTag)
        try choosePolicy(popup, tag: preserveTag)
    } catch {
        watch.invalidate()
        hold.cancel()
        try? pumpUntil({ holder.take() != nil })
        throw error
    }
    watch.invalidate()
    let heldRefusal = try sheetField(sheet, "preflightRefusal")
    let heldNotice = policyNote.stringValue
    hold.cancel()
    try labelled("(b) the holder never finished") {
        try pumpUntil({ holder.take() != nil }, seconds: 5)
    }
    try require(!sawWaitWhileHeld, "(b) an occupied worker started a wait")
    try require(isNil(heldRefusal), "(b) an occupied worker became a refusal: \(heldRefusal)")
    try require(heldNotice.contains(baked), "(b) baked notice: \(heldNotice)")
    try require(!BackgroundWork.occupied, "(b) holder released the worker")

    // (c) Esc during the baking preflight's wait cancels the check.
    // The sheet stays up, Apply is disabled, the note says so and the
    // worker slot is free.
    try choosePolicy(popup, tag: keepTag)
    guard let note = try sheetField(sheet, "note") as? NSTextField else {
        throw ProbeFailure(description: "Convert sheet note")
    }
    var posted = false
    let escape = Timer(timeInterval: 0.002, repeats: true) { _ in
        guard !posted, BackgroundWork.isWaiting, BackgroundWork.occupied,
              let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                  timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: 0,
                  context: nil, characters: "\u{1b}", charactersIgnoringModifiers: "\u{1b}",
                  isARepeat: false, keyCode: 53) else { return }
        posted = true
        NSApp.postEvent(event, atStart: true)
    }
    RunLoop.main.add(escape, forMode: .common)
    try choosePolicy(popup, tag: preserveTag)
    escape.invalidate()
    try require(posted, "(c) the preflight never ran as a worker wait to cancel")
    let cancelled = try sheetField(sheet, "preflightRefusal")
    try require((cancelled as? String) == ConvertProfileSheetController.preflightCancelled,
                "(c) the cancelled check's refusal: \(cancelled)")
    try require(note.stringValue == ConvertProfileSheetController.preflightCancelled,
                "(c) the note reads the cancel: \(note.stringValue)")
    try require(!apply.isEnabled, "(c) Apply is disabled after a cancelled check")
    try require(controller.presentedViewControllers?.contains { $0 === sheet } == true,
                "(c) the sheet is still presented")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "(c) the worker slot was released")

    try require(NSApp.sendAction(Selector(("cancelClicked:")), to: sheet, from: nil),
                "the sheet's cancelClicked: was delivered")
    print("PASS: convert preflight runs on the worker with the sheet up (ticks=\(ticks)), inline "
        + "when occupied, and Esc cancels it with the sheet up and Apply disabled")
}

/// At 16 bits a preserve-appearance preflight that can bake is a
/// worker wait with the Convert sheet up. Apply then lands with three
/// "(baked)" layers under the Convert's own plan, which the last step
/// asserts.
func verifyConvertPreflightSheet16() throws {
    let curves = try PrecisionAdjustment(
        op: .curves, params: ["rgb": [[0, 0], [128, 160], [255, 255]]])
    let document = try precisionFixture(width: 2000, height: 1500) { source in
        var next = source
        for index in 0..<3 {
            guard let added = try next.performingPrecisionAdjustment(.add(
                name: "Curves \(index + 1)", adjustment: curves, opacity: 1,
                anchor: index, position: .above)) else {
                throw ProbeFailure(description: "Curves \(index + 1) was not added")
            }
            next = added
        }
        return next
    }
    defer { document.close() }
    try require(document.doc?.isHighDepth == true && document.doc?.layerCount == 4,
                "a 16-bit document with three Curves layers")
    let controller = try editor(document)
    controller.convertToProfile(nil)
    try labelled("the Convert sheet was never presented") {
        try pumpUntil({ controller.presentedViewControllers?.contains { $0 is ConvertProfileSheetController } == true },
                      seconds: 5)
    }
    guard let sheet = controller.presentedViewControllers?.first(where: { $0 is ConvertProfileSheetController })
        as? ConvertProfileSheetController else { throw ProbeFailure(description: "No convert sheet") }
    _ = sheet.view
    guard let destinations = try sheetField(sheet, "profilePopup") as? NSPopUpButton,
          let destinationAction = destinations.action,
          let p3 = WorkingSpace.allCases.firstIndex(of: .displayP3) else {
        throw ProbeFailure(description: "Convert sheet destination popup")
    }
    destinations.selectItem(withTag: p3)
    try require(NSApp.sendAction(destinationAction, to: destinations.target, from: destinations),
                "Destination popup action was not delivered")
    try labelled("the Display P3 preflight never settled") {
        try pumpUntil({ !BackgroundWork.isWaiting && !BackgroundWork.occupied }, seconds: 10)
    }
    guard let popup = try sheetField(sheet, "adjustmentsPopup") as? NSPopUpButton,
          let apply = try sheetField(sheet, "applyButton") as? NSButton else {
        throw ProbeFailure(description: "Convert sheet controls")
    }
    let preserveTag = AdjustmentPolicy.allCases.firstIndex(of: .preserveAppearance) ?? 1
    var sawWait = false
    let timer = Timer(timeInterval: 0.002, repeats: true) { _ in
        if BackgroundWork.isWaiting && BackgroundWork.occupied { sawWait = true }
    }
    RunLoop.main.add(timer, forMode: .common)
    try choosePolicy(popup, tag: preserveTag)
    timer.invalidate()
    try require(sawWait, "the 16-bit preflight never ran as a worker wait")
    let refusal = try sheetField(sheet, "preflightRefusal")
    try require(isNil(refusal), "the 16-bit preflight refused: \(refusal)")
    try require(apply.isEnabled, "Apply is enabled")
    try require(controller.presentedViewControllers?.contains { $0 === sheet } == true,
                "the sheet is still presented")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "the worker slot was released")
    // Apply plans with the Convert's own plan, which counts the bakes,
    // so it lands: the document turns Display P3 and every Curves layer is
    // "… (baked)".
    let base = document.doc
    let alerts = AutomationAlerts.recorded.count
    try require(NSApp.sendAction(Selector(("applyClicked:")), to: sheet, from: nil),
                "the sheet's applyClicked: was delivered")
    try labelled("Apply never landed") {
        // Both the publication and the bake report, or an error alert: the
        // report may be recorded in a later turn than the publication.
        try pumpUntil({
            let since = AutomationAlerts.recorded.suffix(from: alerts)
            let failed = since.contains { $0.kind == .errorModal || $0.kind == .errorSheet }
            return !BackgroundWork.isWaiting && !BackgroundWork.occupied
                && ((document.doc !== base && !since.isEmpty) || failed)
        }, seconds: 30)
    }
    let presented = AutomationAlerts.recorded.suffix(from: alerts)
    try require(!presented.contains { $0.kind == .errorModal || $0.kind == .errorSheet }
                    && presented.contains { $0.messageText == "Adjustment layers were baked" },
                "Apply presented only the bake report: \(presented.map(\.messageText))")
    try require(document.doc !== base
                    && document.doc?.iccProfile == RasterProfile.builtin(.displayP3),
                "Apply turned the document Display P3")
    let names = (0..<(document.doc?.layerCount ?? 0)).map { document.doc?.layerInfo($0)?.name ?? "" }
    try require(names.filter { $0.hasSuffix(" (baked)") }.count == 3,
                "three layers were baked: \(names)")
    try require(document.isDocumentEdited && document.undoManager?.canUndo == true,
                "the Apply dirtied the document and left an undo step")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "the worker slot was released after the Apply")
    print("PASS: convert preflight 16 runs on the worker with the sheet up; Apply lands with "
        + "three (baked) layers")
}
