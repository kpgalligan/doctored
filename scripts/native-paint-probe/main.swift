import AppKit
import CryptoKit

final class ProbeApplication: NSApplication {
    var errors: [String] = []
    override func presentError(_ error: Error) -> Bool {
        errors.append(error.localizedDescription); trace("APP ERROR: \(error.localizedDescription)"); return false
    }
    override func reportException(_ exception: NSException) {
        trace("OBJECTIVE-C EXCEPTION: \(exception.name.rawValue): \(exception.reason ?? "unknown")")
        trace(exception.callStackSymbols.joined(separator: "\n")); exit(2)
    }
}
func trace(_ message: String) { print(message); fflush(stdout) }
func require(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
    if try !condition() { throw DoctoredCoreError(message: message) }
}
func pump(_ seconds: Double = 0.01) {
    var count = 0
    while count < 64, let event = NSApp.nextEvent(matching: .any, until: Date(), inMode: .default, dequeue: true) {
        NSApp.sendEvent(event); count += 1
    }
    NSApp.updateWindows(); RunLoop.current.run(until: Date().addingTimeInterval(seconds))
}
func waitUntil(_ condition: () -> Bool, _ message: String) throws {
    let deadline = Date().addingTimeInterval(60)
    while !condition(), Date() < deadline { pump() }
    try require(condition(), message)
}
func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
func member(_ owner: Any, _ key: String) -> Any? {
    guard let value = Mirror(reflecting: owner).children.first(where: { $0.label == key })?.value else { return nil }
    let mirror = Mirror(reflecting: value)
    return mirror.displayStyle == .optional ? mirror.children.first?.value : value
}
func screenshot(_ view: NSView, _ url: URL) throws {
    view.layoutSubtreeIfNeeded()
    guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { throw DoctoredCoreError(message: "Owned UI screenshot unavailable") }
    view.cacheDisplay(in: view.bounds, to: bitmap)
    guard let data = bitmap.representation(using: .png, properties: [:]) else { throw DoctoredCoreError(message: "Owned PNG unavailable") }
    try data.write(to: url)
}
func fixture(width: Int = 96, height: Int = 64) -> [UInt16] {
    let alpha: [UInt16] = [0, 1, 257, 27001, 65535]
    var samples: [UInt16] = []; samples.reserveCapacity(width * height * 4)
    for i in 0..<(width * height) {
        samples.append(UInt16(1001 + (i * 113) % 50000))
        samples.append(UInt16(2013 + (i * 67) % 48000))
        samples.append(UInt16(3017 + (i * 37) % 45000))
        samples.append(alpha[i % alpha.count])
    }
    return samples
}
func openDocument(width: Int = 96, height: Int = 64) throws -> (ImageDocument, EditorViewController, RasterDocument) {
    let source = try RasterDocument.fromRawRGBA16(fixture(width: width, height: height), width: width,
        height: height, profile: RasterProfile.builtin(.sRGB))
    let document = ImageDocument(); document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: 16, exportDither: "none"), markSaved: true)
    NSDocumentController.shared.addDocument(document); document.makeWindowControllers(); document.showWindows()
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else { throw DoctoredCoreError(message: "Actual editor missing") }
    _ = editor.view; editor.view.window?.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true); pump()
    return (document, editor, source)
}
/// The 32-BIT FLOAT opener (Phase 12E-3b, carried item 1). There is no
/// promotion into float — `raster_float` is reachable only from a float TIFF
/// or the v40 reader — so the probe opens the committed 11F fixture through
/// the app's own preserving path (`RasterPrecisionDocument.openPreserving`,
/// the call `ImageDocument.openPrecisionDocumentIfRequested` makes) and
/// installs it exactly as `openDocument` installs a typed one.
///
/// `DR_PAINT_UI_FLOAT_FIXTURE` is set by `scripts/verify_phase9h_ui.py` to
/// `core/tests/fixtures/float_11f/scene.tiff`; an absent variable is a
/// FAILURE, never a silently skipped gate.
func openFloatDocument() throws -> (ImageDocument, EditorViewController, RasterDocument) {
    guard let path = ProcessInfo.processInfo.environment["DR_PAINT_UI_FLOAT_FIXTURE"],
          !path.isEmpty else {
        throw DoctoredCoreError(message: "DR_PAINT_UI_FLOAT_FIXTURE is not set")
    }
    let url = URL(fileURLWithPath: path)
    guard FileManager.default.fileExists(atPath: url.path) else {
        throw DoctoredCoreError(message: "Owned float fixture missing at \(url.path)")
    }
    let context = try PrecisionWorkContext()
    let precision = try RasterPrecisionDocument.openPreserving(
        url: url, workingLimit: context.limit, otherRetained: context.otherRetained)
    try require(precision.isFloat, "The float fixture did not open as a float document")
    let source = try RasterDocument(precision: precision)
    let document = ImageDocument(); document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: 16, exportDither: "none"), markSaved: true)
    NSDocumentController.shared.addDocument(document); document.makeWindowControllers(); document.showWindows()
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw DoctoredCoreError(message: "Actual editor missing")
    }
    _ = editor.view; editor.view.window?.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true); pump()
    try require(document.doc?.isFloat == true, "Owned fixture is not a 32-bit float document")
    return (document, editor, source)
}
func raw(_ document: RasterDocument) throws -> [UInt16] {
    let planes = try ["red", "green", "blue", "alpha"].map {
        try PrecisionScalarOperations.read(document, target: $0, index: 0).samples
    }
    return (0..<planes[0].count).flatMap { i in planes.map { $0[i] } }
}
func hashSamples(_ values: [UInt16]) -> String {
    let data = values.withUnsafeBytes { Data($0) }
    return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}
@discardableResult
func send(_ view: NSView, _ kind: NSEvent.EventType, _ point: CGPoint,
          pressure: Double? = nil, modifiers: NSEvent.ModifierFlags = []) throws -> Double {
    guard let window = view.window else {
        throw DoctoredCoreError(message: "Owned mouse event unavailable")
    }
    func build() -> NSEvent? {
        NSEvent.mouseEvent(with: kind, location: view.convert(point, to: nil), modifierFlags: modifiers,
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 0, clickCount: 1, pressure: Float(pressure ?? 1))
    }
    func tabletCopy(_ base: NSEvent, _ pressure: Double, _ shift: CGPoint = .zero) -> NSEvent? {
        guard let cg = base.cgEvent?.copy() else { return nil }
        cg.setIntegerValueField(.mouseEventSubtype, value: 1)
        cg.setDoubleValueField(.mouseEventPressure, value: pressure)
        cg.setDoubleValueField(.tabletEventPointPressure, value: pressure)
        // CG screen space counts y DOWNWARD while a window counts it upward,
        // so a correction measured in window coordinates is applied with its
        // y negated.
        if shift != .zero {
            cg.location = CGPoint(x: cg.location.x + shift.x, y: cg.location.y - shift.y)
        }
        return NSEvent(cgEvent: cg)
    }
    func drift(_ base: NSEvent, _ tablet: NSEvent) -> CGFloat {
        hypot(tablet.locationInWindow.x - base.locationInWindow.x,
              tablet.locationInWindow.y - base.locationInWindow.y)
    }
    guard var base = build() else { throw DoctoredCoreError(message: "Owned mouse event unavailable") }
    let event: NSEvent
    if let pressure {
        guard var tablet = tabletCopy(base, pressure) else {
            throw DoctoredCoreError(message: "Owned tablet event unavailable")
        }
        // The NSEvent -> CGEvent -> NSEvent round trip answers window
        // coordinates a couple of points off, so the assertion below is raced
        // rather than wrong. MEASURED, not assumed: the round trip is AFFINE,
        // not a pure translation — each correction lands about 98 % of the
        // way, so the residual falls by ~50x per attempt and reaches exactly
        // zero in about nine. Exactly zero matters: the airbrush cases
        // compare deposited dab centres with `==`, not with a tolerance.
        //
        // This runs FIRST and PUMPS NOTHING. The old bounded wait pumped the
        // run loop for its whole five seconds (waiting never settles an
        // affine offset), and a pumped run loop lets the airbrush's real
        // Timer deposit dabs between the case's read and its mouse-up.
        var attempts = 0
        var shift = CGPoint.zero
        var measured = CGPoint.zero
        // The BEST pair measured is what the caller gets: an attempt that
        // stops improving must not leave a worse copy installed than the one
        // it started from, because the airbrush cases compare dab centres
        // with `==` and every fraction of a point is load-bearing.
        func compensate() {
            var previous = drift(base, tablet)
            var best = (base: base, tablet: tablet, shift: shift, drift: previous)
            func keepBest() {
                base = best.base
                tablet = best.tablet
                shift = best.shift
            }
            while drift(base, tablet) > 0, attempts < 24 {
                attempts += 1
                measured = CGPoint(x: tablet.locationInWindow.x - base.locationInWindow.x,
                                   y: tablet.locationInWindow.y - base.locationInWindow.y)
                shift = CGPoint(x: shift.x - measured.x, y: shift.y - measured.y)
                guard let refreshed = build(),
                      let corrected = tabletCopy(refreshed, pressure, shift) else {
                    keepBest()
                    return
                }
                base = refreshed
                tablet = corrected
                let residual = drift(base, tablet)
                if residual < best.drift { best = (base, tablet, shift, residual) }
                if residual > 0, residual >= previous {
                    keepBest()
                    return
                }
                previous = residual
            }
            keepBest()
        }
        compensate()
        // Only if the geometry alone cannot answer: the window server may
        // still be digesting a just-shown window or a just-closed popover, in
        // which case waiting IS the fix. Bounded, then compensated again.
        if drift(base, tablet) >= 0.01 {
            let deadline = Date().addingTimeInterval(5)
            while drift(base, tablet) >= 0.01, Date() < deadline {
                pump()
                shift = .zero
                guard let refreshed = build(),
                      let copied = tabletCopy(refreshed, pressure) else { break }
                base = refreshed
                tablet = copied
                attempts = 0
                compensate()
            }
        }
        if attempts > 0 {
            trace("TABLET COMPENSATION \(attempts) attempt(s): last measured (\(measured.x), "
                + "\(measured.y)), shift (\(shift.x), \(shift.y)), residual "
                + "\(drift(base, tablet))")
        }
        // NSEvent's synthetic mouse factory carries byte-grid pressure even
        // with the tablet subtype. Qualify the value actually transferred to
        // the canvas; do not claim these events are hardware tablet samples.
        try require(tablet.subtype == .tabletPoint && abs(Double(tablet.pressure) - pressure) <= 1.0 / 255 + 0.000001,
            "Synthetic event lost tablet subtype/pressure: \(tablet.subtype.rawValue), \(tablet.pressure)")
        try require(drift(base, tablet) < 0.01,
            "Tablet conversion changed owned canvas coordinates by ("
                + "\(tablet.locationInWindow.x - base.locationInWindow.x), "
                + "\(tablet.locationInWindow.y - base.locationInWindow.y)) after \(attempts) "
                + "compensation attempt(s); last measured offset (\(measured.x), \(measured.y)), "
                + "applied shift (\(shift.x), \(shift.y))")
        event = tablet
    } else { event = base }
    switch kind {
    case .leftMouseDown: view.mouseDown(with: event)
    case .leftMouseDragged: view.mouseDragged(with: event)
    case .leftMouseUp: view.mouseUp(with: event)
    default: throw DoctoredCoreError(message: "Unsupported probe event")
    }
    return Double(event.pressure)
}
func call(_ tool: String, _ arguments: [String: Any], document: ImageDocument) throws -> String {
    var arguments = arguments; arguments["document_id"] = AgentServer.shared.documentID(for: document)
    let json = try JSONSerialization.data(withJSONObject: arguments, options: .sortedKeys)
    return AgentServer.shared.execute(tool: tool, argumentsJSON: String(decoding: json, as: UTF8.self))
}
func replay(_ step: ActionStep, document: ImageDocument) throws -> String {
    try call(step.tool, step.arguments, document: document)
}
func configure(_ editor: EditorViewController, size: Double = 24, hardness: Double = 0.2,
               opacity: Double = 73, flow: Double = 37, pressureSize: Bool = false,
               pressureOpacity: Bool = false, airbrush: Bool = false) {
    var options = PaintToolOptions()
    options.size = size; options.hardness = hardness * 100; options.opacity = opacity; options.flow = flow
    options.pressureSize = pressureSize; options.pressureOpacity = pressureOpacity
    options.airbrush = airbrush; options.smoothing = 0
    ToolOptionsStore.shared.brush = options
    editor.paintColor = NSColor(colorSpace: editor.document?.nsColorSpace ?? .sRGB,
        components: [0.713219, 0.193517, 0.831271, 0.671123], count: 4)
    editor.selectTool(.brush); editor.syncCanvasPaintState(); pump()
}

/// AppKit opens an untitled document at launch unless the app delegate refuses.
final class ProbeAppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }
}
let probeAppDelegate = ProbeAppDelegate()
_ = ProbeApplication.shared
NSApp.delegate = probeAppDelegate
AutomationAlerts.installIfEnabled()
_ = NSApp.setActivationPolicy(.accessory)
func runProbe() throws {
    guard let path = ProcessInfo.processInfo.environment["DR_PAINT_UI_OUTPUT"] else { throw DoctoredCoreError(message: "Private output path missing") }
    let output = URL(fileURLWithPath: path)
    var groups: [[String: Any]] = [], failures: [[String: String]] = []
    let oldBrush = ToolOptionsStore.shared.brush
    defer { ToolOptionsStore.shared.brush = oldBrush }
    // `secondPassOnly` is a case the FIRST pass must not run: it reads what an
    // earlier pass left on disk, so it only means anything in a second
    // process over the same private directories. DR_PAINT_UI_CASES (a comma
    // list) selects exactly the named cases; absent, every case that is not
    // second-pass-only runs.
    let cases: [(name: String, secondPassOnly: Bool, run: () throws -> [String: Any])] = [
        ("current-layer-commands-ui", false, { try currentLayerCommandsUI(output: output) }),
        ("pressure-ui", false, { try pressureUI(output: output) }),
        ("airbrush-ui", false, { try airbrushUI(output: output) }),
        ("pixel-airbrush-ui", false, { try airbrushUI(output: output, pixel: true) }),
        ("zero-pressure-pixel-ui", false, { try zeroPressureUI() }),
        ("versioned-mcp", false, { try versionedMCP() }),
        ("quick-mask-ui", false, { try quickMaskUI(output: output) }),
        ("quick-mask-cancel-stale", false, { try quickMaskLifecycle() }),
        ("extended-tools-ui", false, { try extendedToolsUI(output: output) }),
        ("excluded-seed-fill-ui", false, { try excludedSeedFillUI(output: output) }),
        // Phase 12A replay gaps (replay.swift)
        ("byte-pressure-ui", false, { try byteStrokeUI(output: output, airbrush: false) }),
        ("byte-airbrush-ui", false, { try byteStrokeUI(output: output, airbrush: true) }),
        ("move-record-ui", false, { try moveRecordUI() }),
        // Phase 12C filtered panel walk (panel.swift)
        ("label-filter-panel-ui", false, { try labelFilterPanelUI(output: output) }),
        // The Layers panel's thumbnail cache at 16 and 8 bits
        // (thumbnails.swift)
        ("layer-thumbnail-cache-ui", false, { try layerThumbnailCacheUI() }),
        // Phase 12E-2 continuous opacity drag (opacity.swift)
        ("opacity-drag-ui", false, { try opacityDragUI(output: output) }),
        ("byte-opacity-drag-ui", false, { try byteOpacityDragUI(output: output) }),
        // Phase 12E-2 typed multi-layer move, all or nothing (typedmove.swift)
        ("typed-move-all-or-nothing", false, { try typedMoveAllOrNothing() }),
        // Phase 12E-2b the typed New Layer, from the panel (newlayer.swift)
        ("new-layer-panel-ui", false, { try newLayerPanelUI() }),
        // Phase 12E-2c Stamp/Flatten and the ten whitelisted menu selectors
        // (stampflatten.swift)
        ("stamp-flatten-menu-ui", false, { try stampFlattenMenuUI() }),
        // Phase 12E-3b the Blending Options knockout rows and the three merge
        // menu items (knockout.swift)
        ("knockout-blending-ui", false, { try knockoutBlendingUI(output: output) }),
        ("knockout-merge-menu-ui", false, { try knockoutMergeMenuUI() }),
        // Phase 12C-2 Navigator, workspaces and shortcuts (workspace.swift)
        ("navigator-ui", false, { try navigatorUI(output: output) }),
        ("workspace-restore-ui", false, { try workspaceRestoreUI(output: output) }),
        ("workspace-frame-fit-ui", false, { try workspaceFrameFitUI() }),
        ("shortcut-edit-ui", false, { try shortcutEditUI() }),
        // Phase 12E-1b: the measured AppKit Shift-equivalence rule
        ("shortcut-shift-rule-ui", false, { try shortcutShiftRuleUI() }),
        // Phase 12D-2 view rotation and second views (views.swift)
        ("rotate-view-stroke-ui", false, { try rotateViewStrokeUI(output: output) }),
        ("rotate-view-hover-ui", false, { try rotateViewHoverUI(output: output) }),
        ("rotate-view-gesture-ui", false, { try rotateViewGestureUI() }),
        ("rotate-view-sheet-ui", false, { try rotateViewSheetUI() }),
        ("second-view-ui", false, { try secondViewUI(output: output) }),
        ("view-items-at-depth-ui", false, { try viewItemsAtDepthUI() }),
        // Editing rotated/scaled text in place (rotatedtext.swift)
        ("rotated-text-editor-ui", false, { try rotatedTextEditorUI() }),
        // Phase 12F-1 the Canvas Size sheet at depth and its retained-source
        // half (canvassize.swift)
        ("canvas-size-sheet-ui", false, { try canvasSizeSheetUI() }),
        ("image-size-percentage-ui", false, { try imageSizePercentageUI(output: output) }),
        // Phase 12F-1b the exact canvas geometry at 32-bit float
        // (floatgeometry.swift)
        ("float-exact-geometry-menu-ui", false, { try floatExactGeometryMenuUI() }),
        // Phase 12F-1b2 the Crop tool's straighten at 16 bits and the
        // zero-angle commit at both depths (straighten.swift)
        ("straighten-crop-tool-ui", false, { try straightenCropToolUI() }),
        // Phase 12F-1b3 the Rotate Arbitrary… sheet, its menu validation and
        // the SIGN against 12D's view rotation (rotatecanvas.swift)
        ("rotate-canvas-sheet-ui", false, { try rotateCanvasSheetUI() }),
        // Phase 12F-1b4 exact content no-ops and recorder suppression
        // (geometrynoops.swift)
        ("geometry-noops-ui", false, { try geometryNoOpsUI() }),
        ("precision-move-handles-ui", false, { try precisionMoveHandlesUI() }),
        ("precision-transform-handles-ui", false, { try precisionTransformHandlesUI() }),
        ("precision-crop-handles-ui", false, { try precisionCropHandlesUI() }),
        ("precision-geometry-lifecycle-ui", false, { try precisionGeometryLifecycleUI() }),
        ("precision-geometry-float-ui", false, { try precisionGeometryFloatUI() }),
        ("geometry-set-handles-ui", false, { try geometrySetHandlesUI() }),
        ("retained-artwork-geometry-ui", false, { try retainedArtworkGeometryUI() }),
        ("persisted-warp-sheet-ui", false, { try persistedWarpSheetUI() }),
        ("byte-brush-record-ui", false, { try byteBrushRecordUI() }),
        ("byte-eraser-record-ui", false, { try byteEraserRecordUI() }),
        ("byte-clone-record-ui", false, { try byteCloneRecordUI() }),
        ("byte-heal-record-ui", false, { try byteHealRecordUI() }),
        ("byte-spot-heal-record-ui", false, { try byteSpotHealRecordUI() }),
        ("byte-dodge-record-ui", false, { try byteDodgeRecordUI() }),
        ("byte-coverage-record-ui", false, { try byteCoverageRecordUI() }),
        ("byte-quick-mask-record-ui", false, { try byteQuickMaskRecordUI() }),
        ("byte-stroke-contracts-ui", false, { try byteStrokeContractsUI() }),
        ("selection-modifier-feedback-ui", false, {
            try selectionModifierFeedbackUI(output: output) }),
        ("selection-marching-ants-ui", false, { try selectionMarchingAntsUI(output: output) }),
        ("shift-click-line-ui", false, { try shiftClickLineUI() }),
        ("brush-outline-cursor-ui", false, { try brushOutlineCursorUI() }),
        ("margin-select-start-ui", false, { try marginSelectStartUI() }),
        ("canvas-keys-ui", false, { try canvasKeysUI() }),
        ("tool-keys-ui", false, { try toolKeysUI() }),
        ("automation-alerts-ui", false, { try automationAlertsUI() }),
        // Soft proof at 16 bits and exports at depth
        // (softproof.swift)
        ("soft-proof-menu-ui", false, { try softProofMenuUI() }),
        ("export-items-at-depth-ui", false, { try exportItemsAtDepthUI() }),
        // Broken preference-library files in the UI
        // (brokenlibraries.swift)
        ("broken-library-managers-ui", false, { try brokenLibraryManagersUI() }),
        ("new-document-broken-presets-ui", false, { try newDocumentBrokenPresetsUI() }),
        ("workspace-broken-files-ui", false, { try workspaceBrokenFilesUI() }),
        ("shortcut-relaunch-ui", true, { try shortcutRelaunchUI() }),
    ]
    let requested = (ProcessInfo.processInfo.environment["DR_PAINT_UI_CASES"] ?? "")
        .split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
        .filter { !$0.isEmpty }
    for name in requested {
        try require(cases.contains { $0.name == name }, "Unknown probe case \(name)")
    }
    let selected = requested.isEmpty
        ? cases.filter { !$0.secondPassOnly }
        : cases.filter { requested.contains($0.name) }
    for (name, _, run) in selected {
        trace("START \(name)")
        do { groups.append(try autoreleasepool { try run() }); trace("PASS \(name)") }
        catch { failures.append(["name": name, "error": error.localizedDescription]); trace("FAIL \(name): \(error.localizedDescription)") }
    }
    let report: [String: Any] = ["passed": failures.isEmpty, "group_count": groups.count,
        "cases": selected.map(\.name), "requested_cases": requested,
        "groups": groups, "failures": failures, "owned_documents": NSDocumentController.shared.documents.count,
        "background_scratch_after": BackgroundWork.scratchEstimate,
        "event_scope": "Synthetic tablet NSEvents routed through actual canvas handlers; hardware tablet behavior is not asserted"]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: output.appendingPathComponent("host-report.json"))
    exit(failures.isEmpty ? 0 : 1)
}
Timer.scheduledTimer(withTimeInterval: 0.05, repeats: false) { _ in
    do { try runProbe() } catch { trace(error.localizedDescription); exit(1) }
}
NSApp.run()
