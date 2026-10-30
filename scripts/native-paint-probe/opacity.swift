import AppKit
import CryptoKit

// Phase 12E-2 (P7-27): the UI-SOURCE half of the continuous OPACITY drag. An
// MCP drive can only set an opacity; what only a gesture can prove lives
// here — that the drag's PREVIEW is the very picture its release commits,
// that a whole gesture is ONE undo step, that a gesture whose document moved
// under it publishes NOTHING and says so, that the panel is left showing the
// DOCUMENT's state rather than the abandoned drag value, and that a release
// mid-preview lands no late preview and leaks no preview reservation.

/// A CGImage's pixels, drawn into a fixed RGBA8 context so two images from
/// two different paths are compared on the same terms.
private func previewBytes(_ image: CGImage) throws -> [UInt8] {
    let width = image.width, height = image.height
    var bytes = [UInt8](repeating: 0, count: width * height * 4)
    let ok: Bool = bytes.withUnsafeMutableBytes { raw -> Bool in
        guard let base = raw.baseAddress,
              let context = CGContext(
                data: base, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        return true
    }
    guard ok else { throw DoctoredCoreError(message: "Owned preview raster unavailable") }
    return bytes
}

private func previewHash(_ image: CGImage) throws -> String {
    hashBytes(try previewBytes(image))
}

/// The document's own display picture right now — what a COMMIT looks like.
private func committedPreview(_ document: ImageDocument) throws -> CGImage {
    guard let doc = document.doc, let preview = PreparedDocumentEdit(doc).preview else {
        throw DoctoredCoreError(message: "Owned committed preview unavailable")
    }
    return preview
}

private func layerOpacity(_ document: ImageDocument, _ index: Int) throws -> Double {
    guard let value = document.doc?.layerInfo(index)?.opacity else {
        throw DoctoredCoreError(message: "Owned layer opacity unavailable")
    }
    return Double(value)
}

/// The stored opacity is a `Float`, so a value like 0.2 is compared at the
/// precision it is actually kept in rather than with `==`.
private func opacityIs(_ document: ImageDocument, _ index: Int, _ want: Double) throws -> Bool {
    abs(try layerOpacity(document, index) - want) <= Double(Float.ulpOfOne) * 4
}

/// The hash of whatever preview is on the canvas right now, or nil.
private func currentPreview(_ editor: EditorViewController) -> String? {
    guard let image = editor.canvas.previewImage else { return nil }
    return try? previewHash(image)
}

/// Pumps until the drag's preview image has landed (or the deadline passes).
private func waitForPreview(_ editor: EditorViewController) throws {
    try waitUntil({ editor.canvas.previewImage != nil }, "The typed drag never delivered a preview")
}

/// Pumps until a preview DIFFERENT from `previous` has landed: the renderer
/// debounces and coalesces, so "a preview exists" is satisfied by the tick
/// before this one and would compare the wrong picture.
private func waitForNewPreview(_ editor: EditorViewController, after previous: String?,
                               _ what: String) throws -> String {
    try waitUntil({
        let now = currentPreview(editor)
        return now != nil && now != previous
    }, "The typed drag never delivered a new preview for \(what)")
    guard let landed = currentPreview(editor) else {
        throw DoctoredCoreError(message: "Owned drag preview missing")
    }
    return landed
}

/// opacity-drag-ui.
///
/// A 16-bit document: three preview ticks compared against a twin document
/// COMMITTED at the same opacity, one undo step at release, two stale
/// gestures (one ended through the controller, one through the panel) and a
/// release mid-preview.
func opacityDragUI(output: URL) throws -> [String: Any] {
    let (document, editor, _) = try openDocument()
    let (twin, _, _) = try openDocument()
    defer { twin.close(); document.close(); pump() }
    guard let panel = editor.layersPanel else {
        throw DoctoredCoreError(message: "Actual layers panel missing")
    }
    _ = panel.view
    try require(document.doc?.isHighDepth == true, "Owned fixture is not a 16-bit document")
    let layer = document.activeLayerIndex
    document.undoManager?.removeAllActions()

    // --- Three preview ticks, each equal to the twin's COMMITTED picture.
    var previewHashes: [String] = []
    var lastPreview: String?
    for value in [0.25, 0.5, 0.75] {
        panel.scrubOpacity(value, ended: false)
        let drag = try waitForNewPreview(editor, after: lastPreview, "\(value)")
        lastPreview = drag
        let response = try call("set_layer_properties",
                                ["index": layer, "opacity": value], document: twin)
        try require(!(try responseIsError(response)), "twin set_layer_properties refused: \(response)")
        pump()
        let committed = try committedPreview(twin)
        try require(drag == (try previewHash(committed)),
            "The drag preview at \(value) is not the committed picture")
        previewHashes.append(drag)
        // The document itself has NOT moved: a typed drag previews only.
        try require(try opacityIs(document, layer, 1.0),
            "A typed drag tick mutated the document before release")
    }
    try require(Set(previewHashes).count == 3, "Three opacities produced the same preview")

    // --- Release: exactly ONE history step, and one Undo restores.
    let before = document.history.entries.count
    panel.scrubOpacity(0.75, ended: true)
    pump()
    try require(document.history.entries.count == before + 1,
        "A whole gesture published \(document.history.entries.count - before) history steps")
    try require(try opacityIs(document, layer, 0.75), "The release did not commit 0.75")
    try require(editor.canvas.previewImage == nil, "The release left a preview on the canvas")
    document.undoManager?.undo(); pump()
    try require(try opacityIs(document, layer, 1.0), "One Undo did not restore the opacity")
    document.undoManager?.removeAllActions()

    // --- A STALE gesture ended through the CONTROLLER: it throws, publishes
    // nothing, and leaves no preview.
    panel.scrubOpacity(0.6, ended: false)
    try waitForPreview(editor)
    let staleBefore = document.history.entries.count
    let foreign = try call("set_layer_properties", ["index": layer, "opacity": 0.2],
                           document: document)
    try require(!(try responseIsError(foreign)), "The foreign edit refused: \(foreign)")
    pump()
    var threw = false
    do { try editor.precisionOpacityScrub(0.6, ended: true) } catch { threw = true }
    pump()
    try require(threw, "A stale typed gesture did not refuse")
    try require(document.history.entries.count == staleBefore + 1,
        "A stale gesture published a step beside the foreign edit")
    try require(try opacityIs(document, layer, 0.2),
        "A stale gesture overwrote the foreign edit's opacity")
    try require(editor.canvas.previewImage == nil, "A refused gesture left a preview")
    try require(editor.precisionOpacityDrag == nil, "A refused gesture kept its session")

    // --- The same scenario ended through the PANEL: the sentence reaches the
    // app's error presenter, nothing is published, and the slider is left
    // showing the DOCUMENT's opacity rather than the abandoned drag value.
    guard let app = NSApp as? ProbeApplication else {
        throw DoctoredCoreError(message: "Owned probe application missing")
    }
    app.errors.removeAll()
    panel.scrubOpacity(0.9, ended: false)
    try waitForPreview(editor)
    let panelBefore = document.history.entries.count
    let foreign2 = try call("set_layer_properties", ["index": layer, "opacity": 0.4],
                            document: document)
    try require(!(try responseIsError(foreign2)), "The second foreign edit refused: \(foreign2)")
    pump()
    panel.scrubOpacity(0.9, ended: true)
    pump()
    try require(document.history.entries.count == panelBefore + 1,
        "The panel's stale release published a step")
    try require(try opacityIs(document, layer, 0.4),
        "The panel's stale release overwrote the foreign edit")
    let sentence = "The document, target or selection changed while the 16-bit edit was running."
    try require(app.errors.contains { $0.contains(sentence) },
        "The stale sentence never reached the presenter: \(app.errors)")
    try require(abs(panel.opacitySliderValue - (try layerOpacity(document, layer))) < 1e-9,
        "The slider kept the abandoned drag value \(panel.opacitySliderValue)")

    // --- Release MID-PREVIEW: no pump between the tick and the release.
    document.undoManager?.removeAllActions()
    let midBefore = document.history.entries.count
    panel.scrubOpacity(0.75, ended: false)
    panel.scrubOpacity(0.75, ended: true)
    try require(document.history.entries.count == midBefore + 1,
        "A release mid-preview published \(document.history.entries.count - midBefore) steps")
    pump(); pump()
    try require(editor.canvas.previewImage == nil, "A late preview landed after the release")
    // The cancelled preview's reservation lives until the renderer's own
    // debounce work item is released, which happens on the MAIN queue at its
    // scheduled deadline — so this pumps until it drains rather than
    // asserting on one tick's timing. The bound is 0, not a tolerance.
    try waitUntil({ SelectionWorkBudget.scratchBytes == 0 },
        "The preview reservation outlived the gesture: \(SelectionWorkBudget.scratchBytes)")
    try require(try opacityIs(document, layer, 0.75), "The mid-preview release did not commit")

    return ["case": "opacity-drag-ui", "preview_hashes": previewHashes,
            "preview_equals_commit": true, "one_undo_step": true,
            "stale_controller_refused": true, "stale_panel_refused": true,
            "slider_shows_document": panel.opacitySliderValue,
            "release_mid_preview_steps": 1,
            "scratch_after": SelectionWorkBudget.scratchBytes]
}

/// byte-opacity-drag-ui.
///
/// The 8-bit twin of the three-opacity/one-undo checks, plus the byte
/// staleness rule: a byte gesture holds `liveEditBase`, so a foreign MCP edit
/// is refused BY NAME instead of the gesture going stale.
func byteOpacityDragUI(output: URL) throws -> [String: Any] {
    let (document, editor, _) = try openByteDocument()
    let (twin, _, _) = try openByteDocument()
    defer { twin.close(); document.close(); pump() }
    guard let panel = editor.layersPanel else {
        throw DoctoredCoreError(message: "Actual layers panel missing")
    }
    _ = panel.view
    let layer = document.activeLayerIndex
    document.undoManager?.removeAllActions()

    // --- Three live ticks: the byte path swaps the document live, so the
    // gesture's picture IS the document's, compared against a twin COMMITTED
    // at the same opacity through MCP.
    var hashes: [String] = []
    for value in [0.25, 0.5, 0.75] {
        panel.scrubOpacity(value, ended: false)
        pump()
        let response = try call("set_layer_properties",
                                ["index": layer, "opacity": value], document: twin)
        try require(!(try responseIsError(response)), "twin set_layer_properties refused: \(response)")
        pump()
        guard let live = document.doc, let committed = twin.doc else {
            throw DoctoredCoreError(message: "Owned byte documents unavailable")
        }
        let hash = hashBytes(try flatBytes(live))
        try require(hash == hashBytes(try flatBytes(committed)),
            "The byte drag's picture at \(value) is not the committed one")
        hashes.append(hash)
    }
    try require(Set(hashes).count == 3, "Three byte opacities produced the same composite")

    // --- A foreign MCP edit mid-gesture is refused BY NAME.
    let refused = try call("set_layer_properties", ["index": layer, "opacity": 0.1],
                           document: document)
    try require(try responseIsError(refused), "A mid-gesture MCP edit was admitted: \(refused)")
    try require(refused.contains("Finish or cancel the active canvas gesture"),
        "The byte refusal did not name the gesture: \(refused)")

    // --- Release: one history step, one Undo restores.
    let before = document.history.entries.count
    panel.scrubOpacity(0.75, ended: true)
    pump()
    try require(document.history.entries.count == before + 1,
        "A whole byte gesture published \(document.history.entries.count - before) history steps")
    try require(try opacityIs(document, layer, 0.75), "The byte release did not commit 0.75")
    document.undoManager?.undo(); pump()
    try require(try opacityIs(document, layer, 1.0),
        "One Undo did not restore the byte opacity")

    return ["case": "byte-opacity-drag-ui", "composite_hashes": hashes,
            "live_equals_commit": true, "one_undo_step": true,
            "mid_drag_mcp_refused_by_name": true]
}
