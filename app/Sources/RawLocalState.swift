import Foundation

/// A native recipe and its independently shared coverage resources. No pixels
/// are copied through JSON or inferred from the displayed development.
struct RawLocalState {
    let descriptor: RawSourceDescriptor
    let width: Int
    let height: Int
    let nodes: [RawLocalNode]
    let masks: [RasterRawLocalMask]
}

extension RawLocalNode {
    mutating func update(_ object: [String: Any]) throws {
        if let raw = object["name"] {
            guard let value = raw as? String else { throw DoctoredCoreError(message: "RAW local name must be text.") }
            name = value
        }
        if let value = try RawLocalJSON.flag(object, "enabled") { enabled = value }
        if let value = try RawLocalJSON.number(object, "density", range: 0...1) { density = value }
        if let value = try RawLocalJSON.number(object, "feather", range: 0...256) { feather = value }
        if let value = try RawLocalJSON.number(object, "exposure_offset", range: -4...4) { exposureOffset = value }
        if let raw = object["white_balance"] {
            guard let value = raw as? String, let mode = RawLocalWhiteBalance(rawValue: value) else {
                throw DoctoredCoreError(message: "Local white_balance must be inherit, as_shot or custom.")
            }
            whiteBalance = mode
            if mode != .custom { temperature = nil; tint = nil }
        }
        if let value = try RawLocalJSON.number(object, "temperature", range: RawDevelopRange.temperature) {
            temperature = Double(Float(value))
        }
        if let value = try RawLocalJSON.number(object, "tint", range: RawDevelopRange.tint) { tint = Double(Float(value)) }
        // `range` replaces the node's whole range; null removes it.
        if let raw = object["range"] {
            if raw is NSNull {
                range = nil
            } else if let value = raw as? [String: Any] {
                range = try RawLocalRange(json: value)
            } else {
                throw DoctoredCoreError(message: RawLocalRange.refusal)
            }
        }
        // `tone` replaces the node's whole tone; null or every control at 0 removes it.
        if let raw = object["tone"] {
            if raw is NSNull {
                tone = nil
            } else if let value = raw as? [String: Any] {
                let parsed = try RawLocalTone(json: value)
                tone = parsed.isNeutral ? nil : parsed
            } else {
                throw DoctoredCoreError(message: RawLocalTone.refusal)
            }
        }
        try validate()
    }
}

enum RawLocalOperations {
    static let controlKeys: Set<String> = ["name", "enabled", "density", "feather",
        "exposure_offset", "white_balance", "temperature", "tint", "range", "tone"]

    /// Own an immutable, bounded JSON copy before queueing work. Operation/node
    /// identities are explicit so recorded gestures replay on the same targets.
    /// `provided` is how many caller-supplied masks a `{"op": "coverage", "index": k}` create may
    /// name (Synchronize RAW Settings' Masking); 0 everywhere else, so it is refused there.
    static func capture(_ operations: [[String: Any]], provided: Int = 0) throws -> [[String: Any]] {
        guard operations.count <= RawLocalWork.maximumOperations else {
            throw DoctoredCoreError(message: "The RAW local operation list exceeds its limit or contains invalid values.")
        }
        // Bound traversal and copied object storage before JSONSerialization
        // allocates a second representation of an untrusted gesture request.
        var values = 0, textBytes = 0
        func preflight(_ value: Any, depth: Int) throws {
            values += 1
            guard depth <= 12, values <= 65_536 else {
                throw DoctoredCoreError(message: "RAW local operations exceed their nested value bound.")
            }
            if let object = value as? [String: Any] {
                guard object.count <= 32 else { throw DoctoredCoreError(message: "A RAW local object has too many fields.") }
                for (key, child) in object {
                    textBytes += key.utf8.count
                    try preflight(child, depth: depth + 1)
                }
            } else if let array = value as? [Any] {
                guard array.count <= RawLocalWork.maximumDabs else { throw DoctoredCoreError(message: "A RAW local array exceeds its bound.") }
                for child in array { try preflight(child, depth: depth + 1) }
            } else if let text = value as? String {
                textBytes += text.utf8.count
            } else if let number = value as? NSNumber {
                guard number.doubleValue.isFinite else { throw DoctoredCoreError(message: "RAW local numbers must be finite.") }
            } else if !(value is NSNull) {
                throw DoctoredCoreError(message: "RAW local operations require JSON values.")
            }
            guard textBytes <= RawLocalWork.maximumOperationBytes else {
                throw DoctoredCoreError(message: "RAW local text exceeds the operation payload bound.")
            }
        }
        try preflight(operations, depth: 0)
        guard JSONSerialization.isValidJSONObject(operations) else { throw DoctoredCoreError(message: "Invalid RAW local JSON.") }
        let data = try JSONSerialization.data(withJSONObject: operations, options: [.sortedKeys])
        guard data.count <= RawLocalWork.maximumOperationBytes,
              let owned = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            throw DoctoredCoreError(message: "RAW local operations exceed 256 KiB. Apply this edit before continuing.")
        }
        var dabs = 0
        for object in owned {
            guard let op = object["op"] as? String, let id = object["id"] as? String,
                  RawLocalJSON.validID(id) else { throw DoctoredCoreError(message: "Every RAW local operation needs op and a stable id.") }
            switch op {
            case "create":
                try RawLocalJSON.keys(object, controlKeys.union(["op", "id", "mask"]))
                // subject and people are generated here (`RawLocalGenerated`); the core never
                // sees them.
                guard let mask = object["mask"] as? [String: Any],
                      try RawLocalGenerated.kind(of: mask) != nil
                          || ["empty", "full", "radial", "linear"].contains(mask["op"] as? String ?? "")
                          || coverageIndex(mask, provided: provided) != nil else {
                    throw DoctoredCoreError(message: "A RAW local requires an empty, full, radial, linear, subject or people native mask.")
                }
            case "update": try RawLocalJSON.keys(object, controlKeys.union(["op", "id"]))
            case "mask":
                try RawLocalJSON.keys(object, ["op", "id", "command"])
                guard let command = object["command"] as? [String: Any] else {
                    throw DoctoredCoreError(message: "A RAW mask edit requires a native mask command.")
                }
                _ = try RawLocalGenerated.kind(of: command)
                if command["op"] as? String == "brush" {
                    guard let values = command["dabs"] as? [[String: Any]] else {
                        throw DoctoredCoreError(message: "RAW brush dabs must be an array.")
                    }
                    dabs += values.count
                }
            case "remap":
                try RawLocalJSON.keys(object, ["op", "id", "map"])
                guard let map = object["map"] as? [String: Any] else {
                    throw DoctoredCoreError(message: "RAW local remap requires a source map.")
                }
                let parsed = try JSONDecoder().decode(RawSourceMap.self,
                    from: JSONSerialization.data(withJSONObject: map))
                guard parsed.kind != .lens else {
                    throw DoctoredCoreError(message: "A canonical local remap requires EXIF or affine coordinates.")
                }
            case "remove": try RawLocalJSON.keys(object, ["op", "id"])
            case "reorder":
                try RawLocalJSON.keys(object, ["op", "id", "index"])
                let last = Double(RawEnhancementRecipe.maximumLocals - 1)
                guard let value = try RawLocalJSON.number(object, "index", range: 0...last), value.rounded() == value else {
                    throw DoctoredCoreError(message: "RAW local index must be an integer from 0 to 15.")
                }
            default: throw DoctoredCoreError(message: "Unknown RAW local operation \(op).")
            }
        }
        guard dabs <= RawLocalWork.maximumDabs else {
            throw DoctoredCoreError(message: "RAW local gestures exceed 4,096 total dabs. Apply this edit before continuing.")
        }
        return owned
    }

    /// The provided-mask index a `{"op": "coverage", "index": k}` create names, nil for any other
    /// mask or an index outside `0..<provided`.
    static func coverageIndex(_ mask: [String: Any], provided: Int) -> Int? {
        guard mask["op"] as? String == "coverage", Set(mask.keys) == ["op", "index"],
              let number = mask["index"] as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
              number.doubleValue.rounded() == number.doubleValue,
              number.doubleValue >= 0, number.doubleValue < Double(provided) else { return nil }
        return number.intValue
    }

    /// `generate` makes a subject or people mask from the base development of the request these
    /// operations belong to (`RawLocalGenerated`). What it throws, a cancellation aside, comes
    /// back as a `RawLocalGenerationRefusal` naming the operation's index in `operations`.
    /// `provided` holds the masks `coverage` creates name; each is resized to the state's frame in
    /// normalised coordinates. A create whose id a `remove` in the same list dropped, and whose
    /// new mask hashes the same as the dropped one, keeps the dropped object (Recompute's rule),
    /// so a replace that changes nothing publishes nothing.
    static func applying(_ operations: [[String: Any]], state: RawLocalState, control: RasterJobControl,
                         provided: [RasterRawLocalMask] = [],
                         generate: (RawLocalGeneratedKind) throws -> RasterRawLocalMask)
        throws -> ([RawLocalNode], [RasterRawLocalMask]) {
        var nodes = state.nodes, masks = state.masks
        var removed: [String: RasterRawLocalMask] = [:]
        func reusing(_ mask: RasterRawLocalMask, id: String) throws -> RasterRawLocalMask {
            guard let prior = removed[id], prior !== mask else { return mask }
            return try prior.sha256() == mask.sha256() ? prior : mask
        }
        func generated(_ kind: RawLocalGeneratedKind, operation: Int) throws -> RasterRawLocalMask {
            do {
                return try generate(kind)
            } catch let error as JobCancelled {
                throw error
            } catch {
                // A cancelled stage reports its own failure text; it is still a cancellation.
                guard !control.isCancelled else { throw JobCancelled() }
                let message = (error as? SubjectSelection.Failure)?.message ?? error.localizedDescription
                throw RawLocalGenerationRefusal(operation: operation, message: message)
            }
        }
        for (position, object) in operations.enumerated() {
            guard let op = object["op"] as? String, let id = object["id"] as? String else {
                throw DoctoredCoreError(message: "Invalid captured RAW local operation.")
            }
            if op == "create" {
                guard nodes.count < RawEnhancementRecipe.maximumLocals, !nodes.contains(where: { $0.id == id }),
                      let command = object["mask"] as? [String: Any] else {
                    throw DoctoredCoreError(message: "A RAW layer supports at most sixteen uniquely identified locals.")
                }
                var node = RawLocalNode(id: id, name: "Local \(nodes.count + 1)")
                try node.update(object)
                var mask: RasterRawLocalMask
                if let kind = try RawLocalGenerated.kind(of: command) {
                    mask = try generated(kind, operation: position)
                    node.generated = kind
                } else if let index = coverageIndex(command, provided: provided.count) {
                    mask = try control.withChildOperation { child in
                        try provided[index].resized(width: state.width, height: state.height, control: child)
                    }
                    node.generated = nil
                } else {
                    mask = try control.withChildOperation { child in
                        try RasterRawLocalMask.create(width: state.width, height: state.height,
                            command: command, control: child)
                    }
                }
                mask = try reusing(mask, id: id)
                nodes.append(node); masks.append(mask)
                continue
            }
            guard let index = nodes.firstIndex(where: { $0.id == id }) else {
                throw DoctoredCoreError(message: "RAW local \(id) is missing. No local edit was applied.")
            }
            switch op {
            case "update": try nodes[index].update(object)
            case "remove":
                nodes.remove(at: index)
                removed[id] = masks.remove(at: index)
            case "reorder":
                guard let destination = (object["index"] as? NSNumber)?.intValue,
                      nodes.indices.contains(destination) else { throw DoctoredCoreError(message: "RAW local destination index is unavailable.") }
                let node = nodes.remove(at: index), mask = masks.remove(at: index)
                nodes.insert(node, at: destination); masks.insert(mask, at: destination)
            case "mask":
                guard let command = object["command"] as? [String: Any] else { throw DoctoredCoreError(message: "Missing native mask command.") }
                if let kind = try RawLocalGenerated.kind(of: command) {
                    // Recompute: the whole plane is replaced. Coverage equal to the stored one
                    // keeps the stored object, so an unchanged recompute publishes nothing.
                    let fresh = try generated(kind, operation: position)
                    if try fresh.sha256() != masks[index].sha256() { masks[index] = fresh }
                    nodes[index].generated = kind
                } else {
                    masks[index] = try control.withChildOperation { child in
                        try masks[index].edited(command: command, control: child)
                    }
                    // A drawn shape replaces generated coverage; brush and invert edit it.
                    if ["radial", "linear", "empty", "full"].contains(command["op"] as? String ?? "") {
                        nodes[index].generated = nil
                    }
                }
            case "remap":
                guard let map = object["map"] as? [String: Any] else {
                    throw DoctoredCoreError(message: "Missing RAW local remap coordinates.")
                }
                let parsed = try JSONDecoder().decode(RawSourceMap.self,
                    from: JSONSerialization.data(withJSONObject: map))
                guard try parsed.destination(from: (state.width, state.height)) == (state.width, state.height) else {
                    throw DoctoredCoreError(message: "Canonical local remap must retain the current RAW source dimensions.")
                }
                masks[index] = try control.withChildOperation { child in
                    try masks[index].remapped(parsed.json(), control: child)
                }
            default: throw DoctoredCoreError(message: "Unknown captured RAW local operation.")
            }
        }
        return (nodes, masks)
    }
}
