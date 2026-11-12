import AppKit

// The camera-file editor's opener and the helpers of the Develop RAW card's UI actions (their
// `uiActions` entries are in actions.swift; the cards are windows.swift's `developCard` and
// `redevelop_raw`). Each calls what the card's own controls call; none synthesizes an event. The
// card is cancelled when its captures are taken (scenes.swift), never applied.

/// The `editor_raw` window's document: the screen's camera file opened as File ▸ Open opens one,
/// developed at the default settings with no dialog, as the agent's `open_document` does. It is
/// opened from a copy in the run's private home, so the document never points into the fixtures
/// store and no sidecar beside the original is read.
func rawDocument(_ context: Context) throws -> ImageDocument {
    guard let source = context.screen.fixture?.path else { throw ProbeFailure(description: "no camera file") }
    guard let home = ProcessInfo.processInfo.environment["CFFIXED_USER_HOME"], !home.isEmpty else {
        throw ProbeFailure(description: "CFFIXED_USER_HOME is not set (run.py sets a private home)")
    }
    let folder = URL(fileURLWithPath: home, isDirectory: true)
        .appendingPathComponent("raw-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let url = folder.appendingPathComponent(source.lastPathComponent)
    try FileManager.default.copyItem(at: source, to: url)
    let controller = NSDocumentController.shared
    let type = try controller.typeForContents(of: url)
    let previous = RawImportRequest.mode
    RawImportRequest.mode = .headless(RawDevelopSettings())
    defer { RawImportRequest.mode = previous }
    guard let document = try controller.makeDocument(withContentsOf: url, ofType: type) as? ImageDocument else {
        throw ProbeFailure(description: "\(url.lastPathComponent) did not open as an image document")
    }
    return document
}

/// The running Develop RAW card's controller: its Apply button's target
/// (`developController` in scripts/jobs-probe/ui_develop.swift).
func developController(_ context: Context) throws -> RawDevelopWindowController {
    guard let card = context.card?.contentView,
          let apply = control(card, id: "raw-develop-apply", as: NSButton.self),
          let develop = apply.target as? RawDevelopWindowController else {
        throw ProbeFailure(description: "no Develop RAW card is running")
    }
    return develop
}

/// The readiness check of both Develop cards: the preview is drawn. A refusal by the working
/// limit fails the screen at once instead of waiting out the timeout.
func developReady(_ window: NSWindow, _ context: Context) throws -> Bool {
    guard let card = window.contentView,
          let apply = control(card, id: "raw-develop-apply", as: NSButton.self),
          let develop = apply.target as? RawDevelopWindowController else {
        context.seen = "no Apply button whose target is the Develop controller"
        return false
    }
    let status = control(card, id: "raw-develop-status", as: NSTextField.self)?.stringValue ?? ""
    if status.localizedCaseInsensitiveContains("working limit") {
        throw ProbeFailure(description: "the preview was refused: \(status)")
    }
    guard develop.previewIsReady else {
        context.seen = "previewIsReady false; status \"\(status)\""
        return false
    }
    context.notes.append("status: \(status)")
    return true
}

/// `develop_wait_ready`: a second for the last change to start its preview (a request clears
/// `previewIsReady` as it starts), then until the new preview is drawn.
func developWaitReady(_ context: Context) throws {
    let develop = try developController(context)
    settle(context.card, seconds: 1)
    do {
        try pumpUntil({ develop.previewIsReady }, seconds: 120)
    } catch {
        let status = context.card?.contentView.flatMap { control($0, id: "raw-develop-status", as: NSTextField.self) }
        throw ProbeFailure(description: "the preview was not ready after 120 s; status "
                           + "\"\(status?.stringValue ?? "")\"")
    }
    settle(context.card, seconds: 0.5)
}

/// `develop_scroll`: the card's controls column scrolled so the row named `name` (a row label such
/// as "Sharpness:", or an identifier such as "raw-local-tool") is at its top, clamped at the
/// column's end, as a person scrolls to it.
func developScroll(_ name: String, _ context: Context) throws {
    let develop = try developController(context)
    guard let scroll = develop.controlsScroll, let document = scroll.documentView else {
        throw ProbeFailure(description: "the card has no controls column")
    }
    try require(document.isFlipped, "the controls column is not flipped")
    let row = try developRow(name, in: document)
    let frame = row.convert(row.bounds, to: document)
    let clip = scroll.contentView
    // A few points above the row, so it does not sit against the column's edge.
    let wanted = NSRect(origin: NSPoint(x: 0, y: max(0, frame.minY - 6)), size: clip.bounds.size)
    clip.scroll(to: clip.constrainBoundsRect(wanted).origin)
    scroll.reflectScrolledClipView(clip)
    settle(context.card, seconds: 0.3)
}

/// The one shown view under `root` whose identifier is `name`, or the one shown card row
/// labelled `name`.
private func developRow(_ name: String, in root: NSView) throws -> NSView {
    let shown = descendants(root).filter { !$0.isHiddenOrHasHiddenAncestor }
    let named = shown.filter { $0.accessibilityIdentifier() == name || $0.identifier?.rawValue == name }
    let labelled = shown.compactMap { $0 as? CardControlRow }
        .filter { ($0.arrangedSubviews.first as? NSTextField)?.stringValue == name }
    let found = named.isEmpty ? labelled : named
    guard found.count == 1 else {
        throw ProbeFailure(description: "\(found.count) shown rows are named \(name) in the controls column")
    }
    return found[0]
}
