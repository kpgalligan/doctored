import AppKit
import CoreVideo
import ImageIO
import Vision

/// Select ▸ Select People… and `model "people"` on the subject
/// tools, on the user's three group photos (read in place, never copied) and
/// on synthetic `PersonInstances.build` inputs. Every expectation is declared
/// here, before the case runs, in normalized upright canvas coordinates;
/// never widened. "Mask k" is `analysis.mask(subjects: [k])`; "hard" means at
/// least 128.
///
/// Each photo is checked in two parts (controller ruling 1, 2026-10-12):
/// 1. The oracle is Vision's raw output. `peopleOracle` runs Vision's own
///    person-instance request on the same file in the same run, with no app
///    code. The app's person count equals the oracle's instance count, and
///    person k matches the oracle's k-th instance from the left (see
///    `peopleOracleRecall` and `peopleOracleLeak`).
/// 2. Human-truth pins, only where Vision separates people. Vision merges
///    two of c482's four people into one instance, so c482 pins faces 1, 2
///    and 4 to three distinct people numbered left to right, and face 3 only
///    to the All People aggregate.

private struct PeoplePhoto {
    let name: String
    /// Exact count, or nil for "at least `minimumCount`".
    let count: Int?
    let minimumCount: Int
    /// Each face and the person number it must hit; a nil number means
    /// "distinct, increasing in this order".
    let faces: [(x: Double, y: Double, person: Int?)]
    /// Faces pinned only to the All People aggregate (hard there).
    let aggregateFaces: [(x: Double, y: Double)]
    /// Each mask's hard area is at least this share of the aggregate's.
    let minimumShare: Double?
    /// The mask x centroids increase with k, at least this share of W apart.
    let centroidStep: Double?
    let portrait: Bool
    let size: (width: Int, height: Int)?
}

private let peoplePhotos: [PeoplePhoto] = [
    PeoplePhoto(name: "c482d87d-45dd-4e96-b2f3-71ed170a6259.JPG", count: nil, minimumCount: 3,
                faces: [(0.319, 0.297, nil), (0.444, 0.328, nil), (0.688, 0.344, nil)],
                aggregateFaces: [(0.583, 0.328)],
                minimumShare: 0.08, centroidStep: 0.05, portrait: true, size: (1512, 2016)),
    PeoplePhoto(name: "IMG_9639.jpg", count: 2, minimumCount: 2,
                faces: [(0.313, 0.594, 1), (0.719, 0.320, 2)], aggregateFaces: [],
                minimumShare: 0.15, centroidStep: nil, portrait: true, size: nil),
    PeoplePhoto(name: "IMG_0221.jpg", count: nil, minimumCount: 3,
                faces: [(0.086, 0.271, nil), (0.363, 0.510, nil), (0.609, 0.250, nil)], aggregateFaces: [],
                minimumShare: nil, centroidStep: nil, portrait: false, size: nil),
]
/// Part 1's bounds, over the oracle's mask cells (each sampled at its centre
/// pixel) where the app's aggregate is hard, since the aggregate is the edge
/// model and only the split between people is under test. Of the oracle
/// instance's own core (`PeopleOracle`), person k is hard on at
/// least `peopleOracleRecall`; of the other instances' cores, on at most
/// `peopleOracleLeak`. A swapped number, a flipped or shifted map, or a lost
/// person misses by far more; a one-cell seam between neighbours does not.
private let peopleOracleRecall = 0.90
private let peopleOracleLeak = 0.05
/// A core with fewer qualifying cells than this is not a comparison.
private let peopleOracleMinimumCells = 20
/// The hard union of every mask, of the aggregate's hard area.
private let peopleUnionShare = 0.85
/// How far a mask's full-resolution hard box may pass its map box, of the canvas dimension.
private let peopleBoxGrowth = 0.02

private let peopleNoPeople = "No people were found in this image."
private let peopleBadModel = "model must be \"objects\" or \"people\"."

private func peopleDocument(_ image: RasterImage) throws -> ImageDocument {
    guard let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "People document allocation")
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
private func peopleRefusal(_ document: ImageDocument, _ name: String,
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

private func hardCount(_ mask: [UInt8]) -> Int { mask.reduce(0) { $0 + ($1 >= 128 ? 1 : 0) } }
private func softCount(_ mask: [UInt8]) -> Int { mask.reduce(0) { $0 + ($1 > 0 && $1 < 255 ? 1 : 0) } }
private func centroidX(_ mask: [UInt8], width: Int) -> Double {
    var sum = 0.0, weight = 0.0
    for (i, v) in mask.enumerated() where v > 0 {
        sum += Double(v) * Double(i % width)
        weight += Double(v)
    }
    return weight == 0 ? .nan : sum / weight
}
private func hardBox(_ mask: [UInt8], width: Int) -> CGRect {
    var minX = Int.max, minY = Int.max, maxX = -1, maxY = -1
    for (i, v) in mask.enumerated() where v >= 128 {
        let x = i % width, y = i / width
        minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y)
    }
    return maxX < 0 ? .null : CGRect(x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1)
}

/// Vision's raw answer for one photo, through the argmax step: each cell of
/// Vision's mask grid belongs to the nonzero instance whose raw coverage is
/// highest there, if that coverage is at least 0.5 (ties to the lower id);
/// an instance's core is its cells. Vision's raw instance masks overlap
/// where people touch, so without the argmax a cell can sit in two cores.
/// Instances are ordered left to right by the x centroid of their core, then
/// y, then the id. The same request, opening and rule as
/// `scripts/people-instances-oracle`, and no app code.
private struct PeopleOracle {
    let maskWidth: Int, maskHeight: Int
    let ids: [Int]
    let cores: [[Bool]]
}

private func peopleOracle(_ url: URL) throws -> PeopleOracle {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
              kCGImageSourceCreateThumbnailFromImageAlways: true,
              kCGImageSourceCreateThumbnailWithTransform: true,
              kCGImageSourceThumbnailMaxPixelSize: 100_000] as CFDictionary) else {
        throw ProbeFailure(description: "The oracle cannot open \(url.lastPathComponent)")
    }
    let request = VNGeneratePersonInstanceMaskRequest()
    try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
    var planes: [(id: Int, values: [Float])] = []
    var maskWidth = 0, maskHeight = 0
    if let observation = request.results?.first {
        for id in observation.allInstances.sorted() where id != 0 {
            let buffer = try observation.generateMask(forInstances: IndexSet(integer: id))
            guard CVPixelBufferGetPixelFormatType(buffer) == kCVPixelFormatType_OneComponent32Float else {
                throw ProbeFailure(description: "The oracle's instance \(id) mask is not float")
            }
            CVPixelBufferLockBaseAddress(buffer, .readOnly)
            defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
            let width = CVPixelBufferGetWidth(buffer), height = CVPixelBufferGetHeight(buffer)
            let stride = CVPixelBufferGetBytesPerRow(buffer)
            guard let base = CVPixelBufferGetBaseAddress(buffer),
                  planes.isEmpty || (width == maskWidth && height == maskHeight) else {
                throw ProbeFailure(description: "The oracle's instance \(id) mask is unreadable")
            }
            maskWidth = width
            maskHeight = height
            var values = [Float](repeating: 0, count: width * height)
            for y in 0..<height {
                let row = base.advanced(by: y * stride).assumingMemoryBound(to: Float.self)
                for x in 0..<width { values[y * width + x] = row[x] }
            }
            planes.append((id, values))
        }
    }
    let cells = maskWidth * maskHeight
    var found: [(id: Int, core: [Bool], cx: Double, cy: Double)] = planes.map {
        ($0.id, [Bool](repeating: false, count: cells), 0, 0)
    }
    var count = [Int](repeating: 0, count: planes.count)
    for cell in 0..<cells {
        var best = -1
        for (index, plane) in planes.enumerated() where plane.values[cell] >= 0.5 {
            if best < 0 || plane.values[cell] > planes[best].values[cell] { best = index }
        }
        guard best >= 0 else { continue }
        found[best].core[cell] = true
        found[best].cx += Double(cell % maskWidth)
        found[best].cy += Double(cell / maskWidth)
        count[best] += 1
    }
    for index in found.indices {
        found[index].cx = count[index] > 0 ? found[index].cx / Double(count[index]) : .infinity
        found[index].cy = count[index] > 0 ? found[index].cy / Double(count[index]) : .infinity
    }
    found.sort { a, b in
        if a.cx != b.cx { return a.cx < b.cx }
        if a.cy != b.cy { return a.cy < b.cy }
        return a.id < b.id
    }
    return PeopleOracle(maskWidth: maskWidth, maskHeight: maskHeight, ids: found.map(\.id),
                        cores: found.map(\.core))
}

/// Part 1's coverage comparison; answers each person's (recall, leak, cells).
private func comparePeople(_ masks: [[UInt8]], aggregate: [UInt8], width: Int, height: Int,
                           oracle: PeopleOracle, name: String) throws -> [(Double, Double, Int)] {
    let w = oracle.maskWidth, h = oracle.maskHeight
    var centre = [Int](repeating: 0, count: w * h)
    for cy in 0..<h {
        let py = min(height - 1, Int((Double(cy) + 0.5) * Double(height) / Double(h)))
        for cx in 0..<w {
            let px = min(width - 1, Int((Double(cx) + 0.5) * Double(width) / Double(w)))
            centre[cy * w + cx] = py * width + px
        }
    }
    var rows: [(Double, Double, Int)] = []
    for (k, mask) in masks.enumerated() {
        var own = 0, ownHit = 0, other = 0, otherHit = 0
        for cell in 0..<(w * h) {
            let p = centre[cell]
            guard aggregate[p] >= 128 else { continue }
            if oracle.cores[k][cell] {
                own += 1
                if mask[p] >= 128 { ownHit += 1 }
            } else if oracle.cores.indices.contains(where: { $0 != k && oracle.cores[$0][cell] }) {
                other += 1
                if mask[p] >= 128 { otherHit += 1 }
            }
        }
        let recall = own > 0 ? Double(ownHit) / Double(own) : 0
        let leak = other > 0 ? Double(otherHit) / Double(other) : 0
        try require(own >= peopleOracleMinimumCells,
                    "\(name) oracle instance \(oracle.ids[k]) has \(own) core cells under the aggregate "
                    + "(at least \(peopleOracleMinimumCells))")
        try require(recall >= peopleOracleRecall && leak <= peopleOracleLeak,
                    "\(name) person \(k + 1) matches Vision instance \(oracle.ids[k]): recall \(recall) "
                    + "(at least \(peopleOracleRecall)), leak \(leak) (at most \(peopleOracleLeak))")
        rows.append((recall, leak, own))
    }
    return rows
}

private func errorText(_ body: () throws -> Void) -> String? {
    do { try body(); return nil } catch { return error.localizedDescription }
}

/// The pure build and argmax, against hand-computed answers.
private func verifyPeopleSynthetic() throws {
    let full = [UInt8](repeating: 255, count: 5)
    let map = try PersonInstances.build(
        aggregate: full, maskWidth: 5, maskHeight: 1,
        instances: [(1, [200, 100, 0, 128, 127]), (2, [100, 200, 0, 128, 127])], width: 5, height: 1)
    try require(map.ids == [1, 2, 0, 1, 0], "The ID map is the thresholded argmax, ties to the earlier id: \(map.ids)")
    let none = errorText {
        _ = try PersonInstances.build(aggregate: full, maskWidth: 5, maskHeight: 1, instances: [],
                                      width: 5, height: 1)
    }
    try require(none == PersonInstances.crowdMessage, "No instances is the crowd refusal: \(none ?? "a map")")
    let thin = errorText {
        _ = try PersonInstances.build(aggregate: [255, 0, 0, 0, 0], maskWidth: 5, maskHeight: 1,
                                      instances: [(1, [255, 255, 255, 255, 255])], width: 5, height: 1)
    }
    try require(thin == PersonInstances.crowdMessage,
                "A person whose own cells are under a quarter person (1 of 5) is dropped: \(thin ?? "a map")")
    // A faint haze far from a person's own cells owns nothing: id 1's cells
    // are 0–1 (eligible 0–2), id 2's cell is 4 (eligible 3–5), so pixel 5,
    // where only id 1's haze is above 0, is nobody's, as are 2 and 3, where
    // the only eligible person's coverage is 0.
    let bounded = try PersonInstances.build(
        aggregate: [UInt8](repeating: 255, count: 6), maskWidth: 6, maskHeight: 1,
        instances: [(1, [200, 200, 0, 0, 0, 10]), (2, [0, 0, 0, 0, 200, 0])], width: 6, height: 1)
    try require(bounded.owner == [1, 1, 0, 0, 2, 0],
                "A person owns pixels only within one cell of their own cells: \(bounded.owner)")
    let ordered = try PersonInstances.build(
        aggregate: [UInt8](repeating: 255, count: 6), maskWidth: 6, maskHeight: 1,
        instances: [(1, [200, 0, 0, 0, 0, 200]), (2, [0, 200, 200, 0, 0, 0])], width: 6, height: 1)
    try require(ordered.ids == [1, 2, 2, 0, 0, 1] && ordered.instances == [2, 1],
                "People are ordered by centroid, not box: map \(ordered.ids), order \(ordered.instances)")
    print("PASS: people synthetic build — argmax map, crowd refusals and centroid ordering")
}

private func verifyPeoplePhoto(_ photo: PeoplePhoto, in folder: URL) throws {
    let url = folder.appendingPathComponent(photo.name)
    let oracle = try peopleOracle(url)
    let image = try RasterImage.open(url: url)
    let width = image.width, height = image.height
    if let size = photo.size {
        try require(width == size.width && height == size.height,
                    "\(photo.name) opens \(size.width)×\(size.height) (got \(width)×\(height))")
    }
    try require(photo.portrait == (height > width), "\(photo.name) orientation: \(width)×\(height)")
    let document = try peopleDocument(image)
    defer { document.close() }
    let native = document.doc
    let capture = try SelectionCapture(document: document)
    var start = Date()
    let analysis = try SubjectTargeting.analyze(capture, model: .people, in: document)
    let cold = Date().timeIntervalSince(start) * 1000
    start = Date()
    _ = try SubjectTargeting.analyze(capture, model: .people, in: document)
    let warm = Date().timeIntervalSince(start) * 1000
    let count = analysis.instanceCount
    try require(count == oracle.ids.count,
                "\(photo.name) has as many people as Vision's own instance request (\(oracle.ids.count), got \(count))")
    if let exact = photo.count {
        try require(count == exact, "\(photo.name) has exactly \(exact) people (got \(count))")
    } else {
        try require(count >= photo.minimumCount, "\(photo.name) has at least \(photo.minimumCount) people (got \(count))")
    }
    guard let aggregate = analysis.aggregateMask else { throw ProbeFailure(description: "No aggregate") }
    var masks: [[UInt8]] = []
    for k in 1...count {
        guard let mask = analysis.mask(subjects: [k]) else {
            throw ProbeFailure(description: "\(photo.name) person \(k) has no mask")
        }
        masks.append(mask)
    }
    let at = { (x: Double, y: Double) -> Int in
        min(height - 1, Int(y * Double(height))) * width + min(width - 1, Int(x * Double(width)))
    }
    let point = { (x: Double, y: Double) in CGPoint(x: x * Double(width), y: y * Double(height)) }

    // Part 1: against Vision's raw output.
    let compared = try comparePeople(masks, aggregate: aggregate, width: width, height: height,
                                     oracle: oracle, name: photo.name)

    // Faces.
    var hits: [Int] = []
    for face in photo.faces {
        guard let hit = analysis.subject(at: point(face.x, face.y)) else {
            throw ProbeFailure(description: "\(photo.name) face (\(face.x), \(face.y)) hits nobody")
        }
        if let expected = face.person {
            try require(hit == expected, "\(photo.name) face (\(face.x), \(face.y)) hits person \(expected) (got \(hit))")
        }
        hits.append(hit)
    }
    try require(Set(hits).count == hits.count && hits == hits.sorted(),
                "\(photo.name) faces hit distinct people numbered in order: \(hits)")
    for face in photo.aggregateFaces {
        try require(aggregate[at(face.x, face.y)] >= 128,
                    "\(photo.name) face (\(face.x), \(face.y)) lies in All People: \(aggregate[at(face.x, face.y)])")
    }
    for (index, face) in photo.faces.enumerated() {
        let own = masks[hits[index] - 1]
        try require(own[at(face.x, face.y)] >= 128,
                    "\(photo.name) person \(hits[index]) covers its own face: \(own[at(face.x, face.y)])")
        for (other, elsewhere) in photo.faces.enumerated() where other != index {
            try require(own[at(elsewhere.x, elsewhere.y)] < 128,
                        "\(photo.name) person \(hits[index]) leaves face \(other + 1) out: \(own[at(elsewhere.x, elsewhere.y)])")
        }
    }

    // Areas, disjointness and union.
    let aggregateHard = hardCount(aggregate)
    let hardAreas = masks.map(hardCount)
    if let share = photo.minimumShare {
        for (k, area) in hardAreas.enumerated() {
            try require(Double(area) >= share * Double(aggregateHard),
                        "\(photo.name) person \(k + 1) hard area \(area) ≥ \(share) × aggregate \(aggregateHard)")
        }
    }
    var overlap = 0, union = 0
    for p in 0..<(width * height) {
        var positive = 0, hard = false
        for mask in masks {
            if mask[p] > 0 { positive += 1 }
            if mask[p] >= 128 { hard = true }
        }
        if positive > 1 { overlap += 1 }
        if hard { union += 1 }
    }
    try require(overlap == 0, "\(photo.name) masks are disjoint: \(overlap) pixels in two")
    try require(Double(union) >= peopleUnionShare * Double(aggregateHard),
                "\(photo.name) hard union \(union) ≥ \(peopleUnionShare) × aggregate \(aggregateHard)")
    let centroids = masks.map { centroidX($0, width: width) }
    if let step = photo.centroidStep {
        for k in 1..<count {
            try require(centroids[k] - centroids[k - 1] >= step * Double(width),
                        "\(photo.name) x centroids increase by ≥ \(step)·W: \(centroids.map { Int($0) })")
        }
    }

    // Boxes.
    let boxes = analysis.instanceBounds
    for k in 1...count {
        let grown = boxes[k - 1].insetBy(dx: -peopleBoxGrowth * Double(width), dy: -peopleBoxGrowth * Double(height))
        let box = hardBox(masks[k - 1], width: width)
        try require(!box.isNull && grown.contains(box),
                    "\(photo.name) person \(k)'s hard box \(box) lies in its map box \(boxes[k - 1]) grown 2%")
        try require(analysis.subjects(intersecting: boxes[k - 1]).contains(k),
                    "\(photo.name) person \(k)'s map box hits it")
    }

    // MCP parity, one Undo each.
    let inspected = try call(document, "inspect_subjects", ["model": "people"])
    try require(inspected["instance_count"] as? Int == count && inspected["model"] as? String == "people"
                && document.canvasSelection == nil,
                "\(photo.name) inspect_subjects model people is read-only and counts \(count): \(inspected)")
    for k in 1...count {
        let reply = try call(document, "select_subjects", ["model": "people", "instances": [k]])
        try require(reply["model"] as? String == "people" && document.canvasSelection?.maskBytes() == masks[k - 1],
                    "\(photo.name) select_subjects person \(k) equals mask \(k)")
        document.undoManager?.undo()
        try require(document.canvasSelection == nil, "\(photo.name) one Undo reverses person \(k)")
    }
    try require(document.doc === native && !document.isDocumentEdited, "\(photo.name) changes no pixel")
    let matches = compared.map { String(format: "%.3f/%.3f/%d", $0.0, $0.1, $0.2) }
    print(String(format: "PASS: %@ %d×%d, %d people (Vision ids %@), oracle recall/leak/cells %@, faces hit %@, "
                    + "hard areas %@ of aggregate %d, union %.3f, x centroids %@; analysis cold %.0f ms, warm %.0f ms",
                 photo.name, width, height, count, "\(oracle.ids)", "\(matches)", "\(hits)", "\(hardAreas)",
                 aggregateHard, Double(union) / Double(max(1, aggregateHard)), "\(centroids.map { Int($0) })",
                 cold, warm))
}

/// The sheet on the four-person photo.
private func verifyPeopleSheet(in folder: URL) throws {
    let photo = peoplePhotos[0]
    let image = try RasterImage.open(url: folder.appendingPathComponent(photo.name))
    let width = image.width, height = image.height
    let document = try peopleDocument(image)
    defer { document.close() }
    let native = document.doc
    let controller = try editor(document)
    let sheet = try SubjectTargetsSheetController(editor: controller, document: document, model: .people)
    try present(sheet, in: controller)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, message: "the people preview")
    guard let table = allViews(sheet.view).compactMap({ $0 as? NSTableView }).first,
          let apply = allViews(sheet.view).compactMap({ $0 as? NSButton }).first(where: { $0.title == "Apply" }),
          let field = allViews(sheet.view).compactMap({ $0 as? NSTextField }).first(where: { $0.isEditable }) else {
        throw ProbeFailure(description: "People picker table, Apply or feather field missing")
    }
    let count = sheet.analysis.instanceCount
    let labels = (0..<count).compactMap { (sheet.tableView(table, viewFor: nil, row: $0) as? NSTextField)?.stringValue }
    try require(table.numberOfRows == count && labels.enumerated().allSatisfy { $0.element.hasPrefix("Person \($0.offset + 1) ") },
                "The rows are Person N: \(labels)")
    let statuses = allViews(sheet.view).compactMap { ($0 as? NSTextField)?.stringValue }
    try require(statuses.contains { $0.hasPrefix("\(count) of \(count) people selected.") },
                "The status says every person is selected")
    let face = photo.faces[0]
    let first = CGPoint(x: face.x * Double(width), y: face.y * Double(height))
    sheet.toggleObject(at: first)
    try require(!sheet.selectedInstances.contains(1) && sheet.selectedInstances.count == count - 1,
                "A click on person 1 unchooses them")
    sheet.toggleObject(at: first)
    try require(sheet.selectedInstances.contains(1), "A second click chooses person 1 again")
    guard let hard = sheet.analysis.mask(subjects: [2]) else { throw ProbeFailure(description: "Person 2 mask") }
    table.selectRowIndexes(IndexSet(integer: 1), byExtendingSelection: false)
    field.stringValue = "4"
    try require(sheet.feather == 4, "The feather field reads 4")
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, message: "the person 2 preview")
    let narrowed = allViews(sheet.view).compactMap { ($0 as? NSTextField)?.stringValue }
    try require(narrowed.contains { $0.hasPrefix("1 of \(count) people selected.") }, "The status counts one person")
    ActionRecorder.shared.start()
    apply.performClick(nil)
    let recorded = ActionRecorder.shared.stop()
    guard let soft = document.canvasSelection?.maskBytes() else {
        throw ProbeFailure(description: "The people picker's Apply left no selection: \(AutomationAlerts.recorded)")
    }
    try require(softCount(soft) > softCount(hard), "feather 4 softens person 2: \(softCount(soft)) against \(softCount(hard))")
    try require(document.doc === native && !document.isDocumentEdited && AutomationAlerts.recorded.isEmpty,
                "The picker's Apply changes no pixel and shows no alert: \(AutomationAlerts.recorded)")
    let arguments = recorded.first?.arguments ?? [:]
    try require(recorded.count == 1 && recorded.first?.tool == "select_subjects"
                && arguments["model"] as? String == "people" && arguments["instances"] as? [Int] == [2]
                && (arguments["feather"] as? NSNumber)?.doubleValue == 4
                && recorded.first?.note == "Select ▸ Select People…",
                "Apply records one select_subjects step: \(recorded.map { ($0.tool, $0.arguments, $0.note ?? "") })")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil && document.undoManager?.canUndo != true, "The picker's Apply is one Undo")
    let played = ActionPlayer.run(Action(name: "People replay", steps: recorded), on: document)
    try require(played.ok && document.canvasSelection?.maskBytes() == soft, "The recorded step replays the same mask")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil, "One Undo reverses the replay")

    // All People.
    let again = try SubjectTargetsSheetController(editor: controller, document: document, model: .people)
    try present(again, in: controller)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, message: "the second people preview")
    ActionRecorder.shared.start()
    again.selectAllObjects(nil)
    let allRecorded = ActionRecorder.shared.stop()
    guard let everyone = document.canvasSelection?.maskBytes() else {
        throw ProbeFailure(description: "All People left no selection: \(AutomationAlerts.recorded)")
    }
    try require(allRecorded.count == 1 && allRecorded.first?.tool == "select_people"
                && allRecorded.first?.note == "Select ▸ All People",
                "All People records one select_people step: \(allRecorded.map { ($0.tool, $0.arguments) })")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil && document.undoManager?.canUndo != true, "All People is one Undo")
    _ = try call(document, "select_people")
    try require(document.canvasSelection?.maskBytes() == everyone, "The picker's All People equals select_people")
    document.undoManager?.undo()
    try pumpUntil({ controller.view.window?.attachedSheet == nil }, message: "the people sheet to close")
    print("PASS: the people picker — Person N rows, status, click toggle, feather 4, one Undo, recorded "
          + "select_subjects replay, All People equals select_people")
}

func verifyPeopleInstances() throws {
    guard let people = ProcessInfo.processInfo.environment["DR_PEOPLE_PHOTO_DIR"] else {
        throw ProbeFailure(description: "DR_PEOPLE_PHOTO_DIR is not set; pass --people-dir")
    }
    guard let photos = ProcessInfo.processInfo.environment["DR_SELECTION_PHOTO_DIR"] else {
        throw ProbeFailure(description: "DR_SELECTION_PHOTO_DIR is not set; pass --photos-dir")
    }
    let folder = URL(fileURLWithPath: people), licensed = URL(fileURLWithPath: photos)
    AutomationAlerts.reset()
    try verifyPeopleSynthetic()
    for photo in peoplePhotos { try verifyPeoplePhoto(photo, in: folder) }
    try verifyPeopleSheet(in: folder)

    // No people: fur.png refuses in-band, from both tools and the sheet.
    let fur = try peopleDocument(try RasterImage.open(url: licensed.appendingPathComponent("fur.png")))
    defer { fur.close() }
    let inspected = try peopleRefusal(fur, "inspect_subjects", ["model": "people"])
    let selected = try peopleRefusal(fur, "select_subjects", ["model": "people"])
    let sheet = errorText { _ = try SubjectTargetsSheetController(editor: try editor(fur), document: fur, model: .people) }
    try require(inspected == peopleNoPeople && selected == peopleNoPeople && sheet == peopleNoPeople
                && fur.canvasSelection == nil,
                "fur.png refuses people (got \(inspected ?? "nil"), \(selected ?? "nil"), \(sheet ?? "nil"))")
    let faces = try peopleRefusal(fur, "select_subjects", ["model": "faces"])
    try require(faces == peopleBadModel, "An unknown model refuses with the exact text (got \(faces ?? "nil"))")

    // The precision gates.
    let item = NSMenuItem(title: "Select People…", action: NSSelectorFromString("showPeopleTargets:"), keyEquivalent: "")
    let portrait = try RasterImage.open(url: licensed.appendingPathComponent("portrait.png"))
    let deep = try peopleDocument(portrait)
    defer { deep.close() }
    _ = try call(deep, "convert_precision", ["mode": "rgba16", "structure": "preserve"])
    let deepEditor = try editor(deep)
    try require(deepEditor.validatePrecisionItem(item) == nil, "showPeopleTargets: passes the 16-bit gate")
    let scratch = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"]
        ?? NSTemporaryDirectory())
    let url = scratch.appendingPathComponent("people-float-portrait.tiff")
    try writeFloatPortrait(portrait, to: url)
    let float = try floatPortraitDocument(url)
    defer { float.close() }
    let floatEditor = try editor(float)
    try require(floatEditor.validatePrecisionItem(item) == nil, "showPeopleTargets: passes the float gate")
    try require(AutomationAlerts.recorded.isEmpty, "No alert: \(AutomationAlerts.recorded)")
    print("PASS: people refusals on fur.png, the model refusal, and the 16-bit and float gates")
}
