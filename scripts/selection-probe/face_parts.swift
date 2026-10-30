import AppKit

/// Select ▸ Face Parts… and `inspect_faces` / `select_face_parts`
/// on the licensed portrait. Every check is a PROPERTY of the masks — areas
/// against the face's box, containment, order down the face — never mask
/// bytes. The bands are declared here, before the case runs; never widened.
/// B is face 1's box area from `inspect_faces`. "Hard" means coverage at or
/// above one half (32768).
private let facePartsFaceBand = 0.7...2.0
private let facePartsSkinFloor = 0.5          // skin ≥ this × face, and < face
private let facePartsEyesBand = 0.004...0.06
private let facePartsBrowsBand = 0.004...0.10
private let facePartsLipsBand = 0.008...0.10
private let facePartsMouthCeiling = 0.06
private let facePartsEyesOutsideFace = 0.02   // hard eyes outside the hard face, of hard eyes
private let facePartsSkinOnEyes = 0.02        // hard skin ∩ hard eyes, of hard eyes
private let facePartsBrowsOnEyes = 0.05       // hard brows ∩ hard eyes, of hard eyes
private let facePartsFeatherArea = 0.9...1.1  // feather 4's area over the unfeathered one
private let facePartsDepthIoU = 0.9           // 16-bit parts and the float face against 8-bit, hard
private let facePartsFloatCentroid = 1.5      // px: a float eye, brow or lip centroid from 8-bit's
private let facePartsFloatArea = 0.15         // a float eye, brow or lip area change, of 8-bit's

private let facePartsNoFace = "No face was found in this image."

private func facePartsDocument(_ image: RasterImage) throws -> ImageDocument {
    guard let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "Face Parts document allocation")
    }
    document.undoManager?.groupsByEvent = false
    document.updateChangeCount(.changeCleared)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    document.windowControllers.first?.window?.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    document.windowControllers.first?.window?.orderFront(nil)
    document.undoManager?.removeAllActions()
    return document
}

/// The in-band refusal text of a tool call, or nil when it succeeded.
private func facePartsRefusal(_ document: ImageDocument, _ name: String,
                              _ values: [String: Any] = [:]) throws -> String? {
    var arguments = values
    arguments["document_id"] = AgentServer.shared.documentID(for: document)
    let data = try JSONSerialization.data(withJSONObject: arguments)
    let raw = AgentServer.shared.execute(tool: name, argumentsJSON: String(decoding: data, as: UTF8.self))
    guard let result = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Invalid tool result: \(raw)")
    }
    guard result["isError"] as? Bool == true else { return nil }
    let content = result["content"] as? [[String: Any]]
    return content?.first?["text"] as? String
}

/// The u16 selection `select_face_parts` leaves, then one Undo back to none.
private func facePartsMask(_ document: ImageDocument, _ values: [String: Any]) throws -> [UInt16] {
    _ = try call(document, "select_face_parts", values)
    guard let mask = document.canvasSelection?.maskSamples16() else {
        throw ProbeFailure(description: "select_face_parts \(values) left no selection")
    }
    document.undoManager?.undo()
    try require(document.canvasSelection == nil, "One Undo reverses select_face_parts \(values)")
    return mask
}

private func area(_ mask: [UInt16]) -> Double { mask.reduce(0.0) { $0 + Double($1) } / 65535 }
private func hard(_ v: UInt16) -> Bool { v >= 32768 }
private func hardCount(_ mask: [UInt16]) -> Int { mask.reduce(0) { $0 + (hard($1) ? 1 : 0) } }
private func hardBoth(_ a: [UInt16], _ b: [UInt16]) -> Int {
    zip(a, b).reduce(0) { $0 + (hard($1.0) && hard($1.1) ? 1 : 0) }
}
private func centroidY(_ mask: [UInt16], width: Int) -> Double {
    var sum = 0.0, weight = 0.0
    for (i, v) in mask.enumerated() where v > 0 {
        sum += Double(v) * Double(i / width)
        weight += Double(v)
    }
    return weight == 0 ? .nan : sum / weight
}
private func centroidX(_ mask: [UInt16], width: Int) -> Double {
    var sum = 0.0, weight = 0.0
    for (i, v) in mask.enumerated() where v > 0 {
        sum += Double(v) * Double(i % width)
        weight += Double(v)
    }
    return weight == 0 ? .nan : sum / weight
}
private func hardIoU(_ a: [UInt16], _ b: [UInt16]) -> Double {
    var both = 0, either = 0
    for (x, y) in zip(a, b) {
        if hard(x) && hard(y) { both += 1 }
        if hard(x) || hard(y) { either += 1 }
    }
    return either == 0 ? 0 : Double(both) / Double(either)
}

func verifyFaceParts() throws {
    guard let directory = ProcessInfo.processInfo.environment["DR_SELECTION_PHOTO_DIR"] else {
        throw ProbeFailure(description: "DR_SELECTION_PHOTO_DIR is not set; pass --photos-dir")
    }
    let photos = URL(fileURLWithPath: directory)
    let portrait = try RasterImage.open(url: photos.appendingPathComponent("portrait.png"))
    let width = portrait.width
    let document = try facePartsDocument(portrait)
    defer { document.close() }
    let native = document.doc
    AutomationAlerts.reset()

    // The face list.
    let inspection = try call(document, "inspect_faces")
    guard let faceCount = inspection["face_count"] as? Int, faceCount >= 1,
          let faces = inspection["faces"] as? [[String: Any]], faces.count == faceCount,
          let box = faces[0]["bounds"] as? [String: Any],
          let bx = box["x"] as? Int, let by = box["y"] as? Int,
          let bw = box["width"] as? Int, let bh = box["height"] as? Int, bw > 0, bh > 0 else {
        throw ProbeFailure(description: "inspect_faces answered no face on the portrait: \(inspection)")
    }
    try require(document.canvasSelection == nil && inspection["model"] as? String == "Vision face landmarks",
                "inspect_faces is read-only")
    let available = faces[0]["parts"] as? [String] ?? []
    try require(Set(available) == Set(FacePart.allCases.map(\.rawValue)),
                "The portrait's face has every part's landmarks: \(available)")
    let boxArea = Double(bw * bh)
    let faceCenter = [Double(bx) + Double(bw) / 2, Double(by) + Double(bh) / 2]

    // One mask a part.
    var masks: [FacePart: [UInt16]] = [:]
    for part in FacePart.allCases {
        masks[part] = try facePartsMask(document, ["parts": [part.rawValue], "faces": [1]])
    }
    guard let face = masks[.face], let skin = masks[.skin], let eyes = masks[.eyes],
          let irises = masks[.irises], let brows = masks[.eyebrows], let lips = masks[.lips],
          let mouth = masks[.mouth] else { throw ProbeFailure(description: "A part mask is missing") }
    let ratio = { (mask: [UInt16]) in area(mask) / boxArea }
    let report = FacePart.allCases.map { String(format: "%@ %.4f", $0.rawValue, ratio(masks[$0] ?? [])) }
        .joined(separator: ", ")
    try require(facePartsFaceBand.contains(ratio(face)), "face/B in \(facePartsFaceBand): \(report)")
    try require(area(skin) >= facePartsSkinFloor * area(face) && area(skin) < area(face),
                "skin is at least half the face and less than it: \(report)")
    try require(facePartsEyesBand.contains(ratio(eyes)), "eyes/B in \(facePartsEyesBand): \(report)")
    try require(area(irises) > 0 && zip(irises, eyes).allSatisfy { UInt32($0) <= UInt32($1) + 1 },
                "irises cover something and only inside the eyes: \(report)")
    try require(facePartsBrowsBand.contains(ratio(brows)), "eyebrows/B in \(facePartsBrowsBand): \(report)")
    try require(facePartsLipsBand.contains(ratio(lips)), "lips/B in \(facePartsLipsBand): \(report)")
    try require(ratio(mouth) <= facePartsMouthCeiling, "mouth/B ≤ \(facePartsMouthCeiling): \(report)")
    let hardEyes = Double(max(1, hardCount(eyes)))
    let eyesOutside = zip(eyes, face).reduce(0) { $0 + (hard($1.0) && !hard($1.1) ? 1 : 0) }
    try require(Double(eyesOutside) <= facePartsEyesOutsideFace * hardEyes,
                "the eyes lie inside the face: \(eyesOutside) of \(Int(hardEyes)) eye pixels outside")
    try require(Double(hardBoth(skin, eyes)) <= facePartsSkinOnEyes * hardEyes,
                "skin leaves the eyes out: \(hardBoth(skin, eyes)) of \(Int(hardEyes))")
    try require(Double(hardBoth(brows, eyes)) <= facePartsBrowsOnEyes * hardEyes,
                "the eyebrows stay off the eyes: \(hardBoth(brows, eyes)) of \(Int(hardEyes))")
    guard let lipsTop = lips.indices.first(where: { hard(lips[$0]) }).map({ $0 / width }) else {
        throw ProbeFailure(description: "The lips have no hard row")
    }
    let eyesY = centroidY(eyes, width: width), browsY = centroidY(brows, width: width)
    try require(eyesY < Double(lipsTop), "the eyes sit above the lips: \(eyesY) against row \(lipsTop)")
    try require(browsY < eyesY, "the eyebrows sit above the eyes: \(browsY) against \(eyesY)")
    try require(document.doc === native && !document.isDocumentEdited && AutomationAlerts.recorded.isEmpty,
                "Face Parts changes no pixel and shows no alert: \(AutomationAlerts.recorded)")

    // Feather, and the combine modes against a rectangle.
    let soft = try facePartsMask(document, ["parts": ["face"], "faces": [1], "feather": 4])
    let softCount = { (mask: [UInt16]) in mask.reduce(0) { $0 + ($1 > 0 && $1 < 65535 ? 1 : 0) } }
    try require(softCount(soft) > softCount(face) && facePartsFeatherArea.contains(area(soft) / area(face)),
                "feather 4 softens the edge and keeps the area: \(softCount(soft)) soft against "
                    + "\(softCount(face)), area ratio \(area(soft) / area(face))")
    let rect: [String: Any] = ["x": 0, "y": 0, "width": Int(faceCenter[0]), "height": portrait.height]
    _ = try call(document, "select_rect", rect)
    guard let r = document.canvasSelection?.maskSamples16() else {
        throw ProbeFailure(description: "select_rect left no selection")
    }
    var combined: [String: [UInt16]] = [:]
    for mode in ["add", "subtract", "intersect"] {
        _ = try call(document, "select_face_parts", ["parts": ["face"], "faces": [1], "mode": mode])
        combined[mode] = document.canvasSelection?.maskSamples16() ?? [UInt16](repeating: 0, count: r.count)
        document.undoManager?.undo()
        try require(document.canvasSelection?.maskSamples16() == r, "One Undo restores the rectangle after \(mode)")
    }
    document.undoManager?.undo()
    try require(document.canvasSelection == nil, "One Undo removes the rectangle")
    let added = combined["add"] ?? [], cut = combined["subtract"] ?? [], met = combined["intersect"] ?? []
    try require(zip(added, zip(r, face)).allSatisfy { UInt32($0) + 1 >= UInt32(max($1.0, $1.1)) }
                && area(added) > area(r), "add holds the rectangle and the face")
    try require(zip(cut, zip(r, face)).allSatisfy { UInt32($0) <= UInt32(min($1.0, 65535 - $1.1)) + 1 }
                && area(cut) < area(r), "subtract removes the face from the rectangle")
    try require(zip(met, zip(r, face)).allSatisfy { UInt32($0) <= UInt32(min($1.0, $1.1)) + 1 }
                && area(met) > 0, "intersect keeps only the face inside the rectangle")

    // The sheet.
    let controller = try editor(document)
    let sheet = try FacePartsSheetController(editor: controller, document: document)
    try present(sheet, in: controller)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, message: "the Face Parts preview")
    guard let table = allViews(sheet.view).compactMap({ $0 as? NSTableView }).first,
          let apply = allViews(sheet.view).compactMap({ $0 as? NSButton }).first(where: { $0.title == "Apply" }) else {
        throw ProbeFailure(description: "Face Parts table/Apply missing")
    }
    try require(table.numberOfRows == faceCount && sheet.selectedFaces == Array(1...faceCount)
                && sheet.selectedParts == [.face], "The sheet lists every face, all chosen, with Face checked")
    let center = CGPoint(x: faceCenter[0], y: faceCenter[1])
    sheet.toggleFace(at: center)
    try require(!sheet.selectedFaces.contains(1), "A click on face 1 unchooses it")
    sheet.toggleFace(at: center)
    try require(sheet.selectedFaces.contains(1), "A second click chooses it again")
    sheet.setPart(.face, on: false)
    sheet.setPart(.eyes, on: true)
    try require(sheet.selectedParts == [.eyes], "The part checkboxes choose Eyes alone")
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, message: "the Eyes preview")
    try require(apply.isEnabled, "Apply is enabled with a face and a part")
    ActionRecorder.shared.start()
    apply.performClick(nil)
    let recorded = ActionRecorder.shared.stop()
    try require(document.canvasSelection?.maskSamples16() == eyes && document.doc === native
                && !document.isDocumentEdited && AutomationAlerts.recorded.isEmpty,
                "The sheet's Apply selects the eyes, as select_face_parts does, with no alert")
    try require(recorded.count == 1 && recorded.first?.tool == "select_face_parts"
                && recorded.first?.arguments["parts"] as? [String] == ["eyes"]
                && (faceCount > 1 || recorded.first?.arguments["faces"] == nil)
                && recorded.first?.note == "Select ▸ Face Parts…",
                "Apply records one select_face_parts step: \(recorded.map { ($0.tool, $0.arguments, $0.note ?? "") })")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil && document.undoManager?.canUndo != true,
                "The sheet's Apply is one selection Undo")

    // No face: an exact refusal, in-band, from both tools, selection unchanged.
    for name in ["fur.png", "product.jpg"] {
        let photo = try facePartsDocument(try RasterImage.open(url: photos.appendingPathComponent(name)))
        defer { photo.close() }
        let inspected = try facePartsRefusal(photo, "inspect_faces")
        let selected = try facePartsRefusal(photo, "select_face_parts", ["parts": ["face"]])
        try require(inspected == facePartsNoFace && selected == facePartsNoFace && photo.canvasSelection == nil,
                    "\(name) refuses with the no-face sentence (got \(inspected ?? "nil"), \(selected ?? "nil"))")
    }

    // Two copies side by side: numbered left to right, each addressable.
    guard let white = RasterImage.from(rgba: [UInt8](repeating: 255, count: 1280 * 600 * 4),
                                      width: 1280, height: 600),
          let joined = try RasterDocument.from(image: white)?
            .addingImageLayer(above: 0, portrait, name: "Left person")?
            .withLayerOffset(1, 20, 20)?
            .addingImageLayer(above: 1, portrait, name: "Right person")?
            .withLayerOffset(2, 700, 20),
          let joinedImage = joined.flattened() else {
        throw ProbeFailure(description: "Two-portrait fixture")
    }
    let pair = try facePartsDocument(joinedImage)
    defer { pair.close() }
    let pairInspection = try call(pair, "inspect_faces")
    guard pairInspection["face_count"] as? Int == 2, let pairFaces = pairInspection["faces"] as? [[String: Any]],
          let left = pairFaces[0]["bounds"] as? [String: Any], let right = pairFaces[1]["bounds"] as? [String: Any],
          let lx = left["x"] as? Int, let rx = right["x"] as? Int, let ry = right["y"] as? Int,
          let rw = right["width"] as? Int, let rh = right["height"] as? Int else {
        throw ProbeFailure(description: "The two portraits are not two faces: \(pairInspection)")
    }
    try require(lx < 640 && rx >= 640, "Faces are numbered left to right: \(lx), \(rx)")
    let leftHalf = { (mask: [UInt16]) in
        (0..<600).reduce(0.0) { sum, y in sum + area(Array(mask[(y * 1280)..<(y * 1280 + 640)])) }
    }
    let rightOnly = try facePartsMask(pair, ["parts": ["face"], "faces": [2]])
    let byPoint = try facePartsMask(pair, ["parts": ["face"],
                                           "point": [Double(rx) + Double(rw) / 2, Double(ry) + Double(rh) / 2]])
    let bothFaces = try facePartsMask(pair, ["parts": ["face"]])
    try require(leftHalf(rightOnly) == 0 && area(rightOnly) > 0, "faces [2] covers only the right copy")
    try require(byPoint == rightOnly, "A point in face 2 selects face 2")
    try require(leftHalf(bothFaces) > 0 && area(bothFaces) - leftHalf(bothFaces) > 0,
                "No faces argument covers both copies")

    // 16-bit and 32-bit float agree with 8-bit on every face (Ruling 1). 16-bit hands Vision
    // the same bytes, so each part agrees by hard IoU. Float hands Vision the tone-mapped
    // display, whose landmarks can sit a fraction of a pixel off the 8-bit ones: the face is
    // judged by hard IoU, and the eyes, brows and lips by geometry, since half a pixel of
    // shift costs a part about 12 px across a fifth of its overlap.
    let deep = try facePartsDocument(portrait)
    defer { deep.close() }
    _ = try call(deep, "convert_precision", ["mode": "rgba16", "structure": "preserve"])
    let item = NSMenuItem(title: "Face Parts", action: NSSelectorFromString("showFaceParts:"), keyEquivalent: "")
    let deepEditor = try editor(deep)
    try require(deepEditor.validatePrecisionItem(item) == nil, "showFaceParts: passes the 16-bit gate")
    let folder = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"]
        ?? NSTemporaryDirectory())
    let url = folder.appendingPathComponent("face-parts-float-portrait.tiff")
    try writeFloatPortrait(portrait, to: url)
    let float = try floatPortraitDocument(url)
    defer { float.close() }
    let floatEditor = try editor(float)
    try require(floatEditor.validatePrecisionItem(item) == nil, "showFaceParts: passes the float gate")
    let describe = { (mask: [UInt16]) in
        String(format: "area %.1f centroid (%.2f, %.2f)", area(mask), centroidX(mask, width: width),
               centroidY(mask, width: width))
    }
    var deepWorst = 1.0, floatFaceWorst = 1.0, worstShift = 0.0, worstAreaChange = 0.0
    for index in 1...faceCount {
        for part in [FacePart.face, .eyes, .eyebrows, .lips] {
            let arguments: [String: Any] = ["parts": [part.rawValue], "faces": [index]]
            let byte = try facePartsMask(document, arguments)
            let deepMask = try facePartsMask(deep, arguments)
            let floatMask = try facePartsMask(float, arguments)
            let deepIoU = hardIoU(deepMask, byte)
            deepWorst = min(deepWorst, deepIoU)
            try require(deepIoU >= facePartsDepthIoU,
                        "16-bit \(part.rawValue) of face \(index) agrees with 8-bit: IoU \(deepIoU)")
            if part == .face {
                let floatIoU = hardIoU(floatMask, byte)
                floatFaceWorst = min(floatFaceWorst, floatIoU)
                try require(floatIoU >= facePartsDepthIoU,
                            "float face \(index) agrees with 8-bit: IoU \(floatIoU); 8-bit \(describe(byte)), "
                                + "float \(describe(floatMask))")
            } else {
                let shift = hypot(centroidX(floatMask, width: width) - centroidX(byte, width: width),
                                  centroidY(floatMask, width: width) - centroidY(byte, width: width))
                let change = abs(area(floatMask) - area(byte)) / area(byte)
                worstShift = max(worstShift, shift)
                worstAreaChange = max(worstAreaChange, change)
                try require(shift <= facePartsFloatCentroid && change <= facePartsFloatArea,
                            "float \(part.rawValue) of face \(index) sits where 8-bit's does: centroid "
                                + "\(shift) px, area change \(change); 8-bit \(describe(byte)), "
                                + "float \(describe(floatMask))")
            }
        }
    }
    try require(AutomationAlerts.recorded.isEmpty, "No alert: \(AutomationAlerts.recorded)")
    print("PASS: Face Parts parts/B \(report)")
    print(String(format: "PASS: Face Parts no-face refusals, two faces left to right, feather, combine modes, "
                    + "the sheet, 16-bit worst IoU %.4f, float face worst IoU %.4f, float eyes/brows/lips "
                    + "worst centroid shift %.2f px and area change %.1f%%",
                 deepWorst, floatFaceWorst, worstShift, worstAreaChange * 100))
}
