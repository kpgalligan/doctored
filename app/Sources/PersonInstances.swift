import AppKit
import CoreVideo
import Vision

/// Which Vision model a subject picker separates: foreground objects
/// (Select ▸ Select Objects…, `model "objects"`) or individual people
/// (Select ▸ Select People…, `model "people"`). The raw value is
/// the MCP `model` argument; every `.objects` string is the one the objects
/// picker and its tools used before people existed.
enum SubjectModel: String, CaseIterable {
    case objects, people

    /// The sheet's title and the commit's undo name.
    var title: String { self == .objects ? "Select Objects" : "Select People" }
    /// The analysis' progress title.
    var findTitle: String { self == .objects ? "Find Objects" : "Find People" }
    /// The mask generation's progress title.
    var maskTitle: String { self == .objects ? "Select Objects" : "Select People" }
    var columnTitle: String {
        self == .objects ? "Foreground objects (coarse bounds in pixels)"
            : "People, left to right (coarse bounds in pixels)"
    }
    func rowLabel(_ number: Int) -> String { self == .objects ? "Object \(number)" : "Person \(number)" }
    func status(selected: Int, of total: Int) -> String {
        "\(selected) of \(total) \(self == .objects ? "objects" : "people") selected. Model coverage can be refined afterward."
    }
    var emptyStatus: String { self == .objects ? "Select at least one object." : "Select at least one person." }
    var allButtonTitle: String { self == .objects ? "All Objects" : "All People" }
    var hint: String {
        switch self {
        case .objects:
            return "Click objects in the image to toggle them, or select multiple rows with Command/Shift. The region button targets objects touching the existing selection's bounding rectangle. Preview shows their coverage before combining. For person-only coverage, use Select ▸ All People."
        case .people:
            return "People are numbered left to right. Click people in the image to toggle them, or select multiple rows with Command/Shift. The region button targets people touching the existing selection's bounding rectangle. All People selects everyone as one aggregate, as Select ▸ All People does. Vision may merge people who stand close together; Select ▸ All People still covers them."
        }
    }
    /// The recorded Actions step's note.
    var recordNote: String { self == .objects ? "Select ▸ Select Objects" : "Select ▸ Select People…" }
    var chooseRefusal: String {
        self == .objects ? "Choose one or more existing foreground objects." : "Choose one or more existing people."
    }
    var instancesRefusal: String {
        self == .objects ? "instances must be a nonempty list of existing foreground objects."
            : "instances must be a nonempty list of existing people."
    }
    var noHitRefusal: String { self == .objects ? "No object at this point." : "No person at this point." }
    /// What `inspect_subjects` reports as its `segmenter`.
    var segmenter: String {
        self == .objects ? "Vision foreground instances"
            : "Vision person instances over accurate person segmentation"
    }
}

/// One image's people, as `SubjectAnalysis` addresses them.
///
/// `ids` is the coarse targeting map at Vision's own instance-mask size (each
/// cell the Vision id that covers it best, 0 for nobody); `owner` is the same
/// question answered at canvas size, from each instance's coverage resampled
/// to the canvas, among the people eligible there (`PersonInstances.eligibility`;
/// a pixel nobody is eligible for is nobody's and stays in All People only).
/// A person's selection is `aggregate` (the accurate person
/// segmenter's full-resolution coverage) where `owner` names them, so the
/// edges come from the accurate model and only the split between people from
/// the coarse instance model.
struct PersonInstanceMap {
    let idWidth: Int, idHeight: Int, ids: [UInt8]
    /// The Vision ids kept, ordered left to right.
    let instances: [Int]
    let aggregate: [UInt8]
    let owner: [UInt8]
}

/// Individual people through `VNGeneratePersonInstanceMaskRequest`.
///
/// The request's own `instanceMask` is never read. Each person's coverage is
/// asked for separately (`generateMask(forInstances:)`) and every cell or
/// pixel goes to the person whose coverage is highest there (argmax): the
/// same rule at both resolutions, so the coarse map a click is tested against
/// and the full-resolution split it selects agree.
enum PersonInstances {
    /// A coarse cell belongs to someone only when their coverage reaches one
    /// half: below that the instance model itself says "probably not".
    static let cellThreshold: UInt8 = 128
    /// A kept person must be at least this much real person, measured as the
    /// share of their own coarse cells (coverage at least `cellThreshold`)
    /// where the accurate segmenter's coverage is at least one half, so an
    /// instance the accurate model mostly rejects is not offered as a person;
    /// one quarter still keeps a small or partly hidden person, the instance
    /// model's cells being coarser than the accurate segmenter's edge.
    static let minimumAggregateShare = 0.25
    static let crowdMessage = "Vision could not separate these people; use Select ▸ All People."

    /// Runs the instance request beside the All People requests in one
    /// `perform`, applies All People's gate first (so an image with nobody in
    /// it refuses with `SubjectTargeting.noPeopleMessage`), then builds the map.
    static func analyze(_ image: CGImage) throws -> PersonInstanceMap {
        let request = VNGeneratePersonInstanceMaskRequest()
        let requests = SubjectTargeting.personRequests()
        do {
            try VNImageRequestHandler(cgImage: image, options: [:])
                .perform([request, requests.aggregate, requests.humans, requests.faces])
        } catch {
            throw SubjectSelection.Failure.vision(error.localizedDescription)
        }
        let width = image.width, height = image.height
        let aggregate = try SubjectTargeting.aggregateCoverage(requests, width: width, height: height)
        var planes: [(id: UInt8, coverage: [UInt8])] = []
        var maskWidth = 0, maskHeight = 0
        // A missing observation is no instances, which `build` refuses as a crowd.
        if let observation = request.results?.first {
            var ids = observation.allInstances
            // Asking for 0 returns the background, the inverse of a person.
            ids.remove(0)
            for id in ids {
                guard id <= Int(UInt8.max),
                      let buffer = try? observation.generateMask(forInstances: IndexSet(integer: id))
                else { throw SubjectSelection.Failure.unreadableMask }
                let planeWidth = CVPixelBufferGetWidth(buffer), planeHeight = CVPixelBufferGetHeight(buffer)
                // At the buffer's own size, so `coverage` only converts and clamps.
                guard let bytes = SubjectSelection.coverage(buffer, width: planeWidth, height: planeHeight),
                      planes.isEmpty || (planeWidth == maskWidth && planeHeight == maskHeight)
                else { throw SubjectSelection.Failure.unreadableMask }
                maskWidth = planeWidth
                maskHeight = planeHeight
                planes.append((UInt8(id), bytes))
            }
        }
        return try build(aggregate: aggregate, maskWidth: maskWidth, maskHeight: maskHeight,
                         instances: planes, width: width, height: height)
    }

    /// The pure half of `analyze`: the coarse map, the canvas-sized owner,
    /// the keep rule and the left-to-right order. Throws `crowdMessage` when
    /// no person survives.
    static func build(aggregate: [UInt8], maskWidth: Int, maskHeight: Int,
                      instances: [(id: UInt8, coverage: [UInt8])], width: Int, height: Int) throws -> PersonInstanceMap {
        guard !instances.isEmpty else { throw DoctoredCoreError(message: crowdMessage) }
        let cells = maskWidth * maskHeight, pixels = width * height
        guard cells > 0, pixels > 0, aggregate.count == pixels,
              instances.allSatisfy({ $0.id != 0 && $0.coverage.count == cells }) else {
            throw DoctoredCoreError(message: SubjectSelection.Failure.unreadableMask.message)
        }
        var ids = [UInt8](repeating: 0, count: cells)
        do {
            var best = [UInt8](repeating: 0, count: cells)
            for plane in instances {
                accumulate(plane.coverage, id: plane.id, threshold: cellThreshold, best: &best, owner: &ids)
            }
        }
        // The cell each canvas column and row lies in: the nearest-neighbour
        // upscale of a coarse region to the canvas.
        let cellColumn = (0..<width).map { min($0 * maskWidth / width, maskWidth - 1) }
        let cellRow = (0..<height).map { min($0 * maskHeight / height, maskHeight - 1) }
        // One canvas plane at a time: only `best`, `owner` and the current
        // resampled plane are canvas-sized here.
        var owner = [UInt8](repeating: 0, count: pixels)
        do {
            var best = [UInt8](repeating: 0, count: pixels)
            for plane in instances {
                let same = maskWidth == width && maskHeight == height
                guard let scaled = same ? Optional(plane.coverage)
                        : SubjectSelection.resampled(plane.coverage, maskWidth, maskHeight,
                                                     toWidth: width, height: height) else {
                    throw DoctoredCoreError(message: SubjectSelection.Failure.unreadableMask.message)
                }
                let eligible = eligibility(ids, id: plane.id, width: maskWidth, height: maskHeight)
                for y in 0..<height {
                    let row = y * width, cellRowStart = cellRow[y] * maskWidth
                    for x in 0..<width where eligible[cellRowStart + cellColumn[x]] {
                        let p = row + x, value = scaled[p]
                        if value > best[p] {
                            best[p] = value
                            owner[p] = plane.id
                        }
                    }
                }
            }
        }

        // The keep rule, over the person's own cells: a person's share is
        // measured where the instance model itself says "this person" (coverage
        // at least one half), never over the pixels they won by argmax, so a
        // small person's wide faint haze cannot dilute it. The aggregate is
        // read at each cell's centre pixel.
        var ownCells = [Int](repeating: 0, count: 256), ownHard = [Int](repeating: 0, count: 256)
        for plane in instances {
            let id = Int(plane.id)
            for cy in 0..<maskHeight {
                let py = min(Int((Double(cy) + 0.5) * Double(height) / Double(maskHeight)), height - 1)
                for cx in 0..<maskWidth where plane.coverage[cy * maskWidth + cx] >= cellThreshold {
                    let px = min(Int((Double(cx) + 0.5) * Double(width) / Double(maskWidth)), width - 1)
                    ownCells[id] += 1
                    if aggregate[py * width + px] >= 128 { ownHard[id] += 1 }
                }
            }
        }
        var cellCount = [Int](repeating: 0, count: 256)
        var sumX = [Double](repeating: 0, count: 256), sumY = [Double](repeating: 0, count: 256)
        for y in 0..<maskHeight {
            for x in 0..<maskWidth {
                let id = Int(ids[y * maskWidth + x])
                guard id != 0 else { continue }
                cellCount[id] += 1
                sumX[id] += Double(x)
                sumY[id] += Double(y)
            }
        }
        var keep = [Bool](repeating: false, count: 256)
        for plane in instances {
            let id = Int(plane.id)
            keep[id] = cellCount[id] > 0 && ownCells[id] > 0
                && Double(ownHard[id]) >= minimumAggregateShare * Double(ownCells[id])
        }
        // A dropped person's cells and pixels become nobody's; nothing is reassigned.
        for i in 0..<cells where ids[i] != 0 && !keep[Int(ids[i])] { ids[i] = 0 }
        for p in 0..<pixels where owner[p] != 0 && !keep[Int(owner[p])] { owner[p] = 0 }

        // Left to right by the x centroid of the person's cells, then top to
        // bottom, then Vision's id: a centroid, not a box, because a raised arm
        // or a neighbour's overlap moves a box edge far from where the body is.
        let kept = instances.map { Int($0.id) }.filter { keep[$0] }.sorted { a, b in
            let ax = sumX[a] / Double(cellCount[a]), bx = sumX[b] / Double(cellCount[b])
            if ax != bx { return ax < bx }
            let ay = sumY[a] / Double(cellCount[a]), by = sumY[b] / Double(cellCount[b])
            if ay != by { return ay < by }
            return a < b
        }
        guard !kept.isEmpty else { throw DoctoredCoreError(message: crowdMessage) }
        return PersonInstanceMap(idWidth: maskWidth, idHeight: maskHeight, ids: ids, instances: kept,
                                 aggregate: aggregate, owner: owner)
    }

    /// Where a person may own canvas pixels, per coarse cell: their cells in
    /// the ID map (coverage at least `cellThreshold` and the highest there)
    /// grown by one cell, 8-connected. Vision's instance coverage has a wide
    /// faint haze; without this bound that haze wins the argmax wherever no
    /// other instance reaches it, so a person would own specks across a
    /// neighbour's body. Their raw half-coverage cells are not enough of a
    /// bound either: where people touch, one person's cells run far into a
    /// neighbour's, whom the map gives them to. One cell is about 4 px on a
    /// 1512-wide photo and about 10 px on a 4032-wide one, enough to keep the
    /// accurate segmenter's soft edge around the person's own cells and well
    /// inside the 2% box margin a person's pixels are held to.
    static func eligibility(_ ids: [UInt8], id: UInt8, width: Int, height: Int) -> [Bool] {
        var eligible = [Bool](repeating: false, count: width * height)
        for y in 0..<height {
            for x in 0..<width where ids[y * width + x] == id {
                for ny in max(y - 1, 0)...min(y + 1, height - 1) {
                    for nx in max(x - 1, 0)...min(x + 1, width - 1) { eligible[ny * width + nx] = true }
                }
            }
        }
        return eligible
    }

    /// The argmax step: where `coverage` reaches `threshold` and beats `best`,
    /// it becomes the best and `id` the owner. Strictly greater, so a tie
    /// keeps the earlier id.
    static func accumulate(_ coverage: [UInt8], id: UInt8, threshold: UInt8,
                           best: inout [UInt8], owner: inout [UInt8]) {
        let count = min(coverage.count, best.count, owner.count)
        for p in 0..<count {
            let value = coverage[p]
            if value >= threshold && value > best[p] {
                best[p] = value
                owner[p] = id
            }
        }
    }
}
