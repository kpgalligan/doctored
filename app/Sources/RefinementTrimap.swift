import Foundation

// Rasterization stays in the cancellable core; no alpha-to-trimap policy is inferred by the model.
// Publication validates both this local session and the captured document. Select and Mask's Refine
// Edge brush (`RefineEdgeBrush.swift`) rasterizes its strokes through this replay: an add stroke is
// the foreground label, a subtract stroke the background label, over an all-unknown base.
// `refine_selection` rejects unknown fields, `trimap` and `trimap_strokes` among them.

/// Categorical brush data: ordered strokes that overwrite one of three labels.
/// No alpha-to-trimap inference occurs here: callers supply explicit categories.
enum RefinementTrimapLabel: String, Codable {
    case background, unknown, foreground
    var byte: UInt8 {
        switch self { case .background: return 0; case .unknown: return 128; case .foreground: return 255 }
    }
}

private struct TrimapKey: CodingKey {
    let stringValue: String
    var intValue: Int? { nil }
    init?(stringValue: String) { self.stringValue = stringValue }
    init?(intValue: Int) { return nil }
}
private func trimapFields(_ decoder: Decoder, _ expected: Set<String>) throws {
    let keys = try decoder.container(keyedBy: TrimapKey.self).allKeys.map(\.stringValue)
    guard Set(keys) == expected else { throw DoctoredCoreError(message: "Trimap record has missing or unknown fields.") }
}
private struct TrimapPoint: Codable {
    let value: CGPoint
    init(_ point: CGPoint) { value = point }
    init(from decoder: Decoder) throws {
        var values = try decoder.unkeyedContainer()
        let x = try values.decode(Double.self), y = try values.decode(Double.self)
        guard values.isAtEnd else { throw DoctoredCoreError(message: "Trimap points require exactly two numbers.") }
        value = CGPoint(x: x, y: y)
    }
    func encode(to encoder: Encoder) throws {
        var values = encoder.unkeyedContainer()
        try values.encode(Double(value.x)); try values.encode(Double(value.y))
    }
}

struct RefinementTrimapStroke: Equatable, Codable {
    let label: RefinementTrimapLabel
    let radius: Double
    let points: [CGPoint]
    static let maxPoints = 8192
    private enum CodingKeys: String, CodingKey { case label, radius, points }

    init(label: RefinementTrimapLabel, radius: Double, points: [CGPoint]) throws {
        guard radius.isFinite, (0.5...512).contains(radius), !points.isEmpty,
              points.count <= Self.maxPoints, points.allSatisfy(Self.validPoint) else {
            throw DoctoredCoreError(message: "Trimap strokes require a 0.5–512 pixel radius and 1–8192 finite points within ±100000.")
        }
        self.label = label; self.radius = radius; self.points = points
    }
    static func validPoint(_ point: CGPoint) -> Bool {
        point.x.isFinite && point.y.isFinite && abs(point.x) <= 100_000 && abs(point.y) <= 100_000
    }
    init(from decoder: Decoder) throws {
        try trimapFields(decoder, ["label", "radius", "points"])
        let values = try decoder.container(keyedBy: CodingKeys.self)
        try self.init(label: values.decode(RefinementTrimapLabel.self, forKey: .label),
                      radius: values.decode(Double.self, forKey: .radius),
                      points: values.decode([TrimapPoint].self, forKey: .points).map(\.value))
    }
    func encode(to encoder: Encoder) throws {
        var values = encoder.container(keyedBy: CodingKeys.self)
        try values.encode(label, forKey: .label); try values.encode(radius, forKey: .radius)
        try values.encode(points.map(TrimapPoint.init), forKey: .points)
    }
}

/// Stable replay envelope. Ordered strokes overwrite categorical labels at
/// pixel centers inside a round-cap polyline; all rasterization lives in Rust.
struct RefinementTrimap: Equatable, Codable {
    static let maxStrokes = 256, maxPoints = 65_536, maxJSONBytes = 4 * 1024 * 1024
    let width: Int, height: Int
    let strokes: [RefinementTrimapStroke]
    private enum CodingKeys: String, CodingKey { case version, width, height, strokes }

    init(width: Int, height: Int, strokes: [RefinementTrimapStroke] = []) throws {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              strokes.count <= Self.maxStrokes,
              strokes.reduce(0, { $0 + $1.points.count }) <= Self.maxPoints else {
            throw DoctoredCoreError(message: "Trimap exceeds the canvas, 256 stroke, or 65536 point limit.")
        }
        self.width = width; self.height = height; self.strokes = strokes
    }
    init(data: Data) throws {
        guard data.count <= Self.maxJSONBytes else { throw DoctoredCoreError(message: "Trimap replay exceeds 4 MiB.") }
        self = try JSONDecoder().decode(Self.self, from: data)
    }
    init(from decoder: Decoder) throws {
        try trimapFields(decoder, ["version", "width", "height", "strokes"])
        let values = try decoder.container(keyedBy: CodingKeys.self)
        guard try values.decode(Int.self, forKey: .version) == 1 else {
            throw DoctoredCoreError(message: "Unsupported trimap replay version.")
        }
        try self.init(width: values.decode(Int.self, forKey: .width), height: values.decode(Int.self, forKey: .height),
                      strokes: values.decode([RefinementTrimapStroke].self, forKey: .strokes))
    }
    func encode(to encoder: Encoder) throws {
        var values = encoder.container(keyedBy: CodingKeys.self)
        try values.encode(1, forKey: .version); try values.encode(width, forKey: .width)
        try values.encode(height, forKey: .height); try values.encode(strokes, forKey: .strokes)
    }
    func encodedData() throws -> Data {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
        let data = try encoder.encode(self)
        guard data.count <= Self.maxJSONBytes else { throw DoctoredCoreError(message: "Trimap replay exceeds 4 MiB.") }
        return data
    }
    /// Core temporary storage excludes caller input and output (one byte/pixel
    /// each). Admission must include both and any separate matting source/work.
    func coreScratchBytes() throws -> UInt64 {
        UInt64(width * height) + UInt64(try encodedData().count) * 8 + 16 * 1024 * 1024
    }
}

/// Local stroke history retains only bounded point geometry, never full canvas
/// planes or document handles. Capture identity/revision still comes from
/// SelectionCapture; this token additionally invalidates stale local previews.
final class RefinementTrimapSession {
    struct Snapshot {
        let sessionID: UUID
        let revision: UInt64
        let trimap: RefinementTrimap
    }
    private let id = UUID()
    private let width: Int, height: Int
    private var strokes: [RefinementTrimapStroke] = []
    private var cursor = 0
    private var draft: (label: RefinementTrimapLabel, radius: Double, points: [CGPoint])?
    private(set) var revision: UInt64 = 0
    var isDrawing: Bool { draft != nil }
    var canUndo: Bool { draft == nil && cursor > 0 }
    var canRedo: Bool { draft == nil && cursor < strokes.count }
    var retainedPointCount: Int { strokes.reduce(0) { $0 + $1.points.count } + (draft?.points.count ?? 0) }

    init(width: Int, height: Int) throws {
        _ = try RefinementTrimap(width: width, height: height)
        self.width = width; self.height = height
    }
    func begin(label: RefinementTrimapLabel, radius: Double, at point: CGPoint) throws {
        guard draft == nil else { throw DoctoredCoreError(message: "Finish or cancel the current trimap stroke first.") }
        _ = try RefinementTrimapStroke(label: label, radius: radius, points: [point])
        draft = (label, radius, [point]); revision &+= 1
    }
    func append(_ point: CGPoint) throws {
        guard let count = draft?.points.count, count < RefinementTrimapStroke.maxPoints,
              RefinementTrimapStroke.validPoint(point) else {
            throw DoctoredCoreError(message: "Trimap draft is absent or exceeds its point limit.")
        }
        draft?.points.append(point); revision &+= 1
    }
    func finish() throws {
        guard let value = draft else { throw DoctoredCoreError(message: "No trimap stroke is in progress.") }
        let stroke = try RefinementTrimapStroke(label: value.label, radius: value.radius, points: value.points)
        let next = Array(strokes.prefix(cursor)) + [stroke]
        _ = try RefinementTrimap(width: width, height: height, strokes: next)
        strokes = next; cursor = strokes.count; draft = nil; revision &+= 1
    }
    func cancel() { if draft != nil { draft = nil; revision &+= 1 } }
    @discardableResult func undo() -> Bool {
        guard canUndo else { return false }; cursor -= 1; revision &+= 1; return true
    }
    @discardableResult func redo() -> Bool {
        guard canRedo else { return false }; cursor += 1; revision &+= 1; return true
    }
    func snapshot() throws -> Snapshot {
        guard draft == nil else { throw DoctoredCoreError(message: "Finish or cancel the draft before taking a trimap snapshot.") }
        return Snapshot(sessionID: id, revision: revision,
                        trimap: try RefinementTrimap(width: width, height: height, strokes: Array(strokes.prefix(cursor))))
    }
    func isCurrent(_ snapshot: Snapshot) -> Bool {
        draft == nil && snapshot.sessionID == id && snapshot.revision == revision
    }
}
