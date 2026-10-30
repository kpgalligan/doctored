import AppKit

// A canvas-session commit refused by a beep, a declined
// rasterize prompt or a cancelled transform job names its reason to the
// agent (`SessionCommitRefusal`), not the generic sentence. Every case calls
// methods directly, so none needs a key window.

/// The sentence `body` noted inside a capturing scope.
private func captured(_ body: () -> Void) -> String? {
    SessionCommitRefusal.capturing {
        body()
        return false
    }.sentence
}

func verifySessionRefusalSentences() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "Session refusal fixture") }

    // (a) beep notes its sentence.
    try require(captured { SessionCommitRefusal.beep("s") } == "s", "(a) beep(\"s\") gives \"s\"")

    // (b) applyEdit's two beeps.
    let refused = captured { document.applyEdit("Probe", record: .notACommand) { _ in nil } }
    try require(refused == "Probe was refused and nothing changed.",
                "(b) a nil transform names the action: \(refused ?? "nil")")
    guard let other = try base.withLayerName(0, "Other") else {
        throw ProbeFailure(description: "(b) rename produced no handle")
    }
    let changed = captured {
        document.applyEdit("Probe", record: .notACommand) { _ in
            document.doc = other
            return other
        }
    }
    document.doc = base
    try require(changed == "The document changed while Probe was running. Nothing changed.",
                "(b) a document changed under the transform: \(changed ?? "nil")")
    document.beginLiveEdit()
    let entry = captured { document.applyEdit("Probe", record: .notACommand) { _ in other } }
    document.abandonLiveEdit(ownedBy: base)
    try require(entry == "Finish the current gesture or edit first. Nothing changed.",
                "(b) the entry guard: \(entry ?? "nil")")
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "(b) no applyEdit refusal left an undo step")

    // (c) The preflight's depth guard and the SilentEditRefusal presentation.
    let samples = [UInt16](repeating: 32768, count: 64 * 64 * 4)
    let deep = try RasterDocument.fromRawRGBA16(samples, width: 64, height: 64,
                                                profile: RasterProfile.builtin(.sRGB))
    var admitted = true
    let depth = captured { admitted = document.admitsPublication(deep, record: []) }
    try require(!admitted && depth == "The edit was refused and nothing changed.",
                "(c) the depth guard: \(admitted) \(depth ?? "nil")")
    AutomationAlerts.reset()
    let presented = captured { document.presentError(SilentEditRefusal(message: "p")) }
    try require(presented == "p" && AutomationAlerts.recorded.isEmpty,
                "(c) a presented SilentEditRefusal: \(presented ?? "nil") \(AutomationAlerts.recorded)")

    // (d) A declined rasterize prompt names the layer.
    _ = try agentOK("add_text_layer", document, ["text": "Probe", "x": 4, "y": 4])
    guard let doc = document.doc,
          let text = (0..<doc.layerCount).first(where: { document.layerDescribesSource($0) }),
          let name = doc.layerInfo(text)?.name else {
        throw ProbeFailure(description: "(d) add_text_layer made no text layer")
    }
    guard let method = class_getInstanceMethod(NSAlert.self, #selector(NSAlert.runModal)) else {
        throw ProbeFailure(description: "(d) NSAlert.runModal missing")
    }
    let second: @convention(block) (AnyObject) -> Int = { _ in
        NSApplication.ModalResponse.alertSecondButtonReturn.rawValue
    }
    let replacement = imp_implementationWithBlock(second)
    let original = method_setImplementation(method, replacement)
    var confirmed = true
    let declined = captured { confirmed = document.confirmRasterize(layer: text) }
    method_setImplementation(method, original)
    imp_removeBlock(replacement)
    try require(!confirmed && declined?.contains("“\(name)”") == true,
                "(d) the declined prompt names the layer: \(confirmed) \(declined ?? "nil")")

    // (e) A cancelled transform job, end to end through an MCP writer.
    try verifyCancelledSessionCommit()
    print("PASS: session refusal sentences — beep, applyEdit's refusals, the depth guard, a "
        + "presented silent refusal, a declined prompt and a cancelled transform job name "
        + "their reason")
}

private func verifyCancelledSessionCommit() throws {
    let document = try fixture(width: 4000, height: 3000)
    defer { document.close() }
    let controller = try editor(document)
    _ = try transformResponse("add_layer_mask", document: document,
                              arguments: ["layer": 0, "kind": "reveal_all"])
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    guard let base = document.doc else { throw ProbeFailure(description: "(e) no masked source") }
    controller.freeTransform(nil)
    controller.transformDegrees = 17
    controller.transformSampler = DR_FILTER_LANCZOS3
    controller.transformSession?.sampler = DR_FILTER_LANCZOS3
    try require(controller.transformSession != nil, "(e) Free Transform session open")
    try pumpUntil({ controller.transformSession?.corePreview?.isPending != true }, seconds: 120)
    let destination = directory.appendingPathComponent("cancelled.rz")
    try? FileManager.default.removeItem(at: destination)
    let canceller = PanelCanceller(title: "Transform Layer")
    let reply = try agentCall("save_copy", document, ["path": destination.path])
    canceller.stop()
    try require(canceller.clickedAt != nil, "(e) the panel's Cancel was clicked")
    try require(reply.isError && reply.text.contains("could not be committed")
                    && reply.text.contains(JobCancelled.message),
                "(e) save_copy names the cancelled job: \(reply.text)")
    try require(controller.isTransforming, "(e) the session stays open")
    try require(document.undoManager?.canUndo != true && document.doc === base,
                "(e) no undo step and no edit")
    try require(!FileManager.default.fileExists(atPath: destination.path), "(e) no file written")
}
