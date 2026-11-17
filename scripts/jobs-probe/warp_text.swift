import AppKit

// "warp text" (designs/warp-text.md, EditorViewController+WarpText.swift): Layer ▸ Warp Text…
// run off screen through the editor's own action and the sheet's own controls. (a) Its
// validation in every document kind (each mode's closed `selectors` set, the float list,
// `sixteenBitActions`), off on a path; (b) RGB 8: a slider commit sets `canvas.previewImage`,
// Cancel clears it with no change and no step, OK records one `edit_text_layer` with the chosen
// four keys as one "Warp Text" step that undoes and replays, an unchanged OK records nothing,
// None records `warp: null`, a text-session commit keeps the warp, an unchanged OK keeps a
// warp set off the sliders' grid, and None removes a warp that rounds to 0; (c) the payload
// codec: versions 1–7 keep their bytes, an 8 round-trips, malformed 8s are refused, and a
// version-6 subpath of one anchor is refused; (d) RGB 16: a commit while other background work
// runs clears the preview and shows `busyPreviewNotice`, and an idle one previews again; (e) RGB 8
// with its Transparency locked: a commit names the lock in the notice, and (f) one while busy
// keeps its own words. Each section reports its own failure.

/// An open arch across a 64 × 48 fixture.
private let warpTextArch: [[String: Any]] = [["closed": false, "anchors": [
    ["x": 6, "y": 40, "out": [18, 4]], ["x": 58, "y": 40, "in": [46, 4]],
]]]

/// The pixels, placement and description of `layer`.
private func warpLayerState(_ document: ImageDocument, _ layer: Int) -> [String] {
    let info = document.doc?.layerInfo(layer)
    let pixels = document.doc?.layerPixels(layer) ?? []
    return [document.doc?.layerMeta(layer) ?? "-",
            "\(info.map { [$0.offsetX, $0.offsetY, $0.width, $0.height] } ?? [])",
            "\(pixels.count):\(pixels.reduce(0) { ($0 &* 31) &+ Int($1) })"]
}

/// Opens the sheet through the editor's own action and waits for it on screen.
private func warpSheet(_ controller: EditorViewController) throws -> WarpTextSheetController {
    do {
        return try pathSheet(controller, WarpTextSheetController.self) { controller.showWarpText(nil) }
    } catch {
        throw ProbeFailure(description: "the sheet did not open (\(error)): \(uiAlerts())")
    }
}

/// Sets the sheet's controls, each committing as a release does: the popup at `style` (0 None),
/// then each slider given.
private func warpSet(_ sheet: WarpTextSheetController, style: Int, sliders: [String: Double]) throws {
    let popup = try uiControl(sheet.view, id: "warpText.style", as: NSPopUpButton.self)
    popup.selectItem(at: style)
    try uiSend(popup, "style")
    for key in ["bend", "horizontal", "vertical"] {
        guard let value = sliders[key] else { continue }
        let slider = try uiControl(sheet.view, id: "warpText.\(key)", as: NSSlider.self)
        slider.doubleValue = value
        try uiSend(slider, key)
    }
}

/// Presses OK or Cancel and waits for the sheet to go and any worker to finish.
private func warpPress(_ controller: EditorViewController, _ sheet: WarpTextSheetController,
                       _ id: String) throws {
    try uiSend(try uiControl(sheet.view, id: id, as: NSButton.self), id)
    do {
        try pumpUntil({ controller.view.window?.attachedSheet == nil && !BackgroundWork.occupied },
                      seconds: 30)
    } catch {
        throw ProbeFailure(description: "\(id) left the sheet up (\(error)): \(uiAlerts())")
    }
}

func verifyWarpText() throws {
    let tag = "[warp text]"
    var failures: [String] = []
    func check(_ section: String, _ body: () throws -> Void) {
        do { try body() } catch { failures.append("(\(section)) \(error)") }
    }

    // (a) Every document kind: with a text layer active, Warp Text is on wherever a text layer
    // can be made and off in Indexed and Bitmap, and off once the layer is on a path.
    check("a") {
        var wrong: [String] = []
        for row in colourModeRows() {
            let (document, controller, _) = try openColourModeRow(row, tag: tag)
            defer { document.close() }
            let refusedHere = ["Indexed", "Bitmap"].contains(row.name)
            let made = try agentCall("add_text_layer", document,
                                     ["text": "Arc", "x": 4, "y": 4, "size": 12, "wrap_width": 0])
            let on = uiValidates(controller, "showWarpText:")
            if refusedHere {
                if on { wrong.append("\(row.name): Warp Text is on") }
                continue
            }
            if made.isError { wrong.append("\(row.name): add_text_layer refused: \(made.text)") }
            if !on { wrong.append("\(row.name): Warp Text is off on a text layer") }
            let placed = try agentCall("edit_text_layer", document, [
                "layer": made.result["layer"] ?? 0, "on_path": ["subpaths": warpTextArch]])
            if placed.isError {
                wrong.append("\(row.name): could not place the layer: \(placed.text)")
            } else if uiValidates(controller, "showWarpText:") {
                wrong.append("\(row.name): Warp Text is on for a layer on a path")
            }
        }
        try require(wrong.isEmpty, wrong.joined(separator: "; "))
    }

    // (b) The sheet (RGB 8).
    check("b") {
        let document = try fixture(width: 160, height: 80)
        defer {
            if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
            document.close()
        }
        let controller = try editor(document)
        let made = try agentOK("add_text_layer", document,
                               ["text": "Warp me", "x": 10, "y": 10, "size": 24, "wrap_width": 0])
        guard let index = made["layer"] as? Int else {
            throw ProbeFailure(description: "no text layer: \(made)")
        }
        document.undoManager?.removeAllActions()
        let plain = document.doc

        // Cancel: the slider's commit previews; Cancel clears it and changes nothing.
        let cancelled = try warpSheet(controller)
        try warpSet(cancelled, style: 1, sliders: ["bend": 50])
        try require(controller.canvas.previewImage != nil,
                    "a slider commit sets the preview")
        let none = try uiRecorded { try warpPress(controller, cancelled, "warpText.cancel") }
        try require(controller.canvas.previewImage == nil, "Cancel clears the preview")
        try require(document.doc === plain && document.undoManager?.canUndo != true
                    && none.isEmpty, "Cancel changes nothing, adds no step and records nothing")

        // OK: one recorded step with the chosen four keys, one undo, a replay.
        let chosen = TextWarp(style: "arch", bend: 0.5, horizontal: -0.2, vertical: 0.3)
        let sheet = try warpSheet(controller)
        try warpSet(sheet, style: 1, sliders: ["bend": 50, "horizontal": -20, "vertical": 30])
        let committing = try uiRecorded { try warpPress(controller, sheet, "warpText.ok") }
        try require(controller.canvas.previewImage == nil, "OK clears the preview")
        try require(document.doc?.textPayload(index)?.warp == chosen,
                    "OK warps the layer: \(String(describing: document.doc?.textPayload(index)?.warp)) "
                    + uiAlerts())
        let recorded = committing.first?.arguments["warp"] as? [String: Any]
        try require(committing.count == 1 && committing[0].tool == "edit_text_layer"
                    && recorded?["style"] as? String == "arch"
                    && recorded?["bend"] as? Double == 0.5
                    && recorded?["horizontal"] as? Double == -0.2
                    && recorded?["vertical"] as? Double == 0.3 && recorded?.count == 4,
                    "OK records one edit_text_layer with the four keys: "
                    + "\(committing.map { "\($0.tool) \($0.arguments)" })")
        try require(document.undoManager?.undoActionName == "Warp Text",
                    "one Warp Text step: \(document.undoManager?.undoActionName ?? "-")")
        let warpedState = warpLayerState(document, index)
        document.undoManager?.undo()
        try require(document.doc === plain, "one undo takes the warp off")
        let replay = ActionPlayer.runSingle(committing[0], on: document, undoName: "Replay")
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
        try require(replay.ok && warpLayerState(document, index) == warpedState,
                    "the recorded step replays to the same layer: \(replay.failures)")

        // OK with the stored warp unchanged records nothing.
        let warped = document.doc
        let again = try warpSheet(controller)
        let unchanged = try uiRecorded { try warpPress(controller, again, "warpText.ok") }
        try require(unchanged.isEmpty && document.doc === warped,
                    "an unchanged OK records nothing: \(unchanged.map { "\($0.arguments)" })")

        // None on a warped layer records warp: null.
        let off = try warpSheet(controller)
        try warpSet(off, style: 0, sliders: [:])
        let removing = try uiRecorded { try warpPress(controller, off, "warpText.ok") }
        try require(removing.count == 1 && removing[0].arguments["warp"] is NSNull
                    && document.doc?.textPayload(index)?.warp == nil,
                    "None records warp null: \(removing.map { "\($0.arguments)" })")
        document.undoManager?.undo()
        try require(document.doc === warped, "one undo puts the warp back")

        // A text-session commit keeps the warp (WT-7).
        controller.selectTool(.text)
        controller.editTextLayer(index)
        let canvas: ImageCanvasView = controller.canvas
        guard let view = canvas.activeTextView else {
            throw ProbeFailure(description: "the warped layer did not reopen")
        }
        view.setSelectedRange(NSRange(location: (view.string as NSString).length, length: 0))
        view.insertText("s", replacementRange: NSRange(location: NSNotFound, length: 0))
        try require(canvas.commitTextSession(), "the edit committed: \(uiAlerts())")
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
        let edited = document.doc?.textPayload(index)
        try require(edited?.string == "Warp mes" && edited?.warp == chosen,
                    "the session commit keeps the warp: \(edited?.string ?? "-") "
                    + "\(String(describing: edited?.warp))")

        // A warp set off the sliders' whole-hundredth grid (bend 0.125 shows as 13): an
        // unchanged OK keeps it and records nothing.
        _ = try agentOK("edit_text_layer", document, ["layer": index, "warp": ["bend": 0.125]])
        let offGrid = document.doc
        let offGridSheet = try warpSheet(controller)
        let untouched = try uiRecorded { try warpPress(controller, offGridSheet, "warpText.ok") }
        try require(untouched.isEmpty && document.doc === offGrid
                    && document.doc?.textPayload(index)?.warp?.bend == 0.125,
                    "an unchanged OK keeps an off-grid warp and records nothing: "
                    + "\(untouched.map { "\($0.arguments)" })")

        // A warp whose numbers all round to 0 (bend 0.004 shows as Arch 0/0/0): None still
        // removes it.
        _ = try agentOK("edit_text_layer", document, ["layer": index, "warp": [
            "style": "arch", "bend": 0.004, "horizontal": 0, "vertical": 0]])
        let tiny = try warpSheet(controller)
        try warpSet(tiny, style: 0, sliders: [:])
        let cleared = try uiRecorded { try warpPress(controller, tiny, "warpText.ok") }
        try require(cleared.count == 1 && cleared[0].arguments["warp"] is NSNull
                    && document.doc?.textPayload(index)?.warp == nil,
                    "None removes a warp that rounds to 0: \(cleared.map { "\($0.arguments)" }) "
                    + "\(String(describing: document.doc?.textPayload(index)?.warp))")
    }

    // (d) RGB 16: a commit while other background work runs clears the preview and says so
    // (`busyPreviewNotice`); once it ends, a commit previews again and empties the notice.
    check("d") {
        let document = try fixture(width: 160, height: 80)
        defer { document.close() }
        let controller = try editor(document)
        _ = try agentOK("convert_precision", document, ["mode": "rgba16", "structure": "preserve"])
        _ = try agentOK("add_text_layer", document,
                        ["text": "Warp me", "x": 10, "y": 10, "size": 24, "wrap_width": 0])
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
        let sheet = try warpSheet(controller)
        try warpSet(sheet, style: 1, sliders: ["bend": 50])
        try require(controller.canvas.previewImage != nil, "a 16-bit commit sets the preview")
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
        do {
            BackgroundWork.occupied = true
            defer { BackgroundWork.occupied = false }
            try warpSet(sheet, style: 1, sliders: ["bend": 20])
        }
        let stale = controller.canvas.previewImage != nil
        let notice = try uiControl(sheet.view, id: "warpText.notice", as: NSTextField.self)
        try require(!stale && notice.stringValue == WarpTextSheetController.busyPreviewNotice,
                    "a busy commit clears the preview and says so: preview kept \(stale), notice "
                    + "\"\(notice.stringValue)\"")
        try warpSet(sheet, style: 1, sliders: ["bend": 30])
        try require(controller.canvas.previewImage != nil && notice.stringValue.isEmpty,
                    "an idle commit previews again: notice \"\(notice.stringValue)\"")
        try warpPress(controller, sheet, "warpText.cancel")
    }

    /// An RGB 8 "Warp me" layer with its Transparency locked, its sheet open: the document, the
    /// controller, the sheet and the layer's name.
    func lockedSheet() throws -> (ImageDocument, EditorViewController, WarpTextSheetController,
                                  String) {
        let document = try fixture(width: 160, height: 80)
        do {
            let controller = try editor(document)
            let made = try agentOK("add_text_layer", document,
                                   ["text": "Warp me", "x": 10, "y": 10, "size": 24, "wrap_width": 0])
            guard let index = made["layer"] as? Int, let name = document.doc?.layerInfo(index)?.name
            else { throw ProbeFailure(description: "no text layer: \(made)") }
            _ = try agentOK("set_layer_lock", document, ["layer": index, "locks": ["transparency"]])
            return (document, controller, try warpSheet(controller), name)
        } catch {
            document.close()
            throw error
        }
    }

    // (e) RGB 8, Transparency locked: a commit the lock refuses names the lock in the notice.
    check("e") {
        let (document, controller, sheet, name) = try lockedSheet()
        defer { document.close() }
        let lockNotice = "“\(name)” has its transparency locked, and a warp lays the layer out "
            + "again. Unlock Transparency in Layer ▸ Lock to warp it."
        try warpSet(sheet, style: 1, sliders: ["bend": 50])
        let notice = try uiControl(sheet.view, id: "warpText.notice", as: NSTextField.self)
        try require(controller.canvas.previewImage == nil && notice.stringValue == lockNotice,
                    "a locked commit names the lock: notice \"\(notice.stringValue)\"")
        try warpPress(controller, sheet, "warpText.cancel")
    }

    // (f) The same locked layer with other background work running: the failure keeps its own
    // words, never the lock's.
    check("f") {
        let (document, controller, sheet, _) = try lockedSheet()
        defer { document.close() }
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
        do {
            BackgroundWork.occupied = true
            defer { BackgroundWork.occupied = false }
            try warpSet(sheet, style: 1, sliders: ["bend": 30])
        }
        let notice = try uiControl(sheet.view, id: "warpText.notice", as: NSTextField.self)
        try require(controller.canvas.previewImage == nil && !notice.stringValue.isEmpty
                        && !notice.stringValue.contains("transparency locked"),
                    "a locked commit while busy keeps its own words: \"\(notice.stringValue)\"")
        try warpPress(controller, sheet, "warpText.cancel")
    }

    // (c) The codec.
    let artwork = "\"artwork\":{\"evaluation\":\"outline_area_v1\","
        + "\"intrinsicGlyphs\":\"platform_byte_assets_v1\",\"rgba\":[0,0,0,1],\"runs\":[]},"
    let tail = "\"italic\":false,\"leading\":0,\"origin_frac\":[0,0],\"size\":24,\"strikethrough\":false,"
        + "\"string\":\"Hi\",\"tracking\":0,\"transform\":[1,0,0,1],\"type\":\"text\",\"underline\":false,"
    let v3 = "{\"alignment\":\"left\"," + artwork + "\"baseline_shift\":0,\"box_width\":0,"
        + "\"color\":\"#000000FF\",\"font\":\"Helvetica\"," + tail + "\"version\":3,\"weight\":5}"
    let path = { (anchors: String) in
        "\"on_path\":{\"offset\":0,\"outline\":{\"fill_rule\":\"nonzero\",\"subpaths\":"
            + "[{\"anchors\":[" + anchors + "],\"closed\":false}]},\"side\":\"left\"},"
    }
    let twoAnchors = path("[0,0,0,0,0,0,0],[100,0,100,0,100,0,0]")
    let v6 = v3.replacingOccurrences(of: "\"leading\":0,", with: "\"leading\":0," + twoAnchors)
        .replacingOccurrences(of: "\"version\":3", with: "\"version\":6")
    let warp = "\"warp\":{\"bend\":0.5,\"horizontal\":0,\"style\":\"arch\",\"vertical\":0}"
    let v8 = v3.replacingOccurrences(of: "\"version\":3,", with: "\"version\":8," + warp + ",")
    let v8Outset = v8.replacingOccurrences(
        of: "\"font\":\"Helvetica\",", with: "\"font\":\"Helvetica\",\"ink_outset\":[1,2,3,4],")
    check("c") {
        var wrong: [String] = []
        // A subpath of one anchor has no length: a version 6 holding one is not a text layer.
        let oneAnchor = v6.replacingOccurrences(of: twoAnchors, with: path("[0,0,0,0,0,0,0]"))
        if TextLayerPayload.decode(oneAnchor) != nil { wrong.append("a one-anchor subpath decoded") }
        if TextLayerPayload.decode(v6) == nil { wrong.append("a two-anchor subpath was refused") }
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
            ("6", v6),
            ("7", v3.replacingOccurrences(of: "\"font\":\"Helvetica\",",
                                          with: "\"font\":\"Helvetica\",\"ink_outset\":[1,2,3,4],")
                .replacingOccurrences(of: "\"version\":3", with: "\"version\":7")),
            ("8", v8),
            ("8 with ink_outset", v8Outset),
        ]
        for (version, literal) in kept {
            let decoded = TextLayerPayload.decode(literal)
            if decoded == nil || decoded?.json() != literal {
                wrong.append("version \(version) \(decoded?.json() ?? "refused")")
            }
        }
        if TextLayerPayload.decode(v8)?.warp != TextWarp(style: "arch", bend: 0.5, horizontal: 0,
                                                         vertical: 0) {
            wrong.append("version 8 does not read its warp")
        }
        let refused: [(String, String)] = [
            ("8 without warp", v3.replacingOccurrences(of: "\"version\":3", with: "\"version\":8")),
            // A valid version 7 (its ink_outset) that also carries a warp.
            ("7 with warp", v8Outset.replacingOccurrences(of: "\"version\":8", with: "\"version\":7")),
            ("3 with warp", v8.replacingOccurrences(of: "\"version\":8", with: "\"version\":3")),
            ("all zero", v8.replacingOccurrences(of: "\"bend\":0.5", with: "\"bend\":0")),
            ("an unknown style", v8.replacingOccurrences(of: "\"arch\"", with: "\"twist\"")),
            ("a number past 1", v8.replacingOccurrences(of: "\"bend\":0.5", with: "\"bend\":1.5")),
            ("a missing key", v8.replacingOccurrences(of: ",\"vertical\":0", with: "")),
            ("with on_path", v8.replacingOccurrences(of: "\"leading\":0,",
                                                     with: "\"leading\":0," + twoAnchors)),
            ("8 without box_width", v8.replacingOccurrences(of: "\"box_width\":0,", with: "")),
            ("8 without artwork", v8.replacingOccurrences(of: artwork, with: "")),
        ]
        for (name, literal) in refused where TextLayerPayload.decode(literal) != nil {
            wrong.append("\(name) decoded")
        }
        try require(wrong.isEmpty, wrong.joined(separator: "; "))
    }

    try require(failures.isEmpty, "\(tag) \(failures.joined(separator: "; "))")
    print("PASS: \(tag) (a) on in every text-holding kind and off on a path, (b) the sheet's "
          + "preview, Cancel, one recorded step that undoes and replays, an unchanged OK, None and "
          + "a session commit keeping the warp, (c) the codec, (d) a busy 16-bit preview cleared "
          + "and named, (e) a locked preview naming the lock, (f) and a busy one its own words")
}
