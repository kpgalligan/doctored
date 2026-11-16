import AppKit

// "text ink rect" (designs/text-ink-rect.md, TextInkRect.swift): a straight text layer's raster
// covers its glyphs' ink. (a) Zapfino's swash leaves the raster's outermost ring clear in every
// layout, (a-cmyk) and in a CMYK 8 document; (b) the payload codec, versions 1–7; (c) the marked
// rect; (d) a layer saved before the marker keeps its anchor and is marked at it by an edit, a
// transform and Replace Missing Fonts; (d-marked) Replace Missing Fonts recentres a marked layer's
// refitted rect; (d-lock) a transparency lock defers the marker; (e) an unchanged text-session
// commit adds no step. Each section reports its own failure, so one run names every section a
// mutant turns red.

/// The deepest `alpha` on a `width` × `height` raster's outermost rows and columns.
private func textInkRing(_ width: Int, _ height: Int, _ alpha: (Int, Int) -> Int) -> Int {
    var most = 0
    for x in 0..<width { most = max(most, alpha(x, 0), alpha(x, height - 1)) }
    for y in 0..<height { most = max(most, alpha(0, y), alpha(width - 1, y)) }
    return most
}

/// The deepest alpha on layer `layer`'s outermost rows and columns: 0 when nothing is cut.
private func textInkRingAlpha(_ document: ImageDocument, _ layer: Int) -> Int? {
    guard let doc = document.doc, let info = doc.layerInfo(layer), let pixels = doc.layerPixels(layer),
          info.width > 0, info.height > 0, pixels.count == info.width * info.height * 4 else { return nil }
    return textInkRing(info.width, info.height) { x, y in Int(pixels[(y * info.width + x) * 4 + 3]) }
}

/// The same for a CMYK document's layer, read through its full-size Layers-panel thumbnail: the
/// CMYK view of the layer's own raster, its alpha kept (`cmyk_layer_thumbnail_rgba8`).
private func cmykTextInkRingAlpha(_ doc: RasterDocument, _ layer: Int) -> Int? {
    guard let info = doc.layerInfo(layer), info.width > 0, info.height > 0,
          let image = doc.layerThumbnail(layer, maxSide: max(info.width, info.height)),
          image.width == info.width, image.height == info.height else { return nil }
    return textInkRing(info.width, info.height) { x, y in Int(image.pixelRGBA(x: x, y: y)?.a ?? 255) }
}

/// Zapfino's "Zapfino", 48 pt, at (40, 60), point text: the ligature whose swash dips far below
/// its baseline and past its advance.
private let zapfinoLayer: [String: Any] = [
    "text": "Zapfino", "font": "Zapfino", "size": 48, "x": 40, "y": 60, "wrap_width": 0,
    "rgba": [0, 0, 0, 1],
]

/// Layer `layer`'s meta, `[x, y, width, height]` and straight RGBA pixels.
private func textInkState(_ doc: RasterDocument?, _ layer: Int) -> (meta: String, rect: [Int], pixels: [UInt8]) {
    let info = doc?.layerInfo(layer)
    return (doc?.layerMeta(layer) ?? "-",
            info.map { [$0.offsetX, $0.offsetY, $0.width, $0.height] } ?? [],
            doc?.layerPixels(layer) ?? [])
}

/// `doc` with its marked text layer `layer` cut back to what a build before `ink_outset` saved:
/// the pixels cropped to the unmarked rule's rect at the same anchor and the meta without the
/// field, `patch` applied to it.
private func legacyShaped(_ doc: RasterDocument, _ layer: Int,
                          patch: (inout TextLayerPayload) -> Void = { _ in }) throws -> RasterDocument {
    guard let meta = doc.layerMeta(layer), let marked = TextLayerPayload.decode(meta),
          marked.inkOutset != nil, let anchor = doc.describedAnchor(layer),
          let info = doc.layerInfo(layer), let pixels = doc.layerPixels(layer) else {
        throw ProbeFailure(description: "[text ink rect] layer \(layer) is not a marked text layer")
    }
    var unmarked = marked.withInkOutset(nil)
    guard let rect = LayerDescription.text(unmarked).rasterRect() else {
        throw ProbeFailure(description: "[text ink rect] no unmarked rect")
    }
    let x = Int(anchor.x.rounded(.down)) + rect.originX
    let y = Int(anchor.y.rounded(.down)) + rect.originY
    guard x >= info.offsetX, y >= info.offsetY, x + rect.width <= info.offsetX + info.width,
          y + rect.height <= info.offsetY + info.height else {
        throw ProbeFailure(description: "[text ink rect] the unmarked rect is not inside the marked one")
    }
    var cropped: [UInt8] = []
    cropped.reserveCapacity(rect.width * rect.height * 4)
    for row in 0..<rect.height {
        let start = ((y - info.offsetY + row) * info.width + (x - info.offsetX)) * 4
        cropped.append(contentsOf: pixels[start..<(start + rect.width * 4)])
    }
    patch(&unmarked)
    guard let json = unmarked.json(),
          let shaped = doc.setLayerContent(layer, rgba: cropped, width: rect.width, height: rect.height,
                                           offsetX: x, offsetY: y, mask: nil)?.withLayerMeta(layer, json)
    else { throw ProbeFailure(description: "[text ink rect] could not cut the layer back") }
    return shaped
}

/// Whether some fully opaque pixel of `pixels` is `rgb`.
private func inks(_ pixels: [UInt8], _ rgb: (UInt8, UInt8, UInt8)) -> Bool {
    stride(from: 0, to: pixels.count - 3, by: 4).contains {
        pixels[$0 + 3] == 255 && pixels[$0] == rgb.0 && pixels[$0 + 1] == rgb.1 && pixels[$0 + 2] == rgb.2
    }
}

func verifyTextInkRect() throws {
    let tag = "[text ink rect]"
    var failures: [String] = []
    func check(_ section: String, _ body: () throws -> Void) {
        do { try body() } catch { failures.append("(\(section)) \(error)") }
    }

    // (a) RING: no layout cuts the swash.
    check("a") {
        let document = try fixture(width: 640, height: 480)
        defer { document.close() }
        let angle = Double.pi / 6
        let variants: [(String, [String: Any])] = [
            ("left", [:]),
            ("center", ["text": "Zapfino\nZa", "alignment": "center"]),
            ("right", ["text": "Zapfino\nZa", "alignment": "right"]),
            ("baseline +20", ["baseline_shift": 20]),
            ("baseline -20", ["baseline_shift": -20]),
            ("leading 30", ["text": "Zapfino\nZapfino", "leading": 30]),
            ("two lines", ["text": "Zapfino\nZapfino"]),
            ("tracking 5", ["tracking": 5]),
            ("30°", ["transform": [cos(angle), -sin(angle), sin(angle), cos(angle)]]),
        ]
        var cut: [String] = []
        for (name, extra) in variants {
            let made = try agentOK("add_text_layer", document, zapfinoLayer.merging(extra) { $1 })
            guard let layer = made["layer"] as? Int, let ring = textInkRingAlpha(document, layer) else {
                throw ProbeFailure(description: "\(name): no layer: \(made)")
            }
            if ring > 0 { cut.append("\(name) (alpha \(ring))") }
        }
        try require(cut.isEmpty, "ink on the raster's outermost ring: \(cut)")
    }

    // (a-cmyk) The CMYK arm stamps too: the same swash in a CMYK 8 document is marked, in ink, and
    // leaves its raster's outermost ring clear.
    check("a-cmyk") {
        let document = try fixture(width: 640, height: 480)
        defer { document.close() }
        _ = try editor(document)
        _ = try agentOK("convert_mode", document, [
            "mode": "cmyk", "profile": "file",
            "path": "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc"])
        guard document.doc?.isCmyk == true else { throw ProbeFailure(description: "not CMYK") }
        let made = try agentOK("add_text_layer", document, zapfinoLayer)
        guard let layer = made["layer"] as? Int, let doc = document.doc,
              let payload = doc.textPayload(layer), let ring = cmykTextInkRingAlpha(doc, layer) else {
            throw ProbeFailure(description: "no CMYK Zapfino layer: \(made)")
        }
        try require(payload.inkOutset != nil && payload.artwork?.ink == .cmyk && ring == 0,
                    "CMYK 8: outset \(String(describing: payload.inkOutset)), ink "
                    + "\(String(describing: payload.artwork?.ink)), ring alpha \(ring)")
    }

    // (b) The codec: versions 1–6 decode and re-encode byte for byte; a version 7 round-trips.
    let artwork = "\"artwork\":{\"evaluation\":\"outline_area_v1\","
        + "\"intrinsicGlyphs\":\"platform_byte_assets_v1\",\"rgba\":[0,0,0,1],\"runs\":[]},"
    let tail = "\"italic\":false,\"leading\":0,\"origin_frac\":[0,0],\"size\":24,\"strikethrough\":false,"
        + "\"string\":\"Hi\",\"tracking\":0,\"transform\":[1,0,0,1],\"type\":\"text\",\"underline\":false,"
    let v3 = "{\"alignment\":\"left\"," + artwork + "\"baseline_shift\":0,\"box_width\":0,"
        + "\"color\":\"#000000FF\",\"font\":\"Helvetica\"," + tail + "\"version\":3,\"weight\":5}"
    let v7 = v3.replacingOccurrences(of: "\"font\":\"Helvetica\",",
                                     with: "\"font\":\"Helvetica\",\"ink_outset\":[1,2,3,4],")
        .replacingOccurrences(of: "\"version\":3", with: "\"version\":7")
    check("b") {
        let path = "\"on_path\":{\"offset\":0,\"outline\":{\"fill_rule\":\"nonzero\",\"subpaths\":"
            + "[{\"anchors\":[[0,0,0,0,0,0,0],[100,0,100,0,100,0,0]],\"closed\":false}]},"
            + "\"side\":\"left\"},"
        let kept: [(String, String)] = [
            ("1", "{\"alignment\":\"left\",\"box_width\":0,\"color\":\"#000000FF\",\"font\":\"Helvetica\","
                + "\"size\":24,\"string\":\"Hi\",\"type\":\"text\",\"version\":1}"),
            ("2", "{\"alignment\":\"center\",\"baseline_shift\":0,\"box_width\":120,"
                + "\"color\":\"#102030FF\",\"font\":\"Helvetica\",\"italic\":true,\"leading\":0,"
                + "\"origin_frac\":[0,0],\"size\":24,\"strikethrough\":false,\"string\":\"Hi\","
                + "\"tracking\":0,\"transform\":[1,0,0,1],\"type\":\"text\",\"underline\":false,"
                + "\"version\":2,\"weight\":5}"),
            ("3", v3),
            ("4", v3.replacingOccurrences(of: artwork, with: "\"artwork\":{\"evaluation\":"
                + "\"outline_area_v1\",\"ink\":\"linear_srgb\",\"intrinsicGlyphs\":"
                + "\"platform_byte_assets_v1\",\"rgba\":[0.5,0,0,1],\"runs\":[]},")
                .replacingOccurrences(of: "#000000FF", with: "#800000FF")
                .replacingOccurrences(of: "\"version\":3", with: "\"version\":4")),
            ("5", v3.replacingOccurrences(of: artwork, with: "\"artwork\":{\"evaluation\":"
                + "\"outline_area_v1\",\"ink\":\"cmyk\",\"intrinsicGlyphs\":"
                + "\"platform_byte_assets_v1\",\"rgba\":[0,0,0,1,1],\"runs\":[]},")
                .replacingOccurrences(of: "#000000FF", with: "#231F20FF")
                .replacingOccurrences(of: "\"version\":3", with: "\"version\":5")),
            ("6", v3.replacingOccurrences(of: "\"leading\":0,", with: "\"leading\":0," + path)
                .replacingOccurrences(of: "\"version\":3", with: "\"version\":6")),
            ("7", v7),
        ]
        var wrong: [String] = []
        for (version, literal) in kept {
            let decoded = TextLayerPayload.decode(literal)
            if decoded == nil || decoded?.json() != literal {
                wrong.append("version \(version) \(decoded?.json() ?? "refused")")
            }
        }
        try require(TextLayerPayload.decode(v7)?.inkOutset?.array == [1, 2, 3, 4],
                    "version 7 reads its ink_outset: \(String(describing: TextLayerPayload.decode(v7)))")
        let refused: [(String, String)] = [
            ("7 without the field", v3.replacingOccurrences(of: "\"version\":3", with: "\"version\":7")),
            ("3 with the field", v7.replacingOccurrences(of: "\"version\":7", with: "\"version\":3")),
            ("all zero", v7.replacingOccurrences(of: "[1,2,3,4]", with: "[0,0,0,0]")),
            ("a negative", v7.replacingOccurrences(of: "[1,2,3,4]", with: "[-1,2,3,4]")),
            ("three elements", v7.replacingOccurrences(of: "[1,2,3,4]", with: "[1,2,3]")),
            ("a fraction", v7.replacingOccurrences(of: "[1,2,3,4]", with: "[1.5,2,3,4]")),
            ("7 without box_width", v7.replacingOccurrences(of: "\"box_width\":0,", with: "")),
            ("7 without artwork", v7.replacingOccurrences(of: artwork, with: "")),
        ]
        for (name, literal) in refused where TextLayerPayload.decode(literal) != nil {
            wrong.append("\(name) decoded")
        }
        try require(wrong.isEmpty, "codec: \(wrong)")
    }

    // (c) A version 7's rect is its unmarked rect grown by the four numbers; ink that fits is
    // never marked.
    check("c") {
        guard let marked = TextLayerPayload.decode(v7),
              let grown = TextLayer.sourceRect(marked),
              let plain = TextLayer.sourceRect(marked.withInkOutset(nil)) else {
            throw ProbeFailure(description: "no version 7 rect")
        }
        let want = CGRect(x: plain.minX - 1, y: plain.minY - 2, width: plain.width + 4,
                          height: plain.height + 6)
        try require(grown == want, "the version 7 rect \(grown) is not \(plain) grown: \(want)")
        let document = try fixture(width: 640, height: 480)
        defer { document.close() }
        let made = try agentOK("add_text_layer", document, [
            "text": "Hello", "font": "Helvetica Neue", "size": 24, "x": 40, "y": 300,
            "wrap_width": 0, "rgba": [0, 0, 0, 1]])
        guard let layer = made["layer"] as? Int, let doc = document.doc,
              let meta = doc.layerMeta(layer), let payload = TextLayerPayload.decode(meta),
              let rect = LayerDescription.text(payload).rasterRect(), let info = doc.layerInfo(layer) else {
            throw ProbeFailure(description: "no Hello layer: \(made)")
        }
        try require(!meta.contains("ink_outset"), "Hello was marked: \(meta)")
        let got: [Int] = [info.offsetX, info.offsetY, info.width, info.height]
        let unmarked: [Int] = [40 + rect.originX, 300 + rect.originY, rect.width, rect.height]
        try require(got == unmarked, "Hello's raster \(got) is not the unmarked rule's rect \(unmarked)")
    }

    // (d), (d-lock), (e): one marked Zapfino layer and its saved-before-the-marker twin.
    let document = try fixture(width: 640, height: 480)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let made = try agentOK("add_text_layer", document, zapfinoLayer)
    guard let index = made["layer"] as? Int, let marked = document.doc,
          let anchor = marked.describedAnchor(index) else {
        throw ProbeFailure(description: "\(tag) no Zapfino layer: \(made)")
    }
    let direct = textInkState(marked, index)
    let markedOutset = marked.layerMeta(index).flatMap(TextLayerPayload.decode)?.inkOutset
    // The leading-30 twin recentres its y in Replace Missing Fonts (MissingFonts.swift).
    let leaded = try agentOK("add_text_layer", document, zapfinoLayer.merging(["leading": 30]) { $1 })
    let leadedIndex = leaded["layer"] as? Int ?? -1
    guard let withBoth = document.doc, let leadedAnchor = withBoth.describedAnchor(leadedIndex) else {
        throw ProbeFailure(description: "\(tag) no leading-30 layer: \(leaded)")
    }

    check("d") {
        try require(markedOutset != nil, "the render did not mark the layer: \(direct.meta)")
        let legacy = try legacyShaped(marked, index)
        document.doc = legacy
        try require(legacy.describedAnchor(index) == anchor && !legacy.describedRasterIsStale(index),
                    "the saved shape moved its anchor \(String(describing: legacy.describedAnchor(index))) "
                    + "from \(anchor), or reads stale")
        _ = try agentOK("edit_text_layer", document, ["layer": index, "text": "Zapfino"])
        let edited = textInkState(document.doc, index)
        try require(document.doc?.describedAnchor(index) == anchor,
                    "the edit moved the anchor: \(String(describing: document.doc?.describedAnchor(index)))")
        try require(edited.meta == direct.meta && edited.rect == direct.rect
                        && edited.pixels == direct.pixels,
                    "the edit is not the direct render: \(edited.meta) \(edited.rect) / \(direct.meta) "
                    + "\(direct.rect)")
        document.doc = legacy
        for turn in [30, -30] {
            _ = try agentOK("transform_layer", document, ["layer": index, "rotate": turn,
                                                          "pivot_x": anchor.x, "pivot_y": anchor.y])
        }
        let turned = document.doc?.describedAnchor(index) ?? .zero
        try require(abs(turned.x - anchor.x) < 1e-6 && abs(turned.y - anchor.y) < 1e-6
                        && document.doc?.layerMeta(index)?.contains("\"ink_outset\"") == true,
                    "30° and back: anchor \(turned) from \(anchor), meta "
                    + "\(document.doc?.layerMeta(index) ?? "-")")
        // Replace Missing Fonts on a saved layer whose y recentres (a fixed leading) with the
        // family it was made in keeps its anchor and marks it.
        let missing = try legacyShaped(withBoth, leadedIndex) { $0.font = "Zzzzzzz" }
        guard let replaced = try missing.replacingMissingFonts(["Zzzzzzz": "Zapfino"]) else {
            throw ProbeFailure(description: "Replace Missing Fonts replaced nothing")
        }
        try require(replaced.describedAnchor(leadedIndex) == leadedAnchor
                        && replaced.textPayload(leadedIndex)?.inkOutset != nil,
                    "Replace Missing Fonts: anchor "
                    + "\(String(describing: replaced.describedAnchor(leadedIndex))) from \(leadedAnchor), "
                    + "outset \(String(describing: replaced.textPayload(leadedIndex)?.inkOutset))")
    }

    // (d-marked) Replace Missing Fonts on a MARKED leading-30 layer recentres the rect its render
    // publishes, refitted to the new family's ink, not the rect its stored outset grows: a family
    // whose ink fits lands the new raster's y-centre on the saved raster's.
    check("d-marked") {
        guard let meta = withBoth.layerMeta(leadedIndex), var missing = TextLayerPayload.decode(meta),
              missing.inkOutset != nil, let saved = withBoth.layerInfo(leadedIndex) else {
            throw ProbeFailure(description: "the leading-30 layer is not marked")
        }
        missing.font = "Zzzzzzz"
        guard let json = missing.json(), let shaped = withBoth.withLayerMeta(leadedIndex, json),
              let replaced = try shaped.replacingMissingFonts(["Zzzzzzz": "Helvetica Neue"]),
              let info = replaced.layerInfo(leadedIndex) else {
            throw ProbeFailure(description: "Replace Missing Fonts replaced nothing")
        }
        let centre = { (y: Int, height: Int) in Double(y) + Double(height) / 2 }
        let moved = centre(info.offsetY, info.height) - centre(saved.offsetY, saved.height)
        try require(abs(moved) <= 1 && replaced.textPayload(leadedIndex)?.font == "Helvetica Neue",
                    "Replace Missing Fonts on a marked layer moved its y-centre by \(moved): "
                    + "y \(info.offsetY) height \(info.height) from y \(saved.offsetY) height "
                    + "\(saved.height), outset "
                    + "\(String(describing: replaced.textPayload(leadedIndex)?.inkOutset))")
    }

    check("d-lock") {
        let legacy = try legacyShaped(marked, index)
        let saved = textInkState(legacy, index)
        guard let locked = try legacy.withLockFlags(index, .transparency) else {
            throw ProbeFailure(description: "no lock")
        }
        document.doc = locked
        _ = try agentOK("edit_text_layer", document, ["layer": index, "rgba": [1, 0, 0, 1]])
        let recoloured = textInkState(document.doc, index)
        try require(recoloured.rect == saved.rect && document.doc?.describedAnchor(index) == anchor
                        && !recoloured.meta.contains("ink_outset") && inks(recoloured.pixels, (255, 0, 0)),
                    "a locked colour edit: \(recoloured.rect) from \(saved.rect), \(recoloured.meta)")
        // A re-layout is refused as before: the core's lock refusal.
        let resized = try agentCall("edit_text_layer", document, ["layer": index, "size": 60])
        try require(resized.isError && resized.text.contains("refused by layer locks")
                        && textInkState(document.doc, index).rect == saved.rect,
                    "a locked resize: \(resized.text)")
        _ = try agentOK("set_layer_lock", document, ["layer": index, "locks": [String]()])
        _ = try agentOK("edit_text_layer", document, ["layer": index, "rgba": [0, 0, 1, 1]])
        try require(document.doc?.layerMeta(index)?.contains("\"ink_outset\"") == true
                        && document.doc?.describedAnchor(index) == anchor,
                    "unlocked, a colour edit marks the layer at its anchor: "
                    + "\(document.doc?.layerMeta(index) ?? "-")")
        // A marked layer under the same lock is refitted to the same rect.
        guard let lockedMarked = try marked.withLockFlags(index, .transparency) else {
            throw ProbeFailure(description: "no lock on the marked layer")
        }
        document.doc = lockedMarked
        _ = try agentOK("edit_text_layer", document, ["layer": index, "rgba": [1, 0, 0, 1]])
        let refitted = textInkState(document.doc, index)
        try require(refitted.rect == direct.rect
                        && TextLayerPayload.decode(refitted.meta)?.inkOutset == markedOutset,
                    "a locked marked layer's colour edit: \(refitted.rect) from \(direct.rect), "
                    + "\(refitted.meta)")
    }

    // (e) Reopening the marked layer in the text session and committing unchanged adds no step.
    check("e") {
        document.doc = marked
        document.undoManager?.removeAllActions()
        let before = document.doc
        controller.selectTool(.text)
        controller.editTextLayer(index)
        let canvas: ImageCanvasView = controller.canvas
        guard canvas.activeTextView != nil else {
            throw ProbeFailure(description: "the marked layer did not reopen")
        }
        try require(canvas.commitTextSession(), "the unchanged commit failed: \(uiAlerts())")
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
        try require(document.undoManager?.canUndo == false && document.doc === before,
                    "the unchanged commit registered \(document.undoManager?.undoActionName ?? "-")")
    }

    try require(failures.isEmpty, "\(tag) \(failures.joined(separator: "; "))")
    print("PASS: \(tag) (a) the swash uncut in nine layouts, (a-cmyk) and in CMYK 8, (b) the codec, "
          + "(c) the marked rect, (d) a saved layer marked at its anchor, (d-marked) a marked layer's "
          + "font replaced at its centre, (d-lock) the lock deferring it, (e) no phantom step")
}
