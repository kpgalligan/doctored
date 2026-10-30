import AppKit
import CryptoKit

/// Actual modal controls and NSEvent gestures, compiled with final app sources.
/// Reference geometry is calculated here from the documented source/viewport
/// convention, independently of RawLocalGestureView.mapping().
func runRawLocalUIProbe(source originalSource: RasterDocument, layer: Int, directory: URL) throws -> [String: Any] {
    LocalUI.progress("Starting retained UI at \(originalSource.isHighDepth ? 16 : 8)-bit document depth")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let policy = NSApp.activationPolicy()
    _ = NSApp.setActivationPolicy(.accessory)
    defer { _ = NSApp.setActivationPolicy(policy) }
    let transform = try PrecisionPixelCommand(["op": "transform", "layer": layer,
        "matrix": [0.8, 0.1, -0.1, 0.8, 13.0, 9.0], "filter": "bilinear"])
    guard let source = try originalSource.editingRetainedSource(asset: Optional<RasterDocument>.none, request: transform.request) else {
        throw LocalUI.failure("Could not prepare the actual accumulated-affine source fixture")
    }
    let originalBinding = try source.retainedSourceInfo(layer)?["transform"] as? [Double]
    var groups: [String] = []
    let prefix = source.isHighDepth ? "rgba16_" : "rgba8_"
    let state = try RawLocalWork.state(source, layer: layer)
    try LocalUI.check(state.nodes.isEmpty, "Main UI starts from a real retained RAW without locals")
    let original = try LocalUI.signature(source.sourceMaster16(layer))
    groups += try LocalUI.importControls(source: source, layer: layer, directory: directory)

    let document = try LocalUI.document(source, layer: layer)
    defer { document.close() }
    let editor = try LocalUI.editor(document)
    var approved: RasterDocument?
    var reviewedPixels: String?
    var reviewedMasks: [String] = []
    var backingScale: Double = 0
    let a = CGPoint(x: Double(state.width) * 0.2 + 0.25, y: Double(state.height) * 0.2 + 0.5)
    let b = CGPoint(x: Double(state.width) * 0.7 + 0.25, y: Double(state.height) * 0.7 + 0.5)
    let driver = LocalUIDriver(steps: [
        .ready("global controls") { controller in
            guard let window = controller.modal?.window else { throw LocalUI.failure("Missing owned Develop window") }
            backingScale = Double(window.backingScaleFactor)
            try LocalUI.slider("raw-develop-baseline", 0.25, controller)
            if state.descriptor.recipe.moireSupported { try LocalUI.slider("raw-develop-moire", 0.4, controller) }
            try LocalUI.slider("raw-develop-vignette-edge", 0.5, controller)
            try LocalUI.slider("raw-develop-vignette-x", 0.4, controller)
            try LocalUI.slider("raw-develop-vignette-y", 0.6, controller)
            try LocalUI.slider("raw-develop-vignette-falloff", 1.5, controller)
            try LocalUI.popup("raw-develop-output-space", 1, controller)
        },
        .ready("global enhancements visible") { controller in
            guard let scroll = controller.controlsScroll, let document = scroll.documentView,
                  let enhancements = controller.enhancementControls?.view else {
                throw LocalUI.failure("Missing global enhancement scroll controls")
            }
            controller.modal?.window?.contentView?.layoutSubtreeIfNeeded()
            let region = enhancements.convert(enhancements.bounds, to: document)
            _ = document.scrollToVisible(region)
            scroll.reflectScrolledClipView(scroll.contentView)
            try LocalUI.check(document.visibleRect.insetBy(dx: -1, dy: -1).contains(region),
                "All baseline, moire and manual-vignette controls are reachable in the global scroll pane")
            try LocalUI.capture(controller, directory.appendingPathComponent(prefix + "global-enhancements.png"))
            try LocalUI.segment("raw-develop-section", 1, controller)
        },
        .ready("new radial") { try LocalUI.click("raw-local-new-radial", $0) },
        .ready("radial gesture") { try LocalUI.gesture($0, source: state, from: a, to: b) },
        .ready("radial coordinates and local controls") { controller in
            let result = try LocalUI.result(controller)
            guard let mask = result.operations.last?["mask"] as? [String: Any] else { throw LocalUI.failure("Missing captured radial primitive") }
            try LocalUI.pair(mask["center"], [Double(a.x + b.x) / 2, Double(a.y + b.y) / 2], "Independent radial center")
            try LocalUI.pair(mask["radius"], [Double(b.x - a.x) / 2, Double(b.y - a.y) / 2], "Independent radial radius")
            try LocalUI.check(document.doc === source && !document.isDocumentEdited, "Unpublished radial preview")
            try LocalUI.slider("raw-local-exposure", -0.5, controller)
            try LocalUI.popup("raw-local-white-balance", 2, controller)
            try LocalUI.slider("raw-local-temperature", 6200, controller)
            try LocalUI.slider("raw-local-tint", 14, controller)
            try LocalUI.slider("raw-local-density", 0.8, controller)
            try LocalUI.slider("raw-local-feather", 2.5, controller)
        },
        .ready("source brush add") { controller in
            try LocalUI.popup("raw-local-tool", 1, controller)
            try LocalUI.slider("raw-local-radius", 8, controller)
            try LocalUI.gesture(controller, source: state,
                from: CGPoint(x: a.x, y: b.y), to: CGPoint(x: b.x, y: a.y))
        },
        .ready("source brush subtract") { controller in
            try LocalUI.popup("raw-local-tool", 2, controller)
            try LocalUI.slider("raw-local-radius", 5, controller)
            try LocalUI.gesture(controller, source: state,
                from: CGPoint(x: a.x, y: a.y), to: CGPoint(x: b.x, y: b.y))
        },
        .ready("invert") { try LocalUI.click("raw-local-invert", $0) },
        .ready("copy settings") { controller in
            try LocalUI.click("raw-local-copy", controller)
            try LocalUI.click("raw-local-new-linear", controller)
        },
        .ready("linear gesture") { try LocalUI.gesture($0, source: state, from: a, to: b) },
        .ready("paste local controls") { controller in
            try LocalUI.click("raw-local-paste", controller)
        },
        .ready("local order") { controller in
            guard let nodes = controller.localSession?.state.nodes, nodes.count == 2 else { throw LocalUI.failure("Missing two local masks") }
            try LocalUI.check(nodes[0].exposureOffset == nodes[1].exposureOffset
                && nodes[0].whiteBalance == nodes[1].whiteBalance
                && nodes[0].temperature == nodes[1].temperature && nodes[0].tint == nodes[1].tint,
                "Copy/Paste transfers exact local camera settings")
            try LocalUI.check(nodes[1].density == 1 && nodes[1].feather == 0,
                "Copy/Paste preserves target mask properties")
            try LocalUI.click("raw-local-earlier", controller)
        },
        .ready("order restoration") { try LocalUI.click("raw-local-later", $0) },
        .ready("create removable empty") { try LocalUI.click("raw-local-new-empty", $0) },
        .ready("remove empty") { try LocalUI.click("raw-local-remove", $0) },
        .ready("select saved local") { try LocalUI.selectList(1, $0) },
        .ready("disabled local") { try LocalUI.toggleNode(false, $0) },
        .ready("reenabled local") { try LocalUI.toggleNode(true, $0) },
        .ready("exact reviewed Apply") { controller in
            let result = try LocalUI.result(controller)
            approved = result.document
            reviewedPixels = try LocalUI.signature(result.document.sourceMaster16(layer))
            reviewedMasks = try RawLocalWork.state(result.document, layer: layer).masks.map { try $0.sha256() }
            try LocalUI.check(controller.localSession?.state.nodes.count == 2, "Create/remove preserves two final locals")
            try LocalUI.check(result.settings.baselineCompensation == 0.25 && result.settings.vignette?.edgeExposure == 0.5
                && result.settings.outputSpace == .displayP3, "Displayed globals match final sparse request")
            try LocalUI.capture(controller, directory.appendingPathComponent(prefix + "local-controls.png"))
            try LocalUI.click("raw-develop-apply", controller)
        }
    ])
    try driver.run { editor.redevelopRaw(nil) }
    try LocalUI.check(document.doc === approved && document.doc !== source && document.isDocumentEdited,
        "Apply publishes exactly the reviewed complete document")
    try LocalUI.check(try LocalUI.signature(document.doc.sourceMaster16(layer)) == reviewedPixels,
        "Every reviewed source sample/profile remains exact after Apply")
    try LocalUI.check(try RawLocalWork.state(document.doc, layer: layer).masks.map { try $0.sha256() } == reviewedMasks,
        "Every reviewed mask remains exact after Apply")
    try LocalUI.check(try document.doc.retainedSourceInfo(layer)?["transform"] as? [Double] == originalBinding,
        "Local development preserves the actual accumulated source affine")
    document.undoManager?.undo()
    try LocalUI.check(document.doc === source && !document.isDocumentEdited, "One Undo restores original owner and saved state")
    document.undoManager?.redo()
    try LocalUI.check(document.doc === approved, "One Redo restores the reviewed owner")
    groups += [prefix + "global_camera_vignette_p3_and_source_coordinate_radial_linear",
        prefix + "native_brush_add_subtract_invert_order_create_remove",
        prefix + "local_wb_density_feather_enabled_and_exact_settings_copy",
        prefix + "exact_reviewed_apply_masks_samples_profile_and_one_undo_redo"]
    approved = nil

    let cancelled = try LocalUI.document(source, layer: layer)
    defer { cancelled.close() }
    let cancelEditor = try LocalUI.editor(cancelled)
    weak var discarded: RasterDocument?
    weak var discardedMask: RasterRawLocalMask?
    let cancelDriver = LocalUIDriver(steps: [
        .ready("cancel local tab") { try LocalUI.segment("raw-develop-section", 1, $0) },
        .ready("cancel create") { try LocalUI.click("raw-local-new-empty", $0) },
        .ready("cancel releases prepared graph") { controller in
            discarded = controller.localSession?.result?.document
            discardedMask = controller.localSession?.state.masks.first
            try LocalUI.check(discarded != nil && discardedMask != nil, "Cancel owns a real candidate and mask")
            try LocalUI.cancel(controller)
        }
    ])
    try cancelDriver.run { cancelEditor.redevelopRaw(nil) }
    try LocalUI.waitIdle()
    try LocalUI.check(discarded == nil && discardedMask == nil && cancelled.doc === source && !cancelled.isDocumentEdited,
        "Cancel releases complete unpublished candidate/mask graph without editing")
    groups.append(prefix + "cancel_releases_prepared_native_mask_graph")

    let queued = LocalUIDriver(steps: [
        .ready("queue changing exposure") { try LocalUI.slider("raw-develop-baseline", 0.375, $0) },
        .busy("cancel pending native worker") { try LocalUI.cancel($0) }
    ])
    try queued.run { cancelEditor.redevelopRaw(nil) }
    try LocalUI.waitIdle()
    try LocalUI.check(cancelled.doc === source && !cancelled.isDocumentEdited, "Queued Cancel never publishes a late result")
    groups.append(prefix + "queued_worker_cancellation_and_mailbox_release")

    let stale = LocalUIDriver(steps: [
        .ready("stale prepared result") { try LocalUI.slider("raw-develop-baseline", 0.5, $0) },
        .ready("selection changes before Apply") { controller in
            let selection = CanvasSelection(shape: .rect(CGRect(x: 2, y: 3, width: 11, height: 13)),
                canvasWidth: source.width, canvasHeight: source.height)
            try LocalUI.check(selection != nil, "Stale-selection fixture is nonempty")
            cancelEditor.agentSetSelection(selection)
            try LocalUI.click("raw-develop-apply", controller)
            try LocalUI.check(cancelled.doc === source, "Stale selection refuses before publication")
        },
        .always("cancel stale modal") { try LocalUI.cancel($0) }
    ])
    try stale.run { cancelEditor.redevelopRaw(nil) }
    try LocalUI.waitIdle()
    try LocalUI.check(try LocalUI.signature(source.sourceMaster16(layer)) == original, "Original pixels survive every UI refusal")
    groups.append(prefix + "stale_selection_refuses_prepared_apply")
    cancelEditor.agentSetSelection(nil)
    var newer: RasterDocument?
    let staleSource = LocalUIDriver(steps: [
        .ready("prepare before source change") { try LocalUI.slider("raw-develop-baseline", 0.625, $0) },
        .ready("source changes before Apply") { controller in
            withUndoGroup(cancelled.undoManager) {
                cancelled.applyEdit("Probe rename", record: .unrecorded("Probe rename")) {
                    try $0.withLayerName(layer, "Changed during RAW preview")
                }
            }
            newer = cancelled.doc
            try LocalUI.check(newer !== source, "Stale-source fixture actually changed its immutable owner")
            try LocalUI.click("raw-develop-apply", controller)
            try LocalUI.check(cancelled.doc === newer, "Prepared RAW Apply cannot replace the newer source")
        },
        .always("cancel stale source modal") { try LocalUI.cancel($0) }
    ])
    try staleSource.run { cancelEditor.redevelopRaw(nil) }
    try LocalUI.waitIdle()
    groups.append(prefix + "stale_source_refuses_prepared_apply")
    let report: [String: Any] = ["passed": true, "groups": groups, "group_count": groups.count,
        "actual_backing_scale": backingScale]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: directory.appendingPathComponent(prefix + "ui-report.json"))
    LocalUI.progress("Passed \(groups.count) retained UI groups at \(source.isHighDepth ? 16 : 8)-bit document depth")
    return report
}

func runRawLocalNavigationUIProbe(source: RasterDocument, layer: Int, directory: URL) throws -> [String: Any] {
    LocalUI.progress("Starting genuine full-camera UI navigation")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let policy = NSApp.activationPolicy(); _ = NSApp.setActivationPolicy(.accessory)
    defer { _ = NSApp.setActivationPolicy(policy) }
    let state = try RawLocalWork.state(source, layer: layer)
    try LocalUI.check(state.width > 1024 && state.height > 1024, "Navigation uses the actual full-size camera, not a resized substitute")
    let document = try LocalUI.document(source, layer: layer); defer { document.close() }
    let editor = try LocalUI.editor(document)
    var oldCenter = CGPoint.zero, expectedCenter = CGPoint.zero
    var expected: [Double] = []
    var feathered: RasterDocument?
    var backingScale: Double = 0
    var capturedFrame: RawDevelopPreviewFrame?
    let driver = LocalUIDriver(steps: [
        .ready("native detail") { try LocalUI.segment("raw-develop-zoom", 1, $0) },
        .ready("native pan") { controller in
            guard let panel = controller.workflowPanel, let window = controller.modal?.window else { throw LocalUI.failure("Missing native pane") }
            oldCenter = panel.currentViewport().center
            let scale = window.backingScaleFactor
            backingScale = Double(scale)
            expectedCenter = CGPoint(x: oldCenter.x + 32 * scale / CGFloat(state.width),
                y: oldCenter.y + 24 * scale / CGFloat(state.height))
            try LocalUI.drag(view: panel.canvas, from: CGPoint(x: 120, y: 100), to: CGPoint(x: 88, y: 76))
        },
        .ready("pan identity") { controller in
            guard let panel = controller.workflowPanel, let frame = panel.canvas.preview else { throw LocalUI.failure("Missing panned frame") }
            try LocalUI.check(abs(frame.viewport.center.x - expectedCenter.x) < 1e-10
                && abs(frame.viewport.center.y - expectedCenter.y) < 1e-10 && frame.viewport.center != oldCenter,
                "Actual drag pans by backing pixels over complete native source dimensions")
            try LocalUI.segment("raw-develop-section", 1, controller)
        },
        .ready("native radial") { try LocalUI.click("raw-local-new-radial", $0) },
        .ready("native source gesture") { controller in
            let geometry = try LocalUI.geometry(controller, source: state)
            let a = CGPoint(x: geometry.source.minX + 40.25, y: geometry.source.minY + 35.75)
            let b = CGPoint(x: geometry.source.minX + 160.75, y: geometry.source.minY + 120.25)
            expected = [Double(a.x + b.x) / 2, Double(a.y + b.y) / 2]
            try LocalUI.gesture(controller, source: state, from: a, to: b)
        },
        .ready("native gesture and feather") { controller in
            let result = try LocalUI.result(controller)
            guard let mask = result.operations.last?["mask"] as? [String: Any] else { throw LocalUI.failure("Missing native gesture primitive") }
            try LocalUI.pair(mask["center"], expected, "Panned Retina gesture remains in full source coordinates")
            try LocalUI.slider("raw-local-feather", 3, controller)
        },
        .ready("effective feather admission and cached pan") { controller in
            guard let panel = controller.workflowPanel, let window = controller.modal?.window,
                  let local = controller.localSession?.state, let node = local.nodes.first,
                  let mask = local.masks.first else { throw LocalUI.failure("Missing feathered native source mask") }
            try LocalUI.check(node.feather == 3 && controller.localGesture?.overlay != nil,
                "Full camera effective feather overlay is displayed")
            let geometry = try LocalUI.geometry(controller, source: state)
            let required = try mask.previewScratch(x: Int(geometry.source.minX), y: Int(geometry.source.minY),
                width: Int(geometry.source.width), height: Int(geometry.source.height), divisor: 1,
                density: node.density, feather: node.feather, effective: true)
            try LocalUI.check(required > 64 * 1024 * 1024,
                "Actual full-camera feather requires more than the former fixed display allowance")
            feathered = try LocalUI.result(controller).document
            oldCenter = panel.currentViewport().center
            let scale = window.backingScaleFactor
            expectedCenter = CGPoint(x: oldCenter.x + 16 * scale / CGFloat(state.width),
                y: oldCenter.y + 12 * scale / CGFloat(state.height))
            try LocalUI.popup("raw-local-tool", 0, controller)
            try LocalUI.drag(view: panel.canvas, from: CGPoint(x: 120, y: 100), to: CGPoint(x: 104, y: 88))
        },
        .ready("cached feather pan capture and close") { controller in
            guard let frame = controller.workflowPanel?.canvas.preview else { throw LocalUI.failure("Missing cached feather preview") }
            try LocalUI.check(try LocalUI.result(controller).document === feathered
                && controller.localGesture?.overlay != nil
                && abs(frame.viewport.center.x - expectedCenter.x) < 1e-10
                && abs(frame.viewport.center.y - expectedCenter.y) < 1e-10,
                "Cached pan preserves exact developed owner and renders the admitted full-camera feather overlay")
            try LocalUI.capture(controller, directory.appendingPathComponent("camera-native-local.png"))
            capturedFrame = frame
            feathered = nil
            document.close()
        }
    ])
    try driver.run { editor.redevelopRaw(nil) }
    try LocalUI.waitIdle()
    try LocalUI.check(document.hasClosed && document.doc === source, "Actual document close discards unpublished native local development")
    guard let capturedFrame else { throw LocalUI.failure("Missing genuine camera frame for backing-scale geometry") }
    let doubleScale = try LocalUI.doubleScaleGeometry(frame: capturedFrame)
    LocalUI.progress("Passed 2 genuine full-camera navigation/close groups")
    return ["passed": true, "group_count": 2, "actual_backing_scale": backingScale,
        "double_scale_geometry": doubleScale,
        "groups": ["genuine_camera_native_pan_source_gesture_and_admitted_feather", "actual_document_close_discards_local_preview"]]
}

private enum LocalUI {
    static func progress(_ message: String) {
        FileHandle.standardOutput.write(Data(("RAW local UI: " + message + "\n").utf8))
    }
    /// Runs the production canvas and gesture views at 2× using a real screen
    /// when available. A window override is explicitly simulation, not hardware
    /// qualification; no product hook or copied gesture implementation is used.
    static func doubleScaleGeometry(frame: RawDevelopPreviewFrame) throws -> [String: Any] {
        let scales = NSScreen.screens.map { Double($0.backingScaleFactor) }
        let screen = NSScreen.screens.first { $0.backingScaleFactor == 2 }
        let window: NSWindow
        if let screen {
            let rect = CGRect(x: screen.visibleFrame.minX + 40, y: screen.visibleFrame.minY + 40,
                width: 320, height: 240)
            window = NSWindow(contentRect: rect, styleMask: .borderless, backing: .buffered,
                defer: false, screen: screen)
        } else {
            window = LocalUIDoubleScaleWindow()
        }
        window.isReleasedWhenClosed = false
        let canvas = RawDevelopCanvasView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        window.contentView = canvas
        window.orderFront(nil)
        let gesture = RawLocalGestureView(frame: canvas.bounds)
        canvas.addSubview(gesture); gesture.canvas = canvas; gesture.tool = .radial
        defer {
            gesture.clear(); canvas.preview = nil; canvas.onPan = nil
            window.orderOut(nil); window.close()
        }
        try check(window.backingScaleFactor == 2 && canvas.backingSize == CGSize(width: 640, height: 480),
            "The production canvas uses the declared 2x backing extent")
        var viewport = RawDevelopViewport()
        viewport.nativeDetail = true; viewport.center = CGPoint(x: 0.42, y: 0.37)
        viewport.backingSize = canvas.backingSize
        let requestedViewport = viewport
        _ = try RasterResources.shared.admit(scratch: 16 * 1024 * 1024)
        let rendered = try BackgroundWork.wait(try WorkerTask(scratchEstimate: 16 * 1024 * 1024) {
            try RawDevelopPreviewFrame(before: frame.before, after: frame.after, viewport: requestedViewport)
        }, title: "Inspecting 2x Source Geometry")
        canvas.preview = rendered
        let width = frame.after.image.width, height = frame.after.image.height
        let x = Int((0.42 * Double(width)).rounded()) - 320
        let y = Int((0.37 * Double(height)).rounded()) - 240
        try check(rendered.afterDisplay.width == 640 && rendered.afterDisplay.height == 480,
            "2x native preview contains every backing pixel without reduction")
        var delta: CGPoint?
        canvas.onPan = { delta = $0 }
        try drag(view: canvas, from: CGPoint(x: 120, y: 100), to: CGPoint(x: 88, y: 76))
        try check(delta.map { abs($0.x - 64 / CGFloat(width)) < 1e-12
            && abs($0.y - 48 / CGFloat(height)) < 1e-12 } == true,
            "Production 2x canvas drag resolves to exact native-source displacement")
        var command: [String: Any]?
        gesture.onCommand = { value, _ in command = value }
        let a = CGPoint(x: 30.25, y: 40.75), b = CGPoint(x: 140.75, y: 110.25)
        try drag(view: gesture, from: a, to: b)
        try pair(command?["center"], [Double(x) + Double(a.x + b.x), Double(y) + Double(a.y + b.y)],
            "Production 2x radial gesture retains source-pixel center coordinates")
        try pair(command?["radius"], [Double(b.x - a.x), Double(b.y - a.y)],
            "Production 2x radial gesture retains source-pixel radii")
        let mode = screen == nil ? "simulated_2x_window_geometry" : "actual_2x_screen"
        progress("Passed \(mode); available screen scales: \(scales)")
        return ["passed": true, "mode": mode, "screen_backing_scales": scales,
            "tested_backing_scale": 2, "actual_2x_hardware_qualified": screen != nil]
    }
    static func failure(_ text: String) -> DoctoredCoreError { DoctoredCoreError(message: "RAW local UI probe: " + text) }
    static func check(_ value: @autoclosure () throws -> Bool, _ text: String) throws {
        guard try value() else { throw failure(text) }
    }
    static func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
    static func view(_ id: String, _ controller: RawDevelopWindowController) throws -> NSView {
        guard let content = controller.modal?.window?.contentView,
              let found = descendants(content).first(where: { $0.accessibilityIdentifier() == id }) else { throw failure("Missing control " + id) }
        return found
    }
    static func send(_ control: NSControl) throws {
        guard let action = control.action, control.isEnabled else { throw failure("Control action unavailable") }
        _ = control.sendAction(action, to: control.target)
    }
    static func slider(_ id: String, _ value: Double, _ controller: RawDevelopWindowController) throws {
        guard let row = try view(id, controller) as? CardControlRow, let slider = row.slider else { throw failure("Missing slider " + id) }
        slider.doubleValue = value; try send(slider)
    }
    static func popup(_ id: String, _ index: Int, _ controller: RawDevelopWindowController) throws {
        guard let row = try view(id, controller) as? CardControlRow, let popup = row.popUp else { throw failure("Missing popup " + id) }
        popup.selectItem(at: index); try send(popup)
    }
    static func segment(_ id: String, _ index: Int, _ controller: RawDevelopWindowController) throws {
        guard let control = try view(id, controller) as? NSSegmentedControl else { throw failure("Missing segment " + id) }
        control.selectedSegment = index; try send(control)
    }
    static func click(_ id: String, _ controller: RawDevelopWindowController) throws {
        guard let button = try view(id, controller) as? NSButton else { throw failure("Missing button " + id) }
        try check(button.isEnabled, "Enabled button " + id); button.performClick(nil)
    }
    static func cancel(_ controller: RawDevelopWindowController) throws {
        guard let content = controller.modal?.window?.contentView,
              let button = descendants(content).compactMap({ $0 as? NSButton })
                .first(where: { $0.action.map(NSStringFromSelector) == "cancelClicked:" }) else { throw failure("Missing actual Cancel") }
        button.performClick(nil)
    }
    static func selectList(_ index: Int, _ controller: RawDevelopWindowController) throws {
        guard let list = try view("raw-local-list", controller) as? NSPopUpButton else { throw failure("Missing local list") }
        list.selectItem(at: index); try send(list)
    }
    static func toggleNode(_ enabled: Bool, _ controller: RawDevelopWindowController) throws {
        guard let controls = controller.localControls,
              let row = descendants(controls.view).compactMap({ $0 as? CardControlRow }).first,
              let checkbox = row.checkbox else { throw failure("Missing local enable checkbox") }
        checkbox.state = enabled ? .on : .off; try send(checkbox)
    }
    static func result(_ controller: RawDevelopWindowController) throws -> RawLocalDevelopResult {
        guard let result = controller.localSession?.result else { throw failure("Missing actual prepared local result") }
        return result
    }
    static func pair(_ object: Any?, _ expected: [Double], _ message: String) throws {
        guard let values = object as? [NSNumber], values.count == expected.count else { throw failure(message) }
        try check(zip(values, expected).allSatisfy { abs($0.0.doubleValue - $0.1) < 1e-8 }, message)
    }
    static func document(_ source: RasterDocument, layer: Int) throws -> ImageDocument {
        let document = ImageDocument(); document.fileType = ImageDocument.nativeTypeIdentifier
        document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
            layerSelection: .single(layer), paintTarget: .layer, jpegExportQuality: 90,
            embedColorProfile: true, stripMetadata: false, exportBitDepth: source.isHighDepth ? 16 : 8,
            exportDither: "none"), markSaved: true)
        NSDocumentController.shared.addDocument(document)
        document.makeWindowControllers(); document.showWindows()
        return document
    }
    static func editor(_ document: ImageDocument) throws -> EditorViewController {
        guard let controller = document.windowControllers.first?.contentViewController as? EditorViewController else { throw failure("Missing actual editor") }
        _ = controller.view; return controller
    }
    static func signature(_ image: RasterPrecisionImage) throws -> String {
        var hash = SHA256(); hash.update(data: image.profileData)
        try image.withRGBA16 { samples in
            guard let pointer = samples.baseAddress else { throw failure("Empty native master") }
            for offset in stride(from: 0, to: samples.count, by: 4096) {
                hash.update(data: Data(bytes: pointer.advanced(by: offset), count: min(4096, samples.count - offset) * 2))
            }
        }
        return hash.finalize().map { String(format: "%02x", $0) }.joined()
    }
    struct Geometry { let source: CGRect; let display: CGRect }
    static func geometry(_ controller: RawDevelopWindowController, source: RawLocalState) throws -> Geometry {
        guard let panel = controller.workflowPanel, let window = controller.modal?.window,
              let frame = panel.canvas.preview else { throw failure("No displayed source geometry") }
        let canvas = panel.canvas, scale = window.backingScaleFactor
        let region: CGRect, display: CGRect
        if frame.viewport.nativeDetail {
            let w = min(source.width, 1024, max(1, Int(floor(canvas.bounds.width * scale))))
            let h = min(source.height, 1024, max(1, Int(floor(canvas.bounds.height * scale))))
            let x = min(source.width - w, max(0, Int(round(frame.viewport.center.x * CGFloat(source.width))) - w / 2))
            let y = min(source.height - h, max(0, Int(round(frame.viewport.center.y * CGFloat(source.height))) - h / 2))
            region = CGRect(x: x, y: y, width: w, height: h)
            display = CGRect(x: (canvas.bounds.width - CGFloat(w) / scale) / 2,
                y: (canvas.bounds.height - CGFloat(h) / scale) / 2, width: CGFloat(w) / scale, height: CGFloat(h) / scale)
        } else {
            region = CGRect(x: 0, y: 0, width: source.width, height: source.height)
            let ratio = min(canvas.bounds.width / CGFloat(source.width), canvas.bounds.height / CGFloat(source.height))
            display = CGRect(x: (canvas.bounds.width - CGFloat(source.width) * ratio) / 2,
                y: (canvas.bounds.height - CGFloat(source.height) * ratio) / 2,
                width: CGFloat(source.width) * ratio, height: CGFloat(source.height) * ratio)
        }
        return Geometry(source: region, display: display)
    }
    static func gesture(_ controller: RawDevelopWindowController, source: RawLocalState, from: CGPoint, to: CGPoint) throws {
        guard let gesture = controller.localGesture else { throw failure("Missing actual gesture view") }
        let g = try geometry(controller, source: source)
        func point(_ value: CGPoint) -> CGPoint {
            CGPoint(x: g.display.minX + (value.x - g.source.minX) / g.source.width * g.display.width,
                y: g.display.minY + (value.y - g.source.minY) / g.source.height * g.display.height)
        }
        try check(gesture.enabled, "Source gesture is enabled for the displayed frame")
        try drag(view: gesture, from: point(from), to: point(to))
    }
    static func drag(view: NSView, from: CGPoint, to: CGPoint) throws {
        guard let window = view.window else { throw failure("Gesture has no owned window") }
        func event(_ type: NSEvent.EventType, _ point: CGPoint) throws -> NSEvent {
            guard let event = NSEvent.mouseEvent(with: type, location: view.convert(point, to: nil),
                modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1) else { throw failure("Could not create owned mouse event") }
            return event
        }
        view.mouseDown(with: try event(.leftMouseDown, from))
        view.mouseDragged(with: try event(.leftMouseDragged, to))
        view.mouseUp(with: try event(.leftMouseUp, to))
    }
    static func capture(_ controller: RawDevelopWindowController, _ url: URL) throws {
        guard let view = controller.modal?.window?.contentView else { throw failure("Missing owned modal content") }
        view.layoutSubtreeIfNeeded()
        guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { throw failure("Cannot capture owned modal") }
        view.cacheDisplay(in: view.bounds, to: bitmap)
        guard let data = bitmap.representation(using: .png, properties: [:]) else { throw failure("Cannot encode owned modal capture") }
        try data.write(to: url)
    }
    static func waitIdle() throws {
        let end = Date().addingTimeInterval(30)
        while BackgroundWork.occupied, Date() < end { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
        try check(!BackgroundWork.occupied, "All owned preview work released its shared worker slot")
    }
    static func importControls(source: RasterDocument, layer: Int, directory: URL) throws -> [String] {
        let state = try RawLocalWork.state(source, layer: layer)
        let snapshot = try RawSourceSnapshot.restore(source, layer: layer, descriptor: state.descriptor)
        guard let probe = RawImage.inspect(url: snapshot.url) else { throw failure("Missing original import probe") }
        let driver = LocalUIDriver(steps: [
            .ready("initial import controls") { controller in
                try slider("raw-develop-baseline", 0.125, controller)
                if state.descriptor.recipe.moireSupported { try slider("raw-develop-moire", 0.3, controller) }
                try slider("raw-develop-vignette-edge", 0.25, controller)
            },
            .ready("initial import ready") { controller in
                try check(controller.localSession == nil, "Initial import has global controls without uncaptured local resources")
                try click("raw-develop-apply", controller)
            }
        ])
        var imported: (settings: RawDevelopSettings, highDepth: Bool)?
        try driver.run { imported = RawDevelopWindowController.runImport(probe, descriptor: state.descriptor, snapshot: snapshot) }
        try check(imported?.settings.baselineCompensation == 0.125 && imported?.settings.vignette?.edgeExposure == 0.25,
            "Old import API returns the exact displayed new globals")
        if state.descriptor.recipe.moireSupported { try check(imported?.settings.moire == 0.3, "Supported initial moire captured") }
        return [(source.isHighDepth ? "rgba16_" : "rgba8_") + "initial_import_camera_vignette_controls_preserve_old_api"]
    }
}

private final class LocalUIDoubleScaleWindow: NSWindow {
    override var backingScaleFactor: CGFloat { 2 }
    init() {
        super.init(contentRect: CGRect(x: 0, y: 0, width: 320, height: 240),
            styleMask: .borderless, backing: .buffered, defer: false)
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("LocalUIDoubleScaleWindow does not support NSCoder") }
}

private final class LocalUIDriver {
    struct Step {
        enum Wait { case ready, busy, always }
        let name: String; let wait: Wait; let action: (RawDevelopWindowController) throws -> Void
        static func ready(_ name: String, _ action: @escaping (RawDevelopWindowController) throws -> Void) -> Self { Self(name: name, wait: .ready, action: action) }
        static func busy(_ name: String, _ action: @escaping (RawDevelopWindowController) throws -> Void) -> Self { Self(name: name, wait: .busy, action: action) }
        static func always(_ name: String, _ action: @escaping (RawDevelopWindowController) throws -> Void) -> Self { Self(name: name, wait: .always, action: action) }
    }
    let steps: [Step]
    private var index = 0
    private var error: Error?
    private var timer: Timer?
    private var deadline = Date().addingTimeInterval(90)
    init(steps: [Step]) { self.steps = steps }
    func run(_ body: () -> Void) throws {
        let timer = Timer(timeInterval: 0.025, repeats: true) { [weak self] _ in self?.tick() }
        self.timer = timer
        RunLoop.main.add(timer, forMode: .common); RunLoop.main.add(timer, forMode: .modalPanel)
        defer { timer.invalidate(); self.timer = nil }
        autoreleasepool { body() }
        if let error { throw error }
        try LocalUI.check(index == steps.count, "Modal completed before UI step \(index)/\(steps.count)")
    }
    private func tick() {
        guard index < steps.count, error == nil else { return }
        guard let content = NSApp.modalWindow?.contentView,
              let apply = LocalUI.descendants(content).compactMap({ $0 as? NSButton })
                .first(where: { $0.accessibilityIdentifier() == "raw-develop-apply" }),
              let controller = apply.target as? RawDevelopWindowController else {
            if Date() > deadline { error = LocalUI.failure("No owned Develop modal at step \(index)"); NSApp.abortModal() }
            return
        }
        let step = steps[index]
        if Date() > deadline {
            error = LocalUI.failure("Timed out at UI step \(index): \(step.name)")
            try? LocalUI.cancel(controller); return
        }
        switch step.wait {
        case .ready: guard controller.previewIsReady else { return }
        case .busy: guard BackgroundWork.occupied else { return }
        case .always: break
        }
        do {
            LocalUI.progress("Step \(index + 1)/\(steps.count): \(step.name)")
            try autoreleasepool { try step.action(controller) }
            index += 1; deadline = Date().addingTimeInterval(90)
        } catch {
            self.error = LocalUI.failure("Step \(step.name): \(error.localizedDescription)")
            try? LocalUI.cancel(controller)
        }
    }
}
