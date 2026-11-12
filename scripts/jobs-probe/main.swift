import AppKit
import Darwin

struct ProbeFailure: Error, CustomStringConvertible { let description: String }
func require(_ value: @autoclosure () -> Bool, _ message: String) throws {
    if !value() { throw ProbeFailure(description: message) }
}
let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_JOBS_PROBE_DIR"] ?? NSTemporaryDirectory())

func pumpUntil(_ predicate: () -> Bool, seconds: Double = 40) throws {
    let deadline = Date().addingTimeInterval(seconds)
    while !predicate() {
        if Date() > deadline { throw ProbeFailure(description: "Timed out waiting for worker: \(DocumentJobs.active?.report ?? [:])") }
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    }
}

func fixture(width: Int = 1200, height: Int = 1000) throws -> ImageDocument {
    var bytes = [UInt8](repeating: 255, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let p = (y * width + x) * 4
            bytes[p] = UInt8(40 + (x * 3 + y * 5) % 170)
            bytes[p + 1] = UInt8(40 + (x * 7 + y * 11) % 170)
            bytes[p + 2] = UInt8(40 + (x * 13 + y * 3) % 170)
        }
    }
    guard let image = RasterImage.from(rgba: bytes, width: width, height: height),
          let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "Fixture allocation")
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

func editor(_ document: ImageDocument) throws -> EditorViewController {
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw ProbeFailure(description: "Editor window")
    }
    _ = editor.view
    return editor
}

func select(_ document: ImageDocument, side: Int) throws -> [UInt8] {
    guard let source = document.doc else { throw ProbeFailure(description: "No source") }
    var mask = [UInt8](repeating: 0, count: source.width * source.height)
    let x0 = (source.width - side) / 2
    let y0 = (source.height - side) / 2
    for y in y0..<(y0 + side) {
        for x in x0..<(x0 + side) { mask[y * source.width + x] = 255 }
    }
    document.setCanvasSelection(CanvasSelection(shape: .mask(mask), canvasWidth: source.width,
                                                canvasHeight: source.height))
    document.undoManager?.removeAllActions()
    return mask
}

func buttons(_ view: NSView) -> [NSButton] {
    (view as? NSButton).map { [$0] } ?? view.subviews.flatMap(buttons)
}

/// The sheet's captured review owner, by reflection over its private slot
/// (`scripts/content-aware-probe/main.swift`'s helper): no product-only API.
func fillReview(_ sheet: ContentAwareFillSheetController) throws -> ContentAwareFillReview {
    guard let slot = Mirror(reflecting: sheet).children.first(where: { $0.label == "review" })?.value,
          let owner = Mirror(reflecting: slot).children.first?.value as? ContentAwareFillReview else {
        throw ProbeFailure(description: "Sheet's captured review owner is missing")
    }
    return owner
}

func button(_ view: NSView, identifier: String) -> NSButton? {
    buttons(view).first { $0.identifier?.rawValue == identifier }
}

/// Phase 9E (694c39f) made the fill sheet prepare-then-apply: Apply publishes
/// an already prepared review and starts no job, so the cancellable work is
/// the review's PREPARE worker. This drives that worker through the real
/// sheet and cancels it with the sheet's own Cancel button.
func verifyRealSheetAndCancel() throws {
    let document = try fixture()
    defer { document.close() }
    let controller = try editor(document)
    _ = try select(document, side: 700)
    let base = document.doc
    let revision = document.revision
    controller.contentAwareFill(nil)
    try pumpUntil({ controller.presentedViewControllers?.contains { $0 is ContentAwareFillSheetController } == true },
                  seconds: 5)
    guard let sheet = controller.presentedViewControllers?.first(where: { $0 is ContentAwareFillSheetController })
        as? ContentAwareFillSheetController else { throw ProbeFailure(description: "No fill sheet") }
    _ = sheet.view
    let review = try fillReview(sheet)
    if review.state == "idle" {
        guard let prepare = button(sheet.view, identifier: "caf.prepare") else {
            throw ProbeFailure(description: "Actual Prepare Final Result button")
        }
        prepare.performClick(nil)
    }
    var ticks = 0
    let timer = Timer(timeInterval: 0.005, repeats: true) { _ in ticks += 1 }
    RunLoop.main.add(timer, forMode: .common)
    defer { timer.invalidate() }
    try pumpUntil({ (review.state == "running" && BackgroundWork.occupied && ticks >= 5)
                    || !["idle", "running"].contains(review.state) })
    try require(review.state == "running" && BackgroundWork.occupied,
                "Large sheet preparation stayed running until Cancel: \(review.state) \(review.message)")
    guard let cancel = button(sheet.view, identifier: "caf.cancel") else {
        throw ProbeFailure(description: "Actual sheet Cancel button")
    }
    let cancelled = Date()
    cancel.performClick(nil)
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 2)
    try require(review.state == "cancelled", "UI cancellation: \(review.report)")
    try require(document.doc === base && document.revision == revision, "Cancel preserved revision")
    try require(document.undoManager?.canUndo != true && !document.isDocumentEdited, "Cancel did not create undo/dirty")
    try require(BackgroundWork.scratchEstimate == 0, "Cancelled preparation released its scratch")
    try require(ticks > 1, "Main run loop stayed responsive")
    print("PASS: actual fill sheet Prepare/progress/Cancel; responsive ticks=\(ticks), cancellation_ms=\(Date().timeIntervalSince(cancelled) * 1000)")
}

func verifyCompletionAndStale() throws {
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    _ = try editor(document)
    let mask = try select(document, side: 120)
    guard let base = document.doc else { throw ProbeFailure(description: "No base") }
    let request = try ContentAwareFillWork(source: base, layer: 0, coverage: mask,
                                           settings: ContentAwareFillSettings(ring: 48, seed: 17)).command.request
    let expected = try base.contentAwareFillEdit(request, coverage: mask, control: RasterJobControl())
    _ = try DocumentJobs.start(document: document, layer: 0, title: "Content-Aware Fill",
        scratch: UInt64(mask.count), record: .notACommand,
        showProgress: true) { captured, control in
        try captured.contentAwareFillEdit(request, coverage: mask, control: control)
    }
    guard let job = DocumentJobs.active else { throw ProbeFailure(description: "No success job") }
    try pumpUntil({ job.state != "running" })
    try require(job.state == "completed", "UI completion: \(job.report)")
    try require(document.doc?.layerPixels(0) == expected?.layerPixels(0), "UI worker equals the direct edit-export result")
    try require(document.isDocumentEdited, "Successful job dirties file")
    document.undoManager?.undo()
    try require(document.doc === base && !document.isDocumentEdited, "One undo restores saved revision")
    try require(document.undoManager?.canUndo != true, "Exactly one undo entry")
    document.undoManager?.redo()
    try require(document.doc?.layerPixels(0) == expected?.layerPixels(0), "Redo restores result")

    // Generic start avoids displaying the stale alert while testing the same
    // central completion guard. Undo returning to base still changes revision.
    // The raw `document.undoManager?.undo()` below deliberately still pops a
    // step during a job: that is now only the path of ⌘Z or History over an
    // agent job, and the monotonic-revision guard is what protects it. The UI
    // keys, History and MCP undo/redo/restore_history no longer reach it while
    // a UI job runs (`job_keys.swift`, `job_followups.swift`); this check's
    // behaviour is unchanged.
    document.undoManager?.undo()
    let stale = try DocumentJobs.start(document: document, layer: 0, title: "Stale Probe",
        scratch: UInt64(mask.count), record: .notACommand, showProgress: false) { captured, control in
        try captured.contentAwareFillEdit(request, coverage: mask, control: control)
    }
    withUndoGroup(document.undoManager) {
        document.applyEdit("Rename", record: .notACommand) { try $0.withLayerName(0, "Changed") }
    }
    document.undoManager?.undo()
    try require(document.doc === base, "Returned to exact captured handle")
    let before = document.revision
    try pumpUntil({ stale.state != "running" })
    try require(stale.state == "stale" && document.revision == before && document.doc === base,
                "Monotonic revision rejects mutation then undo")
    print("PASS: UI final pixels, exactly one undo/redo, saved-state identity and stale mutation/undo")
}

func verifySpotGesture() throws {
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    let controller = try editor(document)
    let mask = try select(document, side: 120)
    guard let base = document.doc else { throw ProbeFailure(description: "No spot base") }
    controller.selectSpotHealTool(nil)
    try require(controller.canvas.onStrokeBegin?() == true, "Actual spot stroke begin")
    var overlay = [UInt8](repeating: 0, count: mask.count * 4)
    for i in mask.indices { for c in 0..<4 { overlay[i * 4 + c] = mask[i] } }
    overlay.withUnsafeBufferPointer { buffer in
        if let data = buffer.baseAddress { controller.canvas.onCommitStrokeOverlay?(data, "Spot Healing") }
    }
    controller.canvas.onStrokeEnd?("Spot Healing")
    guard let job = DocumentJobs.active else { throw ProbeFailure(description: "Spot gesture did not start worker") }
    try require(!document.isLiveEditing && document.doc === base, "Spot releases live gesture before worker")
    try pumpUntil({ job.state != "running" })
    try require(job.state == "completed", "Spot completed: \(job.report)")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true, "Spot one undo")
    print("PASS: actual spot-heal gesture callback, worker completion and one undo")
}

func verifyCapturedSave() throws {
    let document = try fixture(width: 2400, height: 1600)
    defer { document.close() }
    guard let captured = document.doc else { throw ProbeFailure(description: "No save source") }
    let destination = directory.appendingPathComponent("captured.rz")
    var edited = false
    let timer = Timer(timeInterval: 0.01, repeats: false) { _ in
        withUndoGroup(document.undoManager) {
            document.applyEdit("During Save", record: .notACommand) { try $0.withLayerName(0, "Newer edit") }
        }
        edited = true
    }
    RunLoop.main.add(timer, forMode: .common)
    var finished = false
    var failure: Error?
    document.save(to: destination, ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation) {
        failure = $0
        finished = true
    }
    try pumpUntil({ finished })
    if let failure { throw failure }
    try require(edited, "An edit landed while the worker saved")
    try require(document.isDocumentEdited, "Newer edit remains dirty after save completion")
    try require(document.windowControllers.first?.window?.isDocumentEdited == true,
                "Window dirty indicator follows the newer revision")
    let reopened = try RasterDocument.open(url: destination)
    try require(reopened.layerInfo(0)?.name == captured.layerInfo(0)?.name,
                "File contains captured revision rather than newer edit")
    try require(reopened.layerPixels(0) == captured.layerPixels(0), "Captured save pixel roundtrip")
    document.undoManager?.undo()
    try require(document.doc === captured && !document.isDocumentEdited, "Undo to saved revision is clean")
    document.undoManager?.redo()
    try require(document.isDocumentEdited, "Redo newer edit is dirty")

    let preserved = try Data(contentsOf: destination)
    let bad = directory.appendingPathComponent("missing-parent/out.rz")
    var failed = false
    document.save(to: bad, ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation) {
        failed = $0 != nil
    }
    try pumpUntil({ failed })
    let retained = try Data(contentsOf: destination)
    try require(retained == preserved && document.isDocumentEdited,
                "Failed save retains previous file and dirty state")
    print("PASS: real NSDocument save captures one revision, preserves newer dirty/undo/redo and survives write failure")
}

do {
    _ = NSApplication.shared
    AutomationAlerts.installIfEnabled()
    // Run the main loop once: until it has, a modal session's timers never
    // fire, so a case that runs first could not click a progress panel's Cancel.
    RunLoop.main.run(until: Date())
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    // Every Copy the cases make goes to the runner's private pasteboard.
    guard let pasteboardName = ProcessInfo.processInfo.environment[Clipboard.environmentName],
          !pasteboardName.isEmpty else {
        fputs("FAIL: the runner set no private pasteboard (DR_PASTEBOARD_NAME)\n", stderr)
        exit(1)
    }
    let checks: [(String, () throws -> Void)] = [
        // First: the 37 MP rotate and the 22 MP ⌘T must discriminate the
        // largest stage from the old sum against a baseline residency, and
        // the 13 MP straighten must fit beside what later cases retain.
        ("transform admission", verifyTransformLargeAdmission),
        ("canvas geometry 13 MP crop", verifyCanvasGeometryLargeCrop),
        // Sized against the 2 GiB pin too (a 16 MP float ⌘T's commit
        // plan must admit, and its 65 MP twin must not), so it runs before
        // the cases that leave documents and caches behind.
        ("preview fallback", verifyPreviewFallback),
        ("sheet/cancel", verifyRealSheetAndCancel), ("completion/stale", verifyCompletionAndStale),
        ("spot gesture", verifySpotGesture), ("captured save", verifyCapturedSave),
        ("transforms", verifyTransformJobs), ("job reporting", verifyJobReporting),
        ("escape", verifyEscapeCancelsModalWait), ("convert preflight", verifyConvertPreflightSheet),
        ("convert preflight 16", verifyConvertPreflightSheet16),
        ("transform cancel", verifyTransformCancel),
        ("agent worker wait", verifyAgentWaitsForWorker),
        ("agent serve-thread wait", verifyAgentServeThreadWait),
        ("agent release mid-call", verifyAgentReleaseMidCall),
        ("mcp settings pane", verifyMCPSettingsPane),
        ("commit during preview", verifyCommitDuringPreview),
        ("heal gesture", verifyHealGesture), ("patch commit", verifyPatchCommit),
        ("precision patch commit", verifyPrecisionPatchCommit),
        ("remove accumulation", verifyRemoveAccumulation),
        ("precision stroke jobs", verifyPrecisionStrokeJobs),
        ("stroke stops at cap", verifyStrokeStopsAtCap),
        ("heal leak", verifyHealLeak), ("gesture guard", verifyGestureGuard),
        ("lazy panel", verifyLazyPanel), ("agent heal jobs", verifyAgentHealJobs),
        ("byte render-neutral carry", verifyByteRenderNeutralCarry),
        ("closed view panels", verifyClosedViewPanels),
        ("byte proof retained", verifyByteProofRetained),
        ("group expansion keeps proof", verifyGroupExpansionKeepsProof),
        ("layer thumbnail costs", verifyLayerThumbnailCosts),
        ("composite readers", verifyCompositeReaders),
        ("job escape", verifyJobEscape),
        ("job undo keys", verifyUndoKeysCancelJob),
        ("agent undo during job", verifyAgentUndoDuringJob),
        ("refused job no step", verifyRefusedJobLeavesNoStep),
        ("history during job", verifyHistoryDuringJob),
        ("window escape", verifyWindowEscape),
        ("no empty undo step", verifyNoEmptyUndoStep),
        ("truthful agent undo", verifyTruthfulAgentUndo),
        ("session commit no empty step", verifySessionCommitNoEmptyStep),
        ("window keys to canvas", verifyWindowKeysToCanvas),
        ("floating window escape", verifyFloatingWindowEscape),
        ("silent refusal beeps", verifySilentRefusalBeeps),
        ("group open no undo", verifyGroupOpenNoUndo),
        ("guides refused mid-gesture", verifyGuidesRefusedMidGesture),
        ("panel table keys", verifyPanelTableKeys),
        ("text session window keys", verifyTextSessionWindowKeys),
        ("floating panel undo", verifyFloatingPanelUndo),
        ("progress panel escape", verifyProgressPanelEscape),
        ("session refusal sentences", verifySessionRefusalSentences),
        ("more composite readers", verifyMoreCompositeReaders),
        ("precision readers", verifyPrecisionReaders),
        ("info analysis", verifyInfoAnalysis),
        ("alt nudge duplicate", verifyAltNudgeDuplicate),
        ("alt drag duplicates layers", verifyAltDragDuplicatesLayers),
        ("free transform copy", verifyFreeTransformCopy),
        ("selected pixels polish", verifySelectedPixelsPolish),
        ("move float", verifyMoveFloat),
        ("floating commit refusal", verifyFloatingCommitRefusal),
        ("discard empty group cost", verifyDiscardEmptyGroupCost),
        ("history bookkeeping cost", verifyHistoryBookkeepingCost),
        ("undo while recording", verifyUndoRefusedWhileRecording),
        ("free transform copy layers", verifyFreeTransformCopyLayers),
        ("duplicate snaps to source", verifyDuplicateSnapsToSource),
        ("geometry commit reveal", verifyGeometryCommitReveal),
        ("transform again layers", verifyTransformAgainLayers),
        ("transform again selected pixels", verifyTransformAgainSelectedPixels),
        ("transform again memory", verifyTransformAgainMemory),
        ("transform again perspective", verifyTransformAgainPerspective),
        ("precision artwork transform", verifyPrecisionArtworkTransform),
        ("perspective artwork set prompt", verifyPerspectiveArtworkSetPrompt),
        ("canvas geometry cancel", verifyCanvasGeometryCancel),
        ("auto-align cancel", verifyAutoAlignCancel),
        ("stack mode cancel", verifyStackModeCancel),
        ("merge panorama cancel", verifyMergePanoramaCancel),
        ("hdr toning cancel", verifyHdrToningCancel),
        ("save and open cancel", verifySaveOpenCancel),
        ("snapshot fingerprint cancel", verifySnapshotFingerprintCancel),
        ("float quick mask fill", verifyFloatQuickMaskFill),
        ("float paint strokes", verifyFloatPaintStrokes),
        ("float retouch strokes", verifyFloatRetouchStrokes),
        ("float retouch brushes", verifyFloatRetouchBrushes),
        ("float filters", verifyFloatFilters),
        ("target default layer", verifyTargetDefaultLayer),
        ("byte set perspective prompt", verifyByteSetPerspectivePrompt),
        ("set prompt decline and truncation", verifySetPromptDeclineAndTruncation),
        ("parallelogram copy replay", verifyParallelogramCopyReplay),
        ("copy preview tick cost", verifyCopyPreviewTickCost),
        ("preview own lease", verifyPreviewOwnLease),
        ("stroke layer edge", verifyStrokeLayerEdge),
        ("pencil", verifyPencil),
        ("pencil canvas", verifyPencilCanvases),
        ("pattern library", verifyPatternLibrary),
        ("action failure prompt", verifyActionFailurePrompt),
        ("inverse no selection", verifyInverseNoSelection),
        ("progress panel reuse", verifyProgressPanelReuse),
        ("agent document ids", verifyAgentDocumentIDs),
        ("agent stroke jobs", verifyAgentStrokeJobs),
        ("crop delete cropped", verifyCropDeleteCropped),
        ("fill grows trimmed layer", verifyFillGrowsTrimmedLayer),
        ("convert precision default", verifyConvertPrecisionDefault),
        ("new entries into group", verifyNewEntriesIntoGroup),
        ("window memory", verifyWindowMemory),
        ("long text line", verifyLongTextLine),
        ("late busy reopen", verifyLateBusyReopen),
        ("typed style preview", verifyTypedStylePreview),
        ("layer via typed", verifyLayerViaTyped),
        ("paths follow active layer", verifyPathsFollowActiveLayer),
        ("lens grid overlay", verifyLensGridOverlay),
        // The v1 UI coverage batch: sheets and menu actions through their own
        // controls. The Develop cases come last.
        ("view exposure sheet", verifyViewExposureSheet),
        ("tone map sheet preview", verifyToneMapSheetPreview),
        ("float export rows", verifyFloatExportRows),
        ("presence sheet", verifyPresenceSheet),
        ("smart sharpen sheet", verifySmartSharpenSheet),
        ("develop filter sheet", verifyDevelopFilterSheet),
        ("load files into stack", verifyLoadFilesIntoStack),
        ("auto-align action", verifyAutoAlignAction),
        ("stack mode items", verifyStackModeItems),
        ("reduce noise stack sheet", verifyReduceNoiseStackSheet),
        ("merge to hdr sheet", verifyMergeToHDRSheet),
        ("merge layers to hdr sheet", verifyMergeLayersToHDRSheet),
        ("float lens correction sheet", verifyFloatLensCorrectionSheet),
        ("float liquify sheet", verifyFloatLiquifySheet),
        ("paths panel stroke float", verifyPathsPanelStrokeFloat),
        // Round 2: File ▸ New at 32 bits, Convert Precision, Select People,
        // Lens Correction's profile, the float canvas tools, Select and Mask,
        // the Magnetic Lasso, Presence and the Develop Filter at 32 bits.
        ("new document float", verifyNewDocumentFloat),
        ("convert precision float", verifyConvertPrecisionFloat),
        ("select people", verifySelectPeople),
        ("lens correction profile", verifyLensCorrectionProfile),
        ("float paint bucket", verifyFloatPaintBucket),
        ("float dodge", verifyFloatDodge),
        ("float sponge", verifyFloatSponge),
        ("float magic wand", verifyFloatMagicWand),
        ("float quick selection", verifyFloatQuickSelection),
        ("float color range sheet", verifyFloatColorRangeSheet),
        ("select and mask float", verifySelectAndMaskFloat),
        ("magnetic lasso float", verifyMagneticLassoFloat),
        ("presence float", verifyPresenceFloat),
        ("develop filter float", verifyDevelopFilterFloat),
        ("develop range rows", verifyDevelopRangeRows),
        ("develop generated masks", verifyDevelopGeneratedMasks),
        ("develop output space", verifyDevelopOutputSpace),
        // The 32-bit float RAW development's opacity rule on hand-made buffers.
        ("raw float alpha rule", verifyRawFloatAlphaRule),
        ("develop lens line", verifyDevelopLensLine),
        // v1 shortcuts and Fade: the default chord table and tool letters,
        // Edit ▸ Fade…'s sheet, and Curves… on ⌘M.
        ("default shortcuts", verifyDefaultShortcuts),
        ("fade sheet", verifyFadeSheets),
        ("curves sheet", verifyCurvesSheets),
        ("curves pencil", verifyCurvesPencil),
        // #1: the CMYK document's Mode sheet, Channels panel and open routing.
        ("cmyk mode sheet", verifyCmykModeSheet),
        ("cmyk channels panel", verifyCmykChannelsPanel),
        ("cmyk open routing", verifyCmykOpenRouting),
        // #1 phase 2: the picker's C/M/Y/K fields and the Levels/Curves ink channels.
        ("cmyk picker", verifyCmykPicker),
        ("cmyk tone channels", verifyCmykToneChannels),
        ("cmyk hue saturation", verifyCmykHueSaturation),
        // #1: the selection batch's menu items and tools, and Stroke… in ink.
        ("cmyk selection menus", verifyCmykSelectionMenus),
        ("cmyk stroke sheet", verifyCmykStrokeSheet),
        // #1: the channels batch's Select, Channels, Paths and Vector Mask items, the ink-row
        // targets and Quick Mask on CMYK.
        ("cmyk channels menus", verifyCmykChannelsMenus),
        ("cmyk quick mask", verifyCmykQuickMask),
        // #1 phase 4a: groups, clipping, merges, blend modes and Fill on CMYK.
        ("cmyk structure", verifyCmykStructure),
        // #1 styles batch: layer effects in ink and knockout on CMYK.
        ("cmyk layer style", verifyCmykLayerStyle),
        // #1 phase 5: Color Settings, File ▸ New in CMYK, and Levels/Curves' Auto,
        // eyedroppers and presets on ink.
        ("cmyk color settings", verifyCmykColorSettings),
        ("cmyk new document", verifyCmykNewDocument),
        ("cmyk tone auto", verifyCmykToneAuto),
        ("cmyk tone eyedroppers", verifyCmykToneEyedroppers),
        ("cmyk tone presets", verifyCmykTonePresets),
        // #1 phase 4b: geometry menus and tools on CMYK, and one ⌘T session.
        ("cmyk geometry menus", verifyCmykGeometryMenus),
        ("cmyk transform session", verifyCmykTransformSession),
        // #1: the adjustments and filters on CMYK — menus per the take1 table, two sheets.
        ("cmyk adjustment menus", verifyCmykAdjustmentMenus),
        ("cmyk filter menus", verifyCmykFilterMenus),
        ("cmyk channel mixer sheet", verifyCmykChannelMixerSheet),
        ("cmyk gaussian blur sheet", verifyCmykGaussianBlurSheet),
        // #1 phase 4c: copy and paste, Stamp Selected, Merge Group and Duplicate to Document.
        ("cmyk clipboard", verifyCmykClipboard),
        // #1 phase 3: the import report sheet on a layered CMYK PSD and the
        // layered-TIFF chooser on a layered CMYK TIFF.
        ("cmyk psd import report", verifyCmykPsdImportReport),
        // #1's retouch batch: the CMYK retouch tools, Equalize's prompt and menus, and
        // Dodge/Burn's Protect Tones.
        ("cmyk retouch tools", verifyCmykRetouchTools),
        ("cmyk equalize prompt", verifyCmykEqualizePrompt),
        // #1's remaining gaps: one ink plate targeted — its menus and a Fill on Cyan.
        ("cmyk plate menus", verifyCmykPlateMenus),
        ("equalize menus", verifyEqualizeMenus),
        ("dodge protect tones option", verifyDodgeProtectTonesOption),
        // #1's artwork batch: the Type tool and a gradient fill layer's sheet on CMYK.
        ("cmyk type tool", verifyCmykTypeTool),
        ("cmyk fill layer sheet", verifyCmykFillLayerSheet),
        // Edit ▸ Puppet Warp's on-canvas session.
        ("puppet warp", verifyPuppetWarp),
        // v1.x: HDR photos and the EDR canvas; the loop below
        // runs them at DR_EDR_HEADROOM=4 (`edrCases`, ui_hdr.swift).
        ("hdr open choice", verifyHDROpenChoice),
        ("hdr revert keeps rendition", verifyHDRRevertKeepsRendition),
        ("hdr preview toggle", verifyHDRPreviewToggle),
        ("hdr export rows", verifyHDRExportRows),
        // #1: a float window's spot overprint at its own view, and HDR Preview standing down.
        ("float spot view", verifyFloatSpotView),
        ("hdr spot view", verifyHDRSpotView),
        // Edit ▸ Perspective Warp's on-canvas session.
        ("perspective warp", verifyPerspectiveWarp),
        // The model store against the loopback server, Settings ▸ Models and the
        // Depth Blur sheet (fixture model store; SKIP without it).
        ("model store licence gate", verifyModelStoreLicenceGate),
        ("model store sha refusal", verifyModelStoreShaRefusal),
        ("model store resume", verifyModelStoreResume),
        ("models pane", verifyModelsPane),
        ("models pane ready size", verifyModelsPaneReadySize),
        ("models intro path", verifyModelsIntroPath),
        ("settings card height", verifySettingsCardHeight),
        ("model catalog release urls", verifyModelCatalogReleaseURLs),
        ("model store legacy id", verifyModelStoreLegacyID),
        ("depth blur sheet", verifyDepthBlurSheet),
        // Select ▸ Sky, Person Parts… and the Object Finder (fixture model store;
        // SKIP without it) and the Models pane's five entries.
        ("model selection entries", verifyModelSelectionEntries),
        ("select sky menu", verifySelectSkyMenu),
        ("person parts sheet", verifyPersonPartsSheet),
        ("object finder click", verifyObjectFinderClick),
        // Edit ▸ Sky Replacement…'s sheet (fixture sky model and library; SKIP without them).
        ("sky replacement sheet", verifySkyReplacementSheet),
        // PrecisionGeometryWork's merged-key branch on an Indexed Color document.
        ("indexed background key", verifyIndexedBackgroundKey),
        // The status bar's mode segment on every colour mode and depth.
        ("status mode segment", verifyStatusModeSegment),
        // get_precision's mode and the summary's precision on every colour mode and depth.
        ("precision reply modes", verifyPrecisionReplyModes),
    ]
    // Opt-in measurements: run only when named in DR_JOBS_PROBE_CASES.
    let measurements: [(String, () throws -> Void)] = [
        ("slider preview latency", verifySliderPreviewLatency),
        // 45 MP filters complete and cancel at 8 and 16 bits; needs
        // DR_WORKING_LIMIT_BYTES=8589934592.
        ("large filter jobs", verifyLargeFilterJobs),
    ]
    // DR_JOBS_PROBE_CASES: comma-separated exact case names (unset or empty
    // runs every case); an unknown name fails before any case runs.
    // DR_JOBS_PROBE_REPEAT: run the selected list that many times (default 1).
    let environment = ProcessInfo.processInfo.environment
    let wanted = (environment["DR_JOBS_PROBE_CASES"] ?? "")
        .split(separator: ",")
        .map { $0.trimmingCharacters(in: .whitespaces) }
        .filter { !$0.isEmpty }
    let unknown = wanted.filter { name in !(checks + measurements).contains { $0.0 == name } }
    guard unknown.isEmpty else {
        fputs("FAIL: DR_JOBS_PROBE_CASES names no case: \(unknown.joined(separator: ", "))\n",
              stderr)
        exit(1)
    }
    let selected = wanted.isEmpty ? checks : (checks + measurements).filter { wanted.contains($0.0) }
    let repeatText = environment["DR_JOBS_PROBE_REPEAT"] ?? ""
    guard let repeats = repeatText.isEmpty ? 1 : Int(repeatText), repeats > 0 else {
        fputs("FAIL: DR_JOBS_PROBE_REPEAT is not a positive integer: \(repeatText)\n", stderr)
        exit(1)
    }
    // The screen headroom each case sees: the EDR cases engage HDR
    // Preview at 4 even on an SDR Mac; every other case runs at 1, so HDR
    // Preview (on by default) never fills a float window's proof slot on an
    // EDR screen. `EDRDisplay` reads the variable on every call.
    let edrCases: Set<String> = [
        "hdr open choice", "hdr revert keeps rendition", "hdr preview toggle", "hdr export rows",
        "hdr spot view",
    ]
    var failures = 0
    for _ in 0..<repeats {
        for (name, check) in selected {
            setenv(EDRDisplay.headroomOverrideVariable, edrCases.contains(name) ? "4" : "1", 1)
            let entry = SelectionWorkBudget.scratchBytes
            // Top-level code has no pool of its own, so what a case
            // autoreleases (and any preview reservation it keeps) is drained here.
            do { try autoreleasepool { try check() } }
            catch {
                fputs("FAIL [\(name)]: \(error)\n", stderr)
                failures += 1
                DocumentJobs.active?.control.cancel()
                try? pumpUntil({ DocumentJobs.active == nil })
            }
            if !previewRegistryDrained(after: name, entry: entry) { failures += 1 }
        }
    }
    Clipboard.releasePrivatePasteboard()
    exit(failures == 0 ? 0 : 1)
} catch {
    fputs("FAIL: \(error)\n", stderr)
    Clipboard.releasePrivatePasteboard()
    exit(1)
}
