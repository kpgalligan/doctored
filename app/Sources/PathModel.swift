import AppKit

// Paths: the Swift value types of a document's Bézier paths. The core owns the list, its
// rules and its caps (`core/src/path_geometry.rs`, `doc_path.rs`); these types are what the UI and
// the agent read and write, and their JSON is the core's own — the shape JSON and the path-list
// JSON of the header's "Paths" section. Every coordinate is an ABSOLUTE canvas coordinate, y down
// from the top-left, handles included. `DoctoredCore+Paths.swift` holds the `dr_*` calls.

/// One anchor: its point and the two handles of the segments that meet at it.
/// A handle sitting on its point makes that side of the anchor straight.
struct PathAnchor: Equatable {
    var point: CGPoint
    /// The second control point of the segment ARRIVING at this anchor.
    var handleIn: CGPoint
    /// The first control point of the segment LEAVING this anchor.
    var handleOut: CGPoint
    /// An editing hint — the Pen keeps a smooth anchor's handles collinear.
    /// It changes no geometry.
    var smooth: Bool

    init(point: CGPoint, handleIn: CGPoint, handleOut: CGPoint, smooth: Bool) {
        self.point = point
        self.handleIn = handleIn
        self.handleOut = handleOut
        self.smooth = smooth
    }

    /// A corner: both handles on the point.
    static func corner(_ point: CGPoint) -> PathAnchor {
        PathAnchor(point: point, handleIn: point, handleOut: point, smooth: false)
    }
}

/// One contour. An open subpath is closed by a straight line for FILL only.
struct PathSubpath: Equatable {
    var anchors: [PathAnchor]
    var closed: Bool
}

/// How overlapping contours combine when a shape is filled; the raw value is
/// the JSON spelling.
enum PathFillRule: String {
    case nonZero = "nonzero"
    case evenOdd = "evenodd"
}

/// A whole path: every contour and the fill rule. An empty shape is valid and
/// covers nothing.
struct PathShape: Equatable {
    var subpaths: [PathSubpath] = []
    var fillRule: PathFillRule = .nonZero

    var isEmpty: Bool { subpaths.isEmpty }
    var anchorCount: Int { subpaths.reduce(0) { $0 + $1.anchors.count } }
}

/// The document's one unsaved scratch path, or a named one.
enum PathKind: String {
    case work
    case saved
}

/// One entry of a document's path list. `id` is the core's process-unique
/// identity, stable across edits, geometry ops, depth conversion and undo;
/// 0 marks an entry the core has not seen yet and mints one for. `clipping`
/// is set on the one saved path that is the document's clipping path.
struct PathEntry: Equatable {
    var id: UInt64
    var name: String
    var kind: PathKind
    var shape: PathShape
    var clipping: PathClipping? = nil
}

/// The clipping-path designation (`core/src/doc_path.rs`, `ClippingPath`): the path a file's
/// resource 2999 names. `tail` is the hex of the bytes that resource held after the name, carried
/// verbatim so a file's own bytes are written back; empty for a designation made here.
struct PathClipping: Equatable {
    var tail: String = ""
}

/// A document's path list in the core's order: the work path first when
/// there is one, then the saved paths.
struct PathSet: Equatable {
    /// The name the work path always has; no saved path may take it.
    static let workPathName = "Work Path"

    var entries: [PathEntry] = []

    var workPath: PathEntry? { entries.first { $0.kind == .work } }

    func entry(id: UInt64) -> PathEntry? {
        id == 0 ? nil : entries.first { $0.id == id }
    }

    /// The entry called `name`; the work path answers to `workPathName`.
    func entry(named name: String) -> PathEntry? {
        entries.first { $0.name == name }
    }

    /// The list with the entry at `index` as the one clipping path (nil
    /// clears the designation). A path that already is keeps its tail.
    func settingClippingPath(_ index: Int?) -> PathSet {
        var next = self
        for i in next.entries.indices where i != index {
            next.entries[i].clipping = nil
        }
        if let index, next.entries.indices.contains(index), next.entries[index].clipping == nil {
            next.entries[index].clipping = PathClipping()
        }
        return next
    }

    /// The list with the work path replaced by `shape` (keeping its identity)
    /// or created from it, first in the list as the core stores it.
    func settingWorkPath(_ shape: PathShape) -> PathSet {
        var next = self
        if let index = entries.firstIndex(where: { $0.kind == .work }) {
            next.entries[index].shape = shape
        } else {
            next.entries.insert(
                PathEntry(id: 0, name: Self.workPathName, kind: .work, shape: shape), at: 0)
        }
        return next
    }
}

/// What the Pen and the Paths panel are editing: a path of the list by its
/// id, or the vector mask of a layer by the layer's id. View state.
enum PathTarget: Equatable {
    case path(UInt64)
    case vectorMask(layerID: UInt64)
}

/// A layer's vector mask as the core reports it: the shape, its two flags,
/// and whether a raster mask is kept beside it.
struct VectorMaskInfo: Equatable {
    var shape: PathShape
    var enabled: Bool
    var linked: Bool
    var hasRaster: Bool
    var rasterEnabled: Bool
    /// The raster mask's OWN placement and properties, nil without one. The
    /// layer's mask getters describe the baked mask, not this.
    var rasterProperties: LayerMaskProperties?
}

/// What lies under a point of a shape (`RasterPathCore.hit`). `index` is the
/// anchor's index in its subpath, or the segment's: segment `i` runs from
/// anchor `i` to the next, and a closed subpath's last one returns to its
/// first anchor.
enum PathHit: Equatable {
    case none
    case anchor(subpath: Int, index: Int)
    case handleIn(subpath: Int, index: Int)
    case handleOut(subpath: Int, index: Int)
    /// `t` is the parameter of the segment's nearest point, what
    /// `RasterPathCore.insertingAnchor` takes.
    case segment(subpath: Int, index: Int, t: Double)
}

/// A path JSON value that is not the shape it should be. The message is a
/// whole sentence, fit for a tool error.
struct PathFormatError: Error, LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

// MARK: - JSON

extension PathShape {
    /// The shape out of the shape JSON. LENIENT about form, as the core's own
    /// reader is: an anchor is `{"x","y","in","out","smooth"}` with `in`/`out`
    /// defaulting to the point and `smooth` to false, or the pair `[x, y]`, a
    /// corner; `closed` defaults to false and `fill_rule` to nonzero. The caps
    /// and the coordinate wall are the core's to enforce when the shape is
    /// used, with its own sentences.
    init(json: Any?) throws {
        guard let object = json as? [String: Any] else {
            throw PathFormatError(message: "A path shape must be an object with a subpaths array.")
        }
        try self.init(subpathsJSON: object["subpaths"], fillRuleJSON: object["fill_rule"])
    }

    /// The same, from the two values apart — an MCP call's `subpaths` and
    /// `fill_rule` arguments.
    init(subpathsJSON subpaths: Any?, fillRuleJSON fillRule: Any?) throws {
        switch fillRule {
        case nil, is NSNull: self.fillRule = .nonZero
        case let name as String:
            guard let rule = PathFillRule(rawValue: name) else {
                throw PathFormatError(message: "fill_rule must be \"nonzero\" or \"evenodd\".")
            }
            self.fillRule = rule
        default:
            throw PathFormatError(message: "fill_rule must be \"nonzero\" or \"evenodd\".")
        }
        guard let list = subpaths as? [Any] else {
            throw PathFormatError(
                message: "subpaths must be an array of {\"closed\": bool, \"anchors\": […]} objects.")
        }
        self.subpaths = try list.enumerated().map { index, entry in
            guard let object = entry as? [String: Any], let anchors = object["anchors"] as? [Any]
            else {
                throw PathFormatError(message: "subpath \(index) needs an anchors array.")
            }
            guard !anchors.isEmpty else {
                throw PathFormatError(message: "subpath \(index) needs at least one anchor.")
            }
            let closed: Bool
            switch object["closed"] {
            case nil, is NSNull: closed = false
            case let flag as Bool: closed = flag
            default: throw PathFormatError(message: "subpath \(index): closed must be a boolean.")
            }
            return PathSubpath(
                anchors: try anchors.enumerated().map { position, anchor in
                    try Self.anchor(anchor, "subpath \(index), anchor \(position)")
                },
                closed: closed)
        }
    }

    private static func number(_ value: Any?) -> Double? {
        // A JSON boolean bridges to NSNumber too; it is not a coordinate.
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else {
            return nil
        }
        return number.doubleValue
    }

    private static func pair(_ value: Any?) -> CGPoint? {
        guard let list = value as? [Any], list.count == 2,
              let x = number(list[0]), let y = number(list[1])
        else { return nil }
        return CGPoint(x: x, y: y)
    }

    private static func anchor(_ value: Any, _ place: String) throws -> PathAnchor {
        if value is [Any] {
            guard let point = pair(value) else {
                throw PathFormatError(message: "\(place): a corner anchor is an [x, y] pair of numbers.")
            }
            return .corner(point)
        }
        guard let object = value as? [String: Any],
              let x = number(object["x"]), let y = number(object["y"])
        else {
            throw PathFormatError(
                message: "\(place) must be [x, y] or {\"x\", \"y\", \"in\": [x, y], "
                    + "\"out\": [x, y], \"smooth\": bool}.")
        }
        let point = CGPoint(x: x, y: y)
        func handle(_ key: String) throws -> CGPoint {
            guard let raw = object[key], !(raw is NSNull) else { return point }
            guard let parsed = pair(raw) else {
                throw PathFormatError(message: "\(place): \(key) must be an [x, y] pair of numbers.")
            }
            return parsed
        }
        let smooth: Bool
        switch object["smooth"] {
        case nil, is NSNull: smooth = false
        case let flag as Bool: smooth = flag
        default: throw PathFormatError(message: "\(place): smooth must be a boolean.")
        }
        return PathAnchor(
            point: point, handleIn: try handle("in"), handleOut: try handle("out"), smooth: smooth)
    }

    /// A coordinate as a JSON number spelled the shortest way that reads back
    /// to the same Double. `JSONSerialization` writes a Double with 17
    /// significant digits (250.1 → 250.10000000000002), which would make a
    /// stored four-decimal coordinate look like a different number to a model
    /// reading it back. A non-finite value has no JSON form and becomes 0 —
    /// the core refuses one long before a shape that holds it is stored.
    static func jsonNumber(_ value: CGFloat) -> NSNumber {
        guard value.isFinite else { return 0 }
        return NSDecimalNumber(string: "\(Double(value))")
    }

    private static func jsonPair(_ point: CGPoint) -> [NSNumber] {
        [jsonNumber(point.x), jsonNumber(point.y)]
    }

    /// The `subpaths` array of the shape JSON, every key written.
    var subpathsJSON: [[String: Any]] {
        subpaths.map { subpath in
            [
                "closed": subpath.closed,
                "anchors": subpath.anchors.map { anchor -> [String: Any] in
                    [
                        "x": Self.jsonNumber(anchor.point.x), "y": Self.jsonNumber(anchor.point.y),
                        "in": Self.jsonPair(anchor.handleIn),
                        "out": Self.jsonPair(anchor.handleOut),
                        "smooth": anchor.smooth,
                    ]
                },
            ]
        }
    }

    /// The shape JSON as an object.
    var jsonObject: [String: Any] {
        ["fill_rule": fillRule.rawValue, "subpaths": subpathsJSON]
    }

    /// Whether every coordinate is a finite number — what the JSON can carry.
    var isFinite: Bool {
        subpaths.allSatisfy { subpath in
            subpath.anchors.allSatisfy { anchor in
                [anchor.point, anchor.handleIn, anchor.handleOut].allSatisfy {
                    $0.x.isFinite && $0.y.isFinite
                }
            }
        }
    }
}

extension PathSet {
    /// The list out of the path-list JSON the core writes
    /// (`{"paths":[{"id","name","kind", …shape}]}`).
    init(json: Any?) throws {
        guard let object = json as? [String: Any], let list = object["paths"] as? [Any] else {
            throw PathFormatError(message: "A path list must be an object with a paths array.")
        }
        entries = try list.map { entry in
            guard let object = entry as? [String: Any],
                  let kind = (object["kind"] as? String).flatMap(PathKind.init(rawValue:))
            else { throw PathFormatError(message: "A path entry needs a kind.") }
            let clipping = (object["clipping"] as? [String: Any]).map {
                PathClipping(tail: $0["tail"] as? String ?? "")
            }
            return PathEntry(
                id: (object["id"] as? NSNumber)?.uint64Value ?? 0,
                name: object["name"] as? String ?? (kind == .work ? Self.workPathName : ""),
                kind: kind, shape: try PathShape(json: object), clipping: clipping)
        }
    }

    /// The path-list JSON as an object. An entry whose id is 0 is written
    /// without one, which is how the core is told to mint it.
    var jsonObject: [String: Any] {
        [
            "paths": entries.map { entry -> [String: Any] in
                var object = entry.shape.jsonObject
                object["name"] = entry.name
                object["kind"] = entry.kind.rawValue
                if entry.id != 0 { object["id"] = NSNumber(value: entry.id) }
                if let clipping = entry.clipping { object["clipping"] = ["tail": clipping.tail] }
                return object
            },
        ]
    }
}
