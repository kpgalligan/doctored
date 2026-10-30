import AppKit

// The two compound stack jobs cancel cleanly from MCP. `auto_align_layers`
// (`AutoAlignWork`) and `stack_mode` (`StackModeWork`) each run on a two-layer
// 4000 × 3000 document under a `PanelCanceller`: the call answers
// `JobCancelled.message` in-band and leaves the document, its revision, undo,
// Actions and the alert log untouched (`requireCancelled`, transform_cancel's).
// The same call, uncancelled, then answers ok.

/// The fixture and its duplicate as two identical 4000 × 3000 layers, with the
/// setup steps cleared.
private func twoLayerFixture() throws -> ImageDocument {
    let document = try fixture(width: 4000, height: 3000)
    _ = try agentOK("duplicate_layer", document, ["layer": 0])
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    guard document.doc?.layerCount == 2 else {
        throw ProbeFailure(description: "No two-layer stack source")
    }
    return document
}

/// One MCP call cancelled from its progress panel, then the same call again.
private func cancelThenRun(_ name: String, tool: String, title: String,
                           arguments: [String: Any]) throws -> [String: Any] {
    defer { _ = ActionRecorder.shared.stop() }
    let document = try twoLayerFixture()
    defer { document.close() }
    guard let base = document.doc else {
        throw ProbeFailure(description: "[\(name)] no source document")
    }
    let revision = document.revision
    let alerts = AutomationAlerts.recorded.count
    let canceller = PanelCanceller(title: title)
    ActionRecorder.shared.start()
    let (response, raw) = try transformResponse(tool, document: document, arguments: arguments)
    let steps = ActionRecorder.shared.stop()
    canceller.stop()
    let latency = canceller.latency(name)
    try requireCancelled(name, document, base: base, revision: revision, steps: steps,
                         alerts: alerts, canceller: canceller)
    try require(response["isError"] as? Bool == true && raw.contains(JobCancelled.message),
                "[\(name)] a cancelled \(tool) answers JobCancelled in-band: \(raw)")
    let reply = try agentOK(tool, document, arguments)
    try require(reply["ok"] as? Bool == true, "[\(name)] the uncancelled \(tool) answers ok: \(reply)")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "[\(name)] the uncancelled run released the worker slot and its scratch")
    print("PASS: MCP \(tool) Cancel answers \"\(JobCancelled.message)\" and a retry answers ok "
        + "(\(latency))")
    return reply
}

/// Layer ▸ Auto-Align Layers' job, cancelled from MCP. The two layers are
/// identical, so the uncancelled run registers them and reports the second
/// `unchanged`.
func verifyAutoAlignCancel() throws {
    let reply = try cancelThenRun("auto-align", tool: "auto_align_layers",
                                  title: AutoAlign.title, arguments: ["layers": [0, 1]])
    try require((reply["unregistered"] as? [Any])?.isEmpty == true,
                "[auto-align] identical layers register: \(reply)")
}

/// Layer ▸ Stack Mode's job, cancelled from MCP; the uncancelled run adds the
/// Median layer above the two it read.
func verifyStackModeCancel() throws {
    let reply = try cancelThenRun("stack mode", tool: "stack_mode", title: StackModes.title,
                                  arguments: ["layers": [0, 1], "mode": "median"])
    try require(reply["layer"] as? Int == 2 && reply["name"] as? String == StackMode.median.title,
                "[stack mode] the uncancelled run adds the Median layer at 2: \(reply)")
}

/// One MCP call with no document id, through `execute`: the reply and its text.
private func documentFreeResponse(_ tool: String, _ arguments: [String: Any]) throws
    -> (response: [String: Any], raw: String)
{
    let data = try JSONSerialization.data(withJSONObject: arguments)
    let raw = AgentServer.shared.execute(tool: tool, argumentsJSON: String(decoding: data, as: UTF8.self))
    guard let response = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Malformed \(tool) response: \(raw)")
    }
    return (response, raw)
}

/// File ▸ Automate ▸ Merge Panorama's job, cancelled from MCP: two
/// identical 4000 × 3000 PNGs, `merge_panorama` with Blend on under a
/// `PanelCanceller` on the job's own panel. The call answers `JobCancelled`
/// in-band, opens no document and records no Action, and leaves the worker slot
/// free and the alert log as it was. The same call, uncancelled, registers the
/// identical frames (nothing `unregistered`) and opens one document.
func verifyMergePanoramaCancel() throws {
    let name = "merge_panorama"
    defer { _ = ActionRecorder.shared.stop() }
    let folder = try uiCaseFolder("merge-panorama-cancel")
    let (width, height) = (4000, 3000)
    var rgba = [UInt8](repeating: 255, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let p = (y * width + x) * 4
            rgba[p] = UInt8(40 + (x * 3 + y * 5) % 170)
            rgba[p + 1] = UInt8(40 + (x * 7 + y * 11) % 170)
            rgba[p + 2] = UInt8(40 + (x * 13 + y * 3) % 170)
        }
    }
    let first = folder.appendingPathComponent("frame-a.png")
    try uiWritePNG(rgba, width: width, height: height, to: first)
    let second = folder.appendingPathComponent("frame-b.png")
    try FileManager.default.copyItem(at: first, to: second)
    let arguments: [String: Any] = ["paths": [first.path, second.path], "blend": true]
    let before = NSDocumentController.shared.documents
    let alerts = AutomationAlerts.recorded.count
    let canceller = PanelCanceller(title: MergePanorama.title)
    ActionRecorder.shared.start()
    let (response, raw) = try documentFreeResponse("merge_panorama", arguments)
    let steps = ActionRecorder.shared.stop()
    canceller.stop()
    let latency = canceller.latency(name)
    try require(canceller.clickedAt != nil,
                "[\(name)] the panel's Cancel was never clicked (worker seen: \(canceller.sawOccupied), "
                + "reply: \(raw.prefix(300)))")
    try require(response["isError"] as? Bool == true && raw.contains(JobCancelled.message),
                "[\(name)] a cancelled merge_panorama answers JobCancelled in-band: \(raw)")
    let leftOpen = uiNewDocuments(since: before)
    leftOpen.forEach { $0.close() }
    try require(leftOpen.isEmpty, "[\(name)] Cancel opened no document")
    try require(steps.isEmpty, "[\(name)] Cancel recorded no Action: \(steps.map(\.tool))")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "[\(name)] Cancel released the worker slot and its scratch")
    try require(AutomationAlerts.recorded.count == alerts,
                "[\(name)] Cancel presented no alert or error: \(AutomationAlerts.recorded.suffix(2))")
    let (answer, text) = try documentFreeResponse("merge_panorama", arguments)
    let made = uiNewDocuments(since: before)
    defer { made.forEach { $0.close() } }
    try require(answer["isError"] as? Bool != true, "[\(name)] the uncancelled merge_panorama refused: \(text)")
    guard let content = answer["content"] as? [[String: Any]],
          let body = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String,
          let reply = try JSONSerialization.jsonObject(with: Data(body.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "[\(name)] malformed merge_panorama reply: \(text)")
    }
    try require(reply["ok"] as? Bool == true, "[\(name)] the uncancelled merge_panorama answers ok: \(reply)")
    let align = reply["align"] as? [String: Any]
    try require((align?["unregistered"] as? [Any])?.isEmpty == true,
                "[\(name)] identical frames register: \(align ?? [:])")
    try require(made.count == 1, "[\(name)] the uncancelled merge_panorama opens one document: \(made.count)")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "[\(name)] the uncancelled run released the worker slot and its scratch")
    print("PASS: MCP merge_panorama Cancel answers \"\(JobCancelled.message)\" and opens nothing, and a "
        + "retry answers ok with one new document (\(latency))")
}

/// Image ▸ Adjustments ▸ HDR Toning's job, cancelled from MCP: `hdr_toning`
/// (Equalize Histogram) on the two-layer 4000 × 3000 8-bit document — promote, tone, narrow — under
/// a `PanelCanceller` on the job's own panel. The call answers `JobCancelled` in-band with the
/// document untouched; the same call, uncancelled, flattens it to one 8-bit layer.
func verifyHdrToningCancel() throws {
    let reply = try cancelThenRun("hdr toning", tool: "hdr_toning", title: HdrToning.title,
                                  arguments: ["method": "equalize_histogram"])
    let summary = reply["document"] as? [String: Any]
    try require(reply["flattened"] as? Bool == true && reply["bit_depth"] as? Int == 8
                    && summary?["layer_count"] as? Int == 1,
                "[hdr toning] the uncancelled run flattens to one 8-bit layer: \(reply)")
}
