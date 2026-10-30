import AppKit

// Edit ▸ Perspective Warp: the quads over shared vertices, the straighten entries, the
// request JSON the core takes (`doc_perspective_warp.rs`'s `perspective_warp`,
// `selected_pixels.rs`'s `perspective_selected`) and the recorded Action step. The session is
// `PerspectiveWarpSession`, the commit `MeshWarpWork(kind: .perspective)`, the MCP mirror
// `AgentServer+PerspectiveWarp.swift`.

/// The session's two modes: drawing planes, then warping them.
enum PerspectiveWarpMode: Int {
    case layout, warp
}

/// A straighten entry's direction as the core names it.
enum PerspectiveDirection: Equatable {
    case auto, nearHorizontal, nearVertical, horizontal, vertical
    case degrees(Double)

    var json: Any {
        switch self {
        case .auto: return "auto"
        case .nearHorizontal: return "near_horizontal"
        case .nearVertical: return "near_vertical"
        case .horizontal: return "horizontal"
        case .vertical: return "vertical"
        case .degrees(let value): return value
        }
    }

    init?(json: Any?) {
        if let number = json as? NSNumber, !(json is Bool) {
            self = .degrees(number.doubleValue)
            return
        }
        switch json as? String {
        case "auto": self = .auto
        case "near_horizontal": self = .nearHorizontal
        case "near_vertical": self = .nearVertical
        case "horizontal": self = .horizontal
        case "vertical": self = .vertical
        default: return nil
        }
    }
}

/// One straighten entry: edge `edge` (corner `edge` to corner `edge + 1`) of
/// quad `quad`.
struct PerspectiveStraighten: Equatable {
    var quad: Int
    var edge: Int
    var direction: PerspectiveDirection

    var json: [String: Any] { ["quad": quad, "edge": edge, "direction": direction.json] }
}

/// A shared corner: where it stands in the layer and where it goes.
struct PerspectiveVertex: Equatable {
    var from: CGPoint
    var to: CGPoint
}

/// The quads over shared vertices, as the session edits them and as the core
/// resolves them (`dr_perspective_warp_layout`): joins merged, straighten
/// directions resolved and applied, shared edges found.
struct PerspectiveLayout: Equatable {
    var vertices: [PerspectiveVertex] = []
    /// Four vertex indices per quad, corners in the order drawn.
    var quads: [[Int]] = []
    var straighten: [PerspectiveStraighten] = []
    /// Vertex pairs two or more quads hold as a side.
    var sharedEdges: [[Int]] = []
    var identity = true

    /// The request's quads, each `{"from":[x0,y0,…],"to":[…]}`.
    var quadsJSON: [[String: Any]] {
        quads.map { ids in
            [
                "from": ids.flatMap { [Double(vertices[$0].from.x), Double(vertices[$0].from.y)] },
                "to": ids.flatMap { [Double(vertices[$0].to.x), Double(vertices[$0].to.y)] },
            ]
        }
    }

    /// The layout call's fields: the quads, the straighten entries and the
    /// dragged corner's `to` as the anchor.
    func fields(anchor: Int?) -> [String: Any] {
        var out: [String: Any] = ["quads": quadsJSON, "straighten": straighten.map(\.json)]
        if let anchor, vertices.indices.contains(anchor) {
            out["anchor"] = [Double(vertices[anchor].to.x), Double(vertices[anchor].to.y)]
        }
        return out
    }

    /// The core's description (`Layout::describe`).
    init(described value: [String: Any]) throws {
        func point(_ any: Any?) -> CGPoint? {
            guard let pair = any as? [Double], pair.count == 2 else { return nil }
            return CGPoint(x: pair[0], y: pair[1])
        }
        let list = value["vertices"] as? [[String: Any]] ?? []
        vertices = try list.map { item in
            guard let from = point(item["from"]), let to = point(item["to"]) else {
                throw DoctoredCoreError(message: "The Perspective Warp layout is malformed.")
            }
            return PerspectiveVertex(from: from, to: to)
        }
        quads = (value["quads"] as? [[Int]] ?? []).filter { ids in
            ids.count == 4 && ids.allSatisfy { list.indices.contains($0) }
        }
        sharedEdges = (value["shared_edges"] as? [[Int]] ?? []).filter { $0.count == 2 }
        straighten = (value["straighten"] as? [[String: Any]] ?? []).compactMap { item in
            guard let quad = item["quad"] as? Int, let edge = item["edge"] as? Int,
                  let direction = PerspectiveDirection(json: item["direction"]) else { return nil }
            return PerspectiveStraighten(quad: quad, edge: edge, direction: direction)
        }
        identity = value["identity"] as? Bool ?? true
    }

    init() {}

    /// Quad `quad`'s corners at rest (`warped` false) or warped.
    func corners(_ quad: Int, warped: Bool) -> [CGPoint] {
        quads[quad].map { warped ? vertices[$0].to : vertices[$0].from }
    }

    /// Edge `edge` of quad `quad` as a vertex pair.
    func edge(_ quad: Int, _ edge: Int) -> (Int, Int) {
        (quads[quad][edge], quads[quad][(edge + 1) % 4])
    }

    /// Drops vertices no quad holds and renumbers the rest.
    mutating func compact() {
        var used = [Int](repeating: -1, count: vertices.count)
        var kept: [PerspectiveVertex] = []
        for ids in quads {
            for v in ids where used[v] < 0 {
                used[v] = kept.count
                kept.append(vertices[v])
            }
        }
        quads = quads.map { $0.map { used[$0] } }
        sharedEdges = sharedEdges.compactMap { pair in
            let mapped = pair.map { used[$0] }
            return mapped.allSatisfy { $0 >= 0 } ? mapped : nil
        }
        vertices = kept
    }
}

enum PerspectiveWarp {
    static let actionName = "Perspective Warp"
    static let maxQuads = 64

    /// Compact JSON with sorted keys, within the core's 64 KiB.
    static func json(_ fields: [String: Any]) throws -> String {
        guard JSONSerialization.isValidJSONObject(fields),
              let data = try? JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys]),
              data.count <= 64 * 1024, let text = String(data: data, encoding: .utf8) else {
            throw DoctoredCoreError(message: "The Perspective Warp request must be finite JSON within 64 KiB.")
        }
        return text
    }

    /// The layer form's fields (`perspective_warp`) or, for the float, the
    /// selection's (`perspective_selected`, `bounds` the selection's rect).
    /// The quads are resolved ones: their straightening is already in `to`.
    static func fields(layer: Int, quads: [[String: Any]], float: Bool,
                       bounds: CGRect?) -> [String: Any] {
        var out: [String: Any] = [
            "op": float ? "perspective_selected" : "perspective_warp", "layer": layer,
            "quads": quads,
        ]
        if float, let bounds { out["bounds"] = MoveSelectedPixels.requestBounds(bounds) }
        return out
    }

    /// The recorded step (Actions): the resolved quads (their source corners
    /// and warped pins) on the active layer, as `perspective_warp` takes them.
    static func step(quads: [[String: Any]]) -> [ActionStep] {
        .step("perspective_warp", ["layer": ActionArgs.activeLayer, "quads": quads],
              note: "Edit ▸ Perspective Warp")
    }

    /// `perspective_warp`'s arguments for the core: the quads and the
    /// optional straighten entries as lists of objects; their contents are
    /// the core's to refuse by name.
    static func agentFields(_ a: [String: Any]) throws
        -> (quads: [[String: Any]], straighten: [[String: Any]]?)
    {
        guard let list = a["quads"] as? [Any] else {
            throw DoctoredCoreError(message: a["quads"] == nil ? "Perspective Warp needs at least one quad."
                : "perspective_warp quads must be a list of {from, to} objects.")
        }
        let quads = try list.map { item -> [String: Any] in
            guard let quad = item as? [String: Any] else {
                throw DoctoredCoreError(message: "perspective_warp quads must be a list of {from, to} objects.")
            }
            return quad
        }
        guard let raw = a["straighten"], !(raw is NSNull) else { return (quads, nil) }
        guard let entries = raw as? [Any] else {
            throw DoctoredCoreError(message: "perspective_warp straighten must be a list of {quad, edge, direction} objects.")
        }
        let straighten = try entries.map { item -> [String: Any] in
            guard let entry = item as? [String: Any] else {
                throw DoctoredCoreError(message: "perspective_warp straighten must be a list of {quad, edge, direction} objects.")
            }
            return entry
        }
        return (quads, straighten)
    }
}
