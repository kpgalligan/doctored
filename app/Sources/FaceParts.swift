import AppKit
import Vision

// Select ▸ Face Parts…: Vision face landmarks turned into the core's landmark regions
// (`dr_landmark_coverage`, `DoctoredCore+Landmarks.swift`), which rasterizes each point loop as a
// smooth closed spline into native u16 coverage. This file only CHOOSES points: which landmark
// loops make which part, the forehead the contour lacks, and the eyebrow band. The y flip lives in
// `RedEye.canvasPoints` and nowhere here; face boxes are the landmarks' own canvas bounds for that
// reason.

/// The parts a face can be selected by. The raw value is the MCP spelling.
enum FacePart: String, CaseIterable {
    case face, skin, eyes, irises, eyebrows, lips, mouth

    var title: String {
        switch self {
        case .face: return "Face"
        case .skin: return "Skin"
        case .eyes: return "Eyes"
        case .irises: return "Irises"
        case .eyebrows: return "Eyebrows"
        case .lips: return "Lips"
        case .mouth: return "Mouth"
        }
    }
}

/// One face's landmarks in CANVAS coordinates (row 0 = top).
struct FaceLandmarks {
    /// A landmark outline. `open` is a single line (a band is selected around
    /// it); a closed one is an outline whose inside is selected.
    struct Region {
        let points: [CGPoint]
        let open: Bool
    }

    let bounds: CGRect
    let contour: [CGPoint]
    let leftEye, rightEye, leftEyebrow, rightEyebrow, outerLips, innerLips: Region?
    let leftIris, rightIris: RedEye.Eye?

    /// The parts this face has the landmarks for.
    var availableParts: [FacePart] { FacePart.allCases.filter { !regions($0).isEmpty } }

    /// The face outline: the jaw contour closed over the forehead.
    private var faceLoop: [String: Any]? {
        guard contour.count >= 3 else { return nil }
        let outline = contour + FaceParts.forehead(contour)
        return FaceParts.loop(Region(points: outline, open: false))
    }

    /// The part as kernel region objects (`landmark_region`'s JSON):
    /// `{"include":[LOOP…],"exclude":[LOOP…],"clip":[LOOP…]}`. Empty when the
    /// landmarks it needs are missing.
    func regions(_ part: FacePart) -> [[String: Any]] {
        let single = { (region: Region?) -> [[String: Any]] in
            region.map { [["include": [FaceParts.loop($0)]]] } ?? []
        }
        switch part {
        case .face:
            return faceLoop.map { [["include": [$0]]] } ?? []
        case .skin:
            guard let face = faceLoop else { return [] }
            let holes = [leftEye, rightEye, leftEyebrow, rightEyebrow, outerLips]
                .compactMap { $0.map(FaceParts.loop) }
            return [holes.isEmpty ? ["include": [face]] : ["include": [face], "exclude": holes]]
        case .eyes:
            return single(leftEye) + single(rightEye)
        case .irises:
            let pairs: [(RedEye.Eye?, Region?)] = [(leftIris, leftEye), (rightIris, rightEye)]
            return pairs.compactMap { (iris, eye) -> [String: Any]? in
                guard let iris else { return nil }
                let circle = FaceParts.loop(Region(points: FaceParts.circle(iris), open: false))
                guard let eye else { return ["include": [circle]] }
                return ["include": [circle], "clip": [FaceParts.loop(eye)]]
            }
        case .eyebrows:
            return single(leftEyebrow) + single(rightEyebrow)
        case .lips:
            guard let outer = outerLips else { return [] }
            guard let inner = innerLips else { return [["include": [FaceParts.loop(outer)]]] }
            return [["include": [FaceParts.loop(outer)], "exclude": [FaceParts.loop(inner)]]]
        case .mouth:
            return single(innerLips)
        }
    }
}

extension FaceLandmarks {
    /// nil when Vision located none of the landmarks a part is built from.
    init?(landmarks: VNFaceLandmarks2D, size: CGSize) {
        let region = { (r: VNFaceLandmarkRegion2D?) in FaceParts.region(r, size) }
        var contour: [CGPoint] = []
        if let jaw = landmarks.faceContour, jaw.pointCount >= 3 {
            contour = RedEye.canvasPoints(jaw, size)
        }
        let draft = FaceLandmarks(
            bounds: .null, contour: contour,
            leftEye: region(landmarks.leftEye), rightEye: region(landmarks.rightEye),
            leftEyebrow: region(landmarks.leftEyebrow), rightEyebrow: region(landmarks.rightEyebrow),
            outerLips: region(landmarks.outerLips), innerLips: region(landmarks.innerLips),
            leftIris: RedEye.eye(outline: landmarks.leftEye, pupil: landmarks.leftPupil, size: size),
            rightIris: RedEye.eye(outline: landmarks.rightEye, pupil: landmarks.rightPupil, size: size))
        guard !draft.availableParts.isEmpty else { return nil }
        // The box of every landmark and the forehead arc: the face's extent
        // as the parts see it.
        var points = landmarks.allPoints.map { RedEye.canvasPoints($0, size) } ?? []
        points += contour + FaceParts.forehead(contour)
        let pieces = [draft.leftEye, draft.rightEye, draft.leftEyebrow, draft.rightEyebrow,
                      draft.outerLips, draft.innerLips]
        for piece in pieces { points += piece?.points ?? [] }
        let xs = points.map(\.x), ys = points.map(\.y)
        guard let minX = xs.min(), let maxX = xs.max(), let minY = ys.min(), let maxY = ys.max()
        else { return nil }
        self = FaceLandmarks(
            bounds: CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY),
            contour: contour, leftEye: draft.leftEye, rightEye: draft.rightEye,
            leftEyebrow: draft.leftEyebrow, rightEyebrow: draft.rightEyebrow,
            outerLips: draft.outerLips, innerLips: draft.innerLips,
            leftIris: draft.leftIris, rightIris: draft.rightIris)
    }
}

/// Every face Vision finds in one captured composite, with its landmarks.
struct FaceAnalysis {
    let width: Int, height: Int
    /// Numbered 1… left to right: by `bounds.minX`, then `minY`.
    let faces: [FaceLandmarks]

    /// Face rectangles first (Part 0's detector, the one All People asks),
    /// then landmarks for exactly those faces. Throws
    /// `SubjectTargeting.noFaceMessage` when no face is found and
    /// `FaceParts.noLandmarksMessage` when none has usable landmarks.
    init(image: CGImage) throws {
        let size = CGSize(width: image.width, height: image.height)
        let handler = VNImageRequestHandler(cgImage: image, options: [:])
        let detect = VNDetectFaceRectanglesRequest()
        do { try handler.perform([detect]) } catch {
            throw DoctoredCoreError(message: RedEye.Failure.vision(error.localizedDescription).message)
        }
        let found = detect.results ?? []
        guard !found.isEmpty else { throw DoctoredCoreError(message: SubjectTargeting.noFaceMessage) }
        let request = VNDetectFaceLandmarksRequest()
        request.inputFaceObservations = found
        do { try handler.perform([request]) } catch {
            throw DoctoredCoreError(message: RedEye.Failure.vision(error.localizedDescription).message)
        }
        let located = (request.results ?? []).compactMap { face in
            face.landmarks.flatMap { FaceLandmarks(landmarks: $0, size: size) }
        }
        guard !located.isEmpty else { throw DoctoredCoreError(message: FaceParts.noLandmarksMessage) }
        width = image.width
        height = image.height
        faces = located.sorted {
            ($0.bounds.minX, $0.bounds.minY) < ($1.bounds.minX, $1.bounds.minY)
        }
    }

    /// The 1-based face whose box contains `point`, the smallest when boxes
    /// overlap; nil when none does.
    func face(at point: CGPoint) -> Int? {
        faces.indices.filter { faces[$0].bounds.contains(point) }
            .min { faces[$0].bounds.width * faces[$0].bounds.height
                < faces[$1].bounds.width * faces[$1].bounds.height }
            .map { $0 + 1 }
    }

    /// The core request for `parts` of the 1-based `faces`. Throws
    /// `FaceParts.noPartsMessage` when those faces have none of those parts.
    func regionsJSON(parts: [FacePart], faces chosen: [Int]) throws -> String {
        guard !chosen.isEmpty, chosen.allSatisfy({ (1...faces.count).contains($0) }) else {
            throw DoctoredCoreError(message: "Choose one or more of the faces found.")
        }
        var regions: [[String: Any]] = []
        for face in chosen {
            for part in parts { regions += faces[face - 1].regions(part) }
        }
        guard !regions.isEmpty else { throw DoctoredCoreError(message: FaceParts.noPartsMessage) }
        guard regions.count <= FaceParts.maxRegions else {
            throw DoctoredCoreError(message: FaceParts.tooManyRegionsMessage)
        }
        let data = try JSONSerialization.data(withJSONObject: ["regions": regions])
        return String(decoding: data, as: UTF8.self)
    }
}

/// Face Parts' rules and constants.
enum FaceParts {
    /// The forehead's height over the contour's end chord, as a fraction of
    /// the chin's depth below it. Facial thirds put the hairline one third of
    /// the face above the brow line and the chin two thirds below it; Vision's
    /// jaw contour ends at the temples, a little under the brows, so the
    /// hairline sits about half the chin's depth above the chord. 0.6 keeps
    /// the arc clear of the brows on a tilted head without reaching far into
    /// the hair.
    static let foreheadRatio: CGFloat = 0.6
    /// Points on the forehead arc between the contour's two ends.
    static let foreheadPoints = 9
    /// A single-line eyebrow's band half-width, as a fraction of its own
    /// length: a brow is about a fifth as thick as it is long.
    static let bandRatio: CGFloat = 0.1
    /// Points on an iris circle.
    static let irisPoints = 16
    /// An OPEN region whose end chord is under this fraction of its widest
    /// point pair turns back on itself — it outlines both edges, as Vision's
    /// six-point brows do (end chord 2.6 px, widest 26.4 px on the portrait
    /// fixture) — and is filled as a closed loop rather than banded.
    static let closedChordRatio: CGFloat = 0.5

    static let noLandmarksMessage = "A face was found, but its landmarks could not be located."
    static let noPartsMessage = "Vision could not locate the chosen parts on the chosen faces."
    static let tooManyRegionsMessage = "Too many faces and parts for one selection. Choose fewer faces or parts."
    /// The most regions one core request may hold; the master copy is
    /// `landmark_region::MAX_LANDMARK_REGIONS`. Checked here so a crowd with
    /// every part chosen is refused in plain words, not the core's.
    static let maxRegions = 1_024

    /// What one landmark coverage pass may hold over a `width` x `height`
    /// canvas. The core figure, whose master copy is
    /// `landmark_region::coverage_scratch` (`path_geometry::coverage_scratch`
    /// — 106 MiB of rasterizer edge lists, the plane it answers and 24 bytes
    /// a column — plus three more u16 planes), then the u16 output this side
    /// holds and 32 MiB for parsing the request (`ffi_path::shape_scratch`,
    /// 24 bytes a byte of JSON, over a request of a hundred faces).
    static func coverageScratch(width: Int, height: Int) -> UInt64 {
        let pixels = UInt64(width) * UInt64(height)
        let core = 106 * 1024 * 1024 + pixels * 2 + UInt64(width) * 24 + 3 * pixels * 2
        return core + pixels * 2 + 32 * 1024 * 1024
    }

    /// The widest distance between two of the points.
    static func widest(_ points: [CGPoint]) -> CGFloat {
        var widest: CGFloat = 0
        for (index, a) in points.enumerated() {
            for b in points[(index + 1)...] { widest = max(widest, hypot(a.x - b.x, a.y - b.y)) }
        }
        return widest
    }

    /// A Vision region in canvas points, classified (see `closedChordRatio`);
    /// nil when it has too few points to outline anything.
    static func region(_ region: VNFaceLandmarkRegion2D?, _ size: CGSize) -> FaceLandmarks.Region? {
        guard let region, region.pointCount > 0 else { return nil }
        let points = RedEye.canvasPoints(region, size)
        let widest = widest(points)
        guard widest > 0, let first = points.first, let last = points.last else { return nil }
        var open = region.pointsClassification == .openPath
        if open, hypot(first.x - last.x, first.y - last.y) < closedChordRatio * widest { open = false }
        guard points.count >= (open ? 2 : 3) else { return nil }
        return FaceLandmarks.Region(points: points, open: open)
    }

    /// A region as a kernel loop: a closed outline, or a single line's band
    /// of half-width `bandRatio` × its own widest point pair. Coordinates are
    /// rounded to 1/1000 px, far below what the rasterizer resolves.
    static func loop(_ region: FaceLandmarks.Region) -> [String: Any] {
        let points = region.points.map { [($0.x * 1000).rounded() / 1000, ($0.y * 1000).rounded() / 1000] }
        guard region.open else { return ["points": points] }
        return ["points": points, "half_width": Double(bandRatio * widest(region.points))]
    }

    /// The forehead arc that closes the jaw `contour`: `foreheadPoints`
    /// points on the half-ellipse over the contour's end chord, on the side
    /// away from the chin (the contour point farthest from the chord), from
    /// the contour's last point back to its first. Semi-axes: half the chord
    /// along it, `foreheadRatio` × the chin's depth across it. Empty for a
    /// contour with no chord or no depth.
    static func forehead(_ contour: [CGPoint]) -> [CGPoint] {
        guard contour.count >= 3, let first = contour.first, let last = contour.last else { return [] }
        let length = hypot(last.x - first.x, last.y - first.y)
        guard length > 0 else { return [] }
        let along = CGPoint(x: (last.x - first.x) / length, y: (last.y - first.y) / length)
        var across = CGPoint(x: -along.y, y: along.x)
        let middle = CGPoint(x: (first.x + last.x) / 2, y: (first.y + last.y) / 2)
        let depths = contour.map { ($0.x - middle.x) * across.x + ($0.y - middle.y) * across.y }
        guard let chin = depths.max(by: { abs($0) < abs($1) }), chin != 0 else { return [] }
        if chin > 0 { across = CGPoint(x: -across.x, y: -across.y) }
        let a = length / 2, b = foreheadRatio * abs(chin)
        return (1...foreheadPoints).map { k in
            // θ = 0 is the contour's last point, θ = π its first.
            let theta = CGFloat.pi * CGFloat(k) / CGFloat(foreheadPoints + 1)
            return CGPoint(
                x: middle.x + a * cos(theta) * along.x + b * sin(theta) * across.x,
                y: middle.y + a * cos(theta) * along.y + b * sin(theta) * across.y)
        }
    }

    /// The iris as `irisPoints` points on its circle.
    static func circle(_ iris: RedEye.Eye) -> [CGPoint] {
        (0..<irisPoints).map { k in
            let angle = 2 * CGFloat.pi * CGFloat(k) / CGFloat(irisPoints)
            return CGPoint(x: iris.center.x + iris.radius * cos(angle),
                           y: iris.center.y + iris.radius * sin(angle))
        }
    }
}
