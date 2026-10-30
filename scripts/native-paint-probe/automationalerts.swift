import AppKit

/// The delegate the error-sheet and close-review steps answer through: each
/// records the flag AppKit's did-present / should-close callback carried.
final class AutomationAlertsProbeDelegate: NSObject {
    private(set) var calls: [Bool] = []
    private(set) var closeAnswers: [Bool] = []

    @objc func didPresentError(_ didRecover: Bool, contextInfo: UnsafeMutableRawPointer?) {
        calls.append(didRecover)
    }

    @objc func document(_ document: NSDocument, shouldClose: Bool,
                        contextInfo: UnsafeMutableRawPointer?) {
        closeAnswers.append(shouldClose)
    }
}

/// `AutomationAlerts` under `DR_AUTOMATION_ALERTS=record`: a real canvas
/// alert (the Position lock's refusal on a Move drag) is answered and
/// recorded without a modal window, a sheet alert and an error sheet answer
/// on the next run-loop turn with nothing attached, the probe's own
/// `presentError` override still wins over the hook, and an edited
/// document's close review is cancelled (never saved or discarded) while a
/// clean one passes through.
func automationAlertsUI() throws -> [String: Any] {
    try require(AutomationAlerts.isEnabled && AutomationAlerts.isInstalled,
                "AutomationAlerts is not installed in the probe")
    AutomationAlerts.reset()
    guard let app = NSApp as? ProbeApplication else {
        throw DoctoredCoreError(message: "Probe application missing")
    }
    let (document, editor, _) = try openDocument(width: 48, height: 36)
    defer { document.close(); pump() }
    guard let window = editor.view.window else {
        throw DoctoredCoreError(message: "The owned editor has no window")
    }

    // (a) A real canvas alert: a Move drag over a Position-locked layer.
    _ = try geometryCall("set_layer_lock", ["index": 0, "locks": ["position"]], document)
    guard let refusal = document.doc?.lockRefusal(0, kind: DR_EDIT_POSITION) else {
        throw DoctoredCoreError(message: "The Position lock did not land")
    }
    let lockedDoc = document.doc
    let lockedHistory = document.history.entries.count
    editor.selectTool(.move); pump()
    try send(editor.canvas, .leftMouseDown, CGPoint(x: 14, y: 14))
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 19, y: 17))
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 19, y: 17))
    pump()
    let lockAlerts = AutomationAlerts.recorded
    try require(lockAlerts.count == 1 && lockAlerts[0].kind == .alertModal
        && lockAlerts[0].messageText == refusal.message
        && lockAlerts[0].informativeText.contains("Unlock")
        && lockAlerts[0].answer == "OK",
        "Position lock alert not recorded once: \(lockAlerts)")
    try require(NSApp.modalWindow == nil && editor.precisionMoveGesture == nil
        && document.history.entries.count == lockedHistory && document.doc === lockedDoc,
        "The refused Move left a modal window, a gesture or a history step")
    document.undoManager?.undo(); pump()
    try require(document.doc?.lockRefusal(0, kind: DR_EDIT_POSITION) == nil
        && !document.isDocumentEdited, "Undoing the lock did not restore the clean document")

    // (b) A sheet alert: no sheet attached, the handler runs on a later turn.
    let sheetAlert = NSAlert()
    sheetAlert.messageText = "Automation sheet probe"
    sheetAlert.informativeText = "Answered without a sheet."
    sheetAlert.addButton(withTitle: "First")
    sheetAlert.addButton(withTitle: "Second")
    var sheetResponses: [NSApplication.ModalResponse] = []
    sheetAlert.beginSheetModal(for: window) { sheetResponses.append($0) }
    try require(sheetResponses.isEmpty && window.attachedSheet == nil,
                "The sheet alert answered synchronously or attached a sheet")
    try waitUntil({ sheetResponses == [.alertFirstButtonReturn] },
                  "The sheet alert was not answered with its first button")
    let sheetRecords = AutomationAlerts.recorded
    try require(sheetRecords.count == 2 && sheetRecords[1].kind == .alertSheet
        && sheetRecords[1].buttonTitles == ["First", "Second"]
        && sheetRecords[1].answer == "First"
        && sheetRecords[1].messageText == "Automation sheet probe",
        "Sheet alert record wrong: \(sheetRecords)")

    // (c) An error sheet answers its delegate "not recovered".
    let probe = AutomationAlertsProbeDelegate()
    let sheetError = NSError(domain: "AutomationAlertsProbe", code: 1,
                             userInfo: [NSLocalizedDescriptionKey: "Automation error sheet probe"])
    NSApp.presentError(sheetError, modalFor: window, delegate: probe,
                       didPresent: #selector(AutomationAlertsProbeDelegate.didPresentError(_:contextInfo:)),
                       contextInfo: nil)
    try require(probe.calls.isEmpty && window.attachedSheet == nil,
                "The error sheet answered synchronously or attached a sheet")
    try waitUntil({ probe.calls == [false] }, "The error sheet delegate was not answered")
    let errorRecords = AutomationAlerts.recorded
    try require(errorRecords.count == 3 && errorRecords[2].kind == .errorSheet
        && errorRecords[2].messageText == "Automation error sheet probe"
        && errorRecords[2].answer == "not recovered",
        "Error sheet record wrong: \(errorRecords)")

    // (d) The probe's own presentError override still wins over the hook.
    app.errors.removeAll()
    let modalError = NSError(domain: "AutomationAlertsProbe", code: 2,
                             userInfo: [NSLocalizedDescriptionKey: "Automation modal error probe"])
    let recovered = NSApp.presentError(modalError)
    try require(!recovered && app.errors == ["Automation modal error probe"]
        && AutomationAlerts.recorded.count == 3,
        "ProbeApplication.presentError did not win over the hook: \(app.errors)")

    // (e) reset() empties the record.
    AutomationAlerts.reset()
    try require(AutomationAlerts.recorded.isEmpty, "reset() left records")

    // (f) The unsaved-changes review is cancelled; a clean close passes through.
    _ = try geometryCall("duplicate_layer", ["index": 0], document)
    try require(document.isDocumentEdited, "Duplicate Layer did not dirty the document")
    let shouldClose = #selector(AutomationAlertsProbeDelegate.document(_:shouldClose:contextInfo:))
    func closeRecords() -> [AutomationAlertRecord] {
        AutomationAlerts.recorded.filter { $0.kind == .documentCloseReview }
    }
    document.canClose(withDelegate: probe, shouldClose: shouldClose, contextInfo: nil)
    try require(probe.closeAnswers.isEmpty, "The close review answered synchronously")
    try waitUntil({ probe.closeAnswers == [false] }, "The close review was not cancelled")
    try require(AutomationAlerts.recorded.count == 1 && closeRecords().count == 1
        && closeRecords()[0].messageText == document.displayName
        && closeRecords()[0].answer == "cancel",
        "Close review record wrong: \(AutomationAlerts.recorded)")
    window.performClose(nil)
    try waitUntil({ closeRecords().count == 2 }, "performClose did not reach the close review")
    pump()
    try require(window.isVisible && !document.hasClosed
        && NSDocumentController.shared.documents.contains(document)
        && document.isDocumentEdited && window.attachedSheet == nil && NSApp.modalWindow == nil,
        "performClose on an edited document closed, saved or showed a sheet")
    document.undoManager?.undo(); pump()
    try require(!document.isDocumentEdited, "Undoing the duplicate did not restore the clean document")
    let passThroughCount = AutomationAlerts.recorded.count
    document.canClose(withDelegate: probe, shouldClose: shouldClose, contextInfo: nil)
    try waitUntil({ probe.closeAnswers == [false, true] }, "A clean close did not pass through")
    try require(AutomationAlerts.recorded.count == passThroughCount,
                "A clean close was recorded")

    return ["case": "automation-alerts-ui", "passed": true,
            "lock_alert": refusal.message, "close_answers": probe.closeAnswers,
            "error_sheet_calls": probe.calls]
}
