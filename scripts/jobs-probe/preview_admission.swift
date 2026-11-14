import AppKit

/// A preview worker whose own stages charge its reservation runs it as
/// its own lease. The typed opacity drag reserves P + 64 KiB (P the display
/// estimate) and its worker's display preparation admits P against
/// everything else: with F free, P + 64 KiB ≤ F < 2P + 64 KiB, the preview
/// lands only when the worker leaves its own lease out (`run` counted it
/// beside the P it admits). Then the artwork half: a ⌘T of a text layer at
/// 16 bits (`PrecisionGeometryGesture`) and at 8 bits with a masked layer
/// (`TransformMaskPreview`) holds the artwork stage's scratch as a payload
/// lease, which every admission counts on every thread, while only the core
/// stage is the reservation the worker leaves out.
func verifyPreviewOwnLease() throws {
    let name = "preview own lease"
    let document = try precisionFixture(width: 3000, height: 2000)
    defer { document.close() }
    let controller = try editor(document)
    guard let facade = document.doc else {
        throw ProbeFailure(description: "[\(name)] no 16-bit fixture")
    }
    let display = try facade.projectionScratchEstimate()
    // Earlier cases in a full run may leave preview reservations registered;
    // every drain below is relative to what this case found.
    let reserved0 = SelectionWorkBudget.scratchBytes
    _ = try RasterResources.shared.report(evict: true)
    let free0 = try RasterResources.shared.admit(scratch: 0)
    let target = display + display / 2 + 65_536
    try require(free0 > target, "[\(name)] \(free0) free covers the target \(target)")
    try require(controller.canvas.previewImage == nil, "[\(name)] no preview before the drag")
    let (free, landed) = try withBallast(free0 - target) { () throws -> (UInt64, Bool) in
        let free = try RasterResources.shared.admit(scratch: 0)
        print("INFO: \(name): P \(display), free \(free) with the ballast held, "
              + "\(reserved0) preview bytes registered at entry; "
              + "P + 64 KiB = \(display + 65_536), 2P + 64 KiB = \(2 * display + 65_536)")
        try require(display + 65_536 <= free && free < 2 * display + 65_536,
                    "[\(name)] P + 64 KiB ≤ F < 2P + 64 KiB: P \(display), F \(free)")
        try controller.precisionOpacityScrub(0.5, ended: false)
        let landed = (try? pumpUntil({ controller.canvas.previewImage != nil }, seconds: 20)) != nil
        return (free, landed)
    }
    controller.precisionOpacityDrag?.cancel()
    controller.precisionOpacityDrag = nil
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= reserved0 }, seconds: 10)
    try require(landed, "[\(name)] the opacity drag's preview lands with F = \(free) < 2P + 64 KiB "
                + "(its worker must not count its own P + 64 KiB lease)")
    print("PASS: \(name) — the typed opacity drag's preview lands with \(free) free "
          + "(P \(display); 2P + 64 KiB would refuse) and the registry drains")

    try verifyArtworkLease(depth: 16)
    try verifyArtworkLease(depth: 8)
}

/// The artwork stage's scratch stays counted: while a text
/// layer's ⌘T preview is in flight, its payload lease holds at least
/// `NativeArtworkEvaluation.scratchBytes`' 132 MiB floor (once 48) beside the core
/// reservation; the preview lands, and both leave once the session ends.
private func verifyArtworkLease(depth: Int) throws {
    let tag = "[preview own lease, \(depth)-bit artwork]"
    let document = depth == 8
        ? try fixture(width: 64, height: 64) : try precisionFixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    _ = try agentOK("add_text_layer", document, ["text": "Probe", "x": 4, "y": 4])
    if depth == 8 {
        // An enabled mask routes the 8-bit ⌘T through `TransformMaskPreview`.
        _ = try agentOK("add_layer_mask", document, ["layer": 0, "kind": "reveal_all"])
    }
    guard let withText = document.doc,
          let text = (0..<withText.layerCount).first(where: withText.hasArtworkDescription) else {
        throw ProbeFailure(description: "\(tag) add_text_layer made no text layer")
    }
    let payloads = { () throws -> UInt64 in
        try PrecisionDocumentDescriptor.integer(RasterResources.shared.report(),
                                                "precision_capture_payload_bytes")
    }
    let reserved0 = SelectionWorkBudget.scratchBytes
    let payload0 = try payloads()
    controller.setSelectedLayers(.single(text))
    controller.freeTransform(nil)
    var ended = false
    defer { if !ended { controller.endTransformSession() } }
    controller.transformDegrees = 17
    let pending: () -> Bool
    let status: () -> String
    if depth == 8 {
        guard let preview = controller.transformSession?.corePreview else {
            throw ProbeFailure(description: "\(tag) the masked 8-bit ⌘T has no core preview")
        }
        pending = { preview.isPending }
        status = { preview.status }
    } else {
        guard let gesture = controller.transformSession?.precisionGesture else {
            throw ProbeFailure(description: "\(tag) no 16-bit transform session")
        }
        pending = { gesture.isPending }
        status = { gesture.status }
    }
    let held = try payloads() - payload0
    let registered = SelectionWorkBudget.scratchBytes
    let reserved = registered > reserved0 ? registered - reserved0 : 0
    try require(pending() && reserved > 0 && held >= 132 * 1024 * 1024,
                "\(tag) the in-flight preview holds its artwork stage as a payload lease "
                + "(\(held) bytes ≥ 132 MiB) beside its core reservation (\(reserved))")
    try pumpUntil({ !pending() }, seconds: 60)
    try require(status() == "Preview current", "\(tag) the preview lands: \(status())")
    controller.endTransformSession()
    ended = true
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= reserved0 }, seconds: 10)
    try pumpUntil({ ((try? payloads()) ?? .max) <= payload0 + 1024 * 1024 }, seconds: 10)
    print("PASS: preview own lease, \(depth)-bit artwork — the in-flight ⌘T preview held "
          + "\(held) payload bytes for its artwork stage beside a \(reserved)-byte core "
          + "reservation; it landed and both released")
}

/// Holds `bytes` of admitted payload ballast for `body`'s duration only.
private func withBallast<T>(_ bytes: UInt64, _ body: () throws -> T) throws -> T {
    let ballast = try PrecisionEditPayload.admitted(bytes: bytes)
    return try withExtendedLifetime(ballast) { try body() }
}
