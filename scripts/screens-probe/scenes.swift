import AppKit
import ObjectiveC.runtime

// One screen at a time: open its window (windows.swift's `windowKinds`), run its steps, record its
// captures, tear it down. Tool steps go through the in-process `AgentServer.shared.execute` (no
// socket is bound); UI steps are actions.swift's closures. A card's steps and the screen's
// captures run inside the card's modal session, from a timer (`ModalSteps`). No mouse or key
// event is synthesized and the app never activates.

/// The first 2x screen, or the main screen when there is none (a server capture then records
/// an error).
let placementScreen = NSScreen.screens.first { $0.backingScaleFactor == 2 } ?? NSScreen.main
let placement = NSScreen.screens.contains { $0.backingScaleFactor == 2 }
    ? "window placed on a 2x screen"
    : "no 2x screen: window on a \(placementScreen?.backingScaleFactor ?? 0)x screen"

func place(_ window: NSWindow) {
    guard let frame = placementScreen?.visibleFrame else { return }
    window.setFrameTopLeftPoint(NSPoint(x: frame.minX + 24, y: frame.maxY - 24))
}

/// What one screen has open while it runs, and what its teardown must undo.
final class Context {
    let screen: Screen
    let recorder: Recorder
    var document: ImageDocument?
    var editor: EditorViewController?
    /// The screen's own window: the editor's, the plain window, or a card host's card.
    var window: NSWindow?
    /// The running card's window, while its modal session runs.
    var card: NSWindow?
    /// The alert being captured, while the alert interceptor runs.
    var alert: NSWindow?
    /// Windows `show_window` opened, ordered out at teardown.
    var shown: [NSWindow] = []
    /// What puts back app-wide state an action changed (a preference, the shared colours); run
    /// at teardown, last first.
    var restorers: [() -> Void] = []
    /// Notes every capture of the screen carries.
    var notes: [String] = [placement]
    /// What a card's readiness check last saw, for the timeout message.
    var seen = ""

    init(screen: Screen, recorder: Recorder) {
        self.screen = screen
        self.recorder = recorder
    }

    func requireEditor() throws -> EditorViewController {
        guard let editor else { throw ProbeFailure(description: "this window has no editor") }
        return editor
    }

    func requireDocument() throws -> ImageDocument {
        guard let document else { throw ProbeFailure(description: "this window has no document") }
        return document
    }

    /// The open dialog's root view: the running card's, else the editor window's attached sheet's.
    func dialog() throws -> NSView {
        if let content = card?.contentView { return content }
        guard let content = window?.attachedSheet?.contentView else {
            throw ProbeFailure(description: "no sheet or card is open")
        }
        return content
    }
}

func callTool(_ tool: String, _ arguments: [String: Any], _ document: ImageDocument) throws {
    var arguments = arguments
    arguments["document_id"] = AgentServer.shared.documentID(for: document)
    let json = try JSONSerialization.data(withJSONObject: arguments, options: .sortedKeys)
    let reply = AgentServer.shared.execute(tool: tool, argumentsJSON: String(decoding: json, as: UTF8.self))
    let object = try? JSONSerialization.jsonObject(with: Data(reply.utf8)) as? [String: Any]
    if object == nil || object?["isError"] as? Bool == true {
        throw ProbeFailure(description: "\(tool) refused: \(reply.prefix(300))")
    }
}

func runScreen(_ screen: Screen, _ recorder: Recorder) throws {
    let context = Context(screen: screen, recorder: recorder)
    defer { tearDown(context) }
    try withAlertCaptures(context) {
        switch screen.kind.host {
        case .editor:
            try openEditor(context)
            try runSteps(screen.steps, context)
            if let card = screen.card {
                try runCard(card, context)
            } else {
                settle(context.window)
                captureAll(context)
            }
        case .card:
            guard let card = screen.card else { throw ProbeFailure(description: "a card host with no card") }
            try runCard(card, context)
        case .plain(let open):
            let window = try open(context)
            context.window = window
            place(window)
            settle(window, seconds: 0.5)
            try runSteps(screen.steps, context)
            settle(window)
            captureAll(context)
        }
    }
}

private func runStep(_ index: Int, _ step: Step, _ context: Context) throws {
    do {
        switch step {
        case .tool(let name, let arguments): try callTool(name, arguments, try context.requireDocument())
        case .ui(_, let run): try run(context)
        }
    } catch {
        throw ProbeFailure(description: "step \(index) (\(step.label)): \(error)")
    }
    settle(context.card ?? context.window)
}

private func runSteps(_ steps: [(index: Int, step: Step)], _ context: Context) throws {
    for (index, step) in steps { try runStep(index, step, context) }
}

/// Every capture of the screen but its alerts (taken while the alert runs).
private func captureAll(_ context: Context) {
    var notes = context.notes
    if let editor = context.editor { notes.append("zoomPercent \(editor.zoomPercent)") }
    for spec in context.screen.captures {
        if case .alert = spec.target { continue }
        context.recorder.capture(context.screen.id, spec, subject: {
            try captureSubject(spec.target, context)
        }, notes: notes)
    }
}

/// Runs `card`: opens it, places it on the 2x screen, waits for its readiness check, runs its
/// steps, takes the screen's captures and ends it with Cancel (`abortModal` when it has none).
private func runCard(_ card: CardRun, _ context: Context) throws {
    var steps: [ModalSteps.Step] = [("place the card", { window in
        context.card = window
        if context.editor == nil { context.window = window }
        window.hidesOnDeactivate = false
        place(window)
        settle(window)
        return true
    })]
    if let ready = card.spec.ready {
        steps.append(("the card is ready", { try ready($0, context) }))
    }
    for (index, step) in card.steps {
        steps.append(("step \(index) (\(step.label))", { _ in
            try runStep(index, step, context)
            return true
        }))
    }
    steps.append(("capture", { window in
        settle(window, seconds: 0.5)
        captureAll(context)
        return true
    }))
    steps.append(("cancel", { window in
        if let content = window.contentView, let cancel = try? cancelButton(content) {
            cancel.performClick(nil)
        } else {
            NSApp.abortModal()
        }
        return true
    }))
    defer { context.card = nil }
    do {
        try ModalSteps(title: card.spec.title, timeout: 150, steps: steps).run { try card.spec.open(context) }
    } catch {
        throw ProbeFailure(description: "card \(card.name): \(error)"
                           + (context.seen.isEmpty ? "" : "; last seen: \(context.seen)"))
    }
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
}

/// Runs `body` with `NSAlert.runModal` answered by an interceptor when the screen has alert
/// captures: it lays the alert out, shows it behind every other app's windows on the 2x screen,
/// takes the alert captures of the first alert, and answers Cancel (the first button when there
/// is none). The interceptor stacks on `AutomationAlerts`' hook and restores it
/// (scripts/color-metadata-probe/psd.swift's `verifyPSDImports`).
private func withAlertCaptures(_ context: Context, _ body: () throws -> Void) throws {
    let specs = context.screen.captures.filter { if case .alert = $0.target { true } else { false } }
    guard !specs.isEmpty else { return try body() }
    guard let method = class_getInstanceMethod(NSAlert.self, #selector(NSAlert.runModal)) else {
        throw ProbeFailure(description: "NSAlert.runModal cannot be intercepted")
    }
    var taken = false
    let interceptor: @convention(block) (AnyObject) -> Int = { object in
        guard let alert = object as? NSAlert else { return NSApplication.ModalResponse.abort.rawValue }
        // An alert's default icon is its bundle's, and the probe's "bundle" is its folder. No
        // alert in the app sets its own icon, so the app's is the one the person sees.
        alert.icon = NSApp.applicationIconImage
        alert.layout()
        let window = alert.window
        window.hidesOnDeactivate = false
        place(window)
        window.orderBack(nil)
        settle(window, seconds: 0.5)
        if !taken {
            taken = true
            context.alert = window
            for spec in specs {
                context.recorder.capture(context.screen.id, spec, subject: {
                    try captureSubject(spec.target, context)
                }, notes: context.notes)
            }
            context.alert = nil
        }
        window.orderOut(nil)
        let cancel = alert.buttons.firstIndex { $0.keyEquivalent == "\u{1b}" || $0.title == "Cancel" } ?? 0
        return NSApplication.ModalResponse.alertFirstButtonReturn.rawValue + cancel
    }
    let replacement = imp_implementationWithBlock(interceptor)
    let original = method_setImplementation(method, replacement)
    defer { method_setImplementation(method, original); imp_removeBlock(replacement) }
    try body()
    try require(taken, "no alert appeared for the alert capture")
}

// ---- editor

func openEditor(_ context: Context) throws {
    // A camera file opens through the app's RAW path (develop_actions.swift), a photo here.
    let document = context.screen.kind.fixture == "raw" ? try rawDocument(context) : try photoDocument(context)
    NSDocumentController.shared.addDocument(document)
    context.document = document
    document.makeWindowControllers()
    document.showWindows()
    guard let controller = document.windowControllers.first, let window = controller.window,
          let editor = controller.contentViewController as? EditorViewController else {
        throw ProbeFailure(description: "editor missing")
    }
    context.editor = editor
    context.window = window
    _ = editor.view
    window.setContentSize(NSSize(width: 1280, height: 800))
    place(window)
    settle(window)
    editor.zoomToFit()
    settle(window)
}

/// A fresh untitled document of the screen's photo.
private func photoDocument(_ context: Context) throws -> ImageDocument {
    guard let photo = context.screen.fixture?.path, let picture = Bitmap.decodeImage(photo) else {
        throw ProbeFailure(description: "the photo does not decode")
    }
    let space = Bitmap.ingestSpace(of: picture).space
    guard let rgba = Bitmap.straightRGBA(from: picture, fitting: picture.width, height: picture.height,
                                         space: space),
          let image = RasterImage.from(rgba: rgba, width: picture.width, height: picture.height),
          let source = RasterDocument.from(image: image) else {
        throw ProbeFailure(description: "the photo did not become a document")
    }
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: 8, exportDither: "none"), markSaved: true)
    return document
}

// ---- teardown

/// Puts everything the screen opened away: a sheet is cancelled, never applied; open sessions are
/// cancelled (closing a window would commit a transform); shown windows are ordered out;
/// app-wide state is put back; the document closes without saving.
private func tearDown(_ context: Context) {
    if let editor = context.editor, let window = context.window {
        if let sheet = window.attachedSheet {
            if let content = sheet.contentView, let cancel = try? cancelButton(content) {
                cancel.performClick(nil)
            } else {
                window.endSheet(sheet, returnCode: .cancel)
            }
            try? pumpUntil({ window.attachedSheet == nil }, seconds: 20)
        }
        editor.optionsBar.closeOverflow()
        editor.endTransformSession()
        editor.cancelPuppetWarp()
        editor.cancelShapeEditSession()
        editor.canvas.cancelTextSession()
        editor.cancelPendingRemove()
        if editor.cropSession != nil { editor.endCropSession() }
    }
    for window in context.shown { window.orderOut(nil) }
    for restore in context.restorers.reversed() { restore() }
    if let document = context.document {
        document.close()
    } else if case .plain = context.screen.kind.host {
        context.window?.orderOut(nil)
    }
    settle(nil)
}
