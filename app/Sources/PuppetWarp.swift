import AppKit

// Edit ▸ Puppet Warp: the options and pins, the request JSON the core takes
// (`doc_puppet.rs`'s `puppet_warp`, `selected_pixels.rs`'s `puppet_selected`) and the
// recorded Action step. The session is `PuppetWarpSession`, the commit `MeshWarpWork`,
// the MCP mirror `AgentServer+PuppetWarp.swift`.

/// The options bar's Mode: how much the mesh resists stretching.
enum PuppetMode: String, CaseIterable {
    case rigid, normal, distort

    var title: String {
        switch self {
        case .rigid: return "Rigid"
        case .normal: return "Normal"
        case .distort: return "Distort"
        }
    }
}

/// The options bar's Density: how fine the mesh is.
enum PuppetDensity: String, CaseIterable {
    case fewer, normal, more

    var title: String {
        switch self {
        case .fewer: return "Fewer Points"
        case .normal: return "Normal"
        case .more: return "More Points"
        }
    }
}

/// One pin, in canvas pixels. `from` is the rest vertex it sits on (so the
/// core lands the vertex on `to` exactly), `rotate` a Fixed angle in degrees
/// (clockwise on the canvas) or nil for Auto.
struct PuppetPin: Equatable {
    var from: CGPoint
    var to: CGPoint
    var depth = 0
    var rotate: Double?

    /// The core's pin object; Auto and depth 0 are left out.
    var json: [String: Any] {
        var out: [String: Any] = [
            "from": [Double(from.x), Double(from.y)], "to": [Double(to.x), Double(to.y)],
        ]
        if depth != 0 { out["depth"] = depth }
        if let rotate { out["rotate"] = rotate }
        return out
    }

    var isMoved: Bool { from != to || (rotate ?? 0) != 0 }
}

/// Everything besides the pins: Mode, Density, Expansion and Show Mesh.
struct PuppetWarpOptions: Equatable {
    var mode = PuppetMode.normal
    var density = PuppetDensity.normal
    /// Pixels, −100…100.
    var expansion = 2
    var showMesh = true

    /// The mesh exports' `{"density","expansion"}`.
    var meshFields: [String: Any] { ["density": density.rawValue, "expansion": expansion] }
}

enum PuppetWarp {
    static let actionName = "Puppet Warp"
    static let maxPins = 256
    static let expansionRange = -100...100
    static let depthRange = -1000...1000

    /// Compact JSON with sorted keys, within the core's 64 KiB.
    static func json(_ fields: [String: Any]) throws -> String {
        guard JSONSerialization.isValidJSONObject(fields),
              let data = try? JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys]),
              data.count <= 64 * 1024, let text = String(data: data, encoding: .utf8) else {
            throw DoctoredCoreError(message: "The Puppet Warp request must be finite JSON within 64 KiB.")
        }
        return text
    }

    /// The layer form's fields (`puppet_warp`) or, with `bounds`, the
    /// selection float's (`puppet_selected`, `bounds` the selection's rect).
    static func fields(layer: Int, options: PuppetWarpOptions, pins: [[String: Any]],
                       float: Bool, bounds: CGRect?) -> [String: Any] {
        var out: [String: Any] = [
            "op": float ? "puppet_selected" : "puppet_warp", "layer": layer,
            "mode": options.mode.rawValue, "density": options.density.rawValue,
            "expansion": options.expansion, "pins": pins,
        ]
        if float, let bounds { out["bounds"] = MoveSelectedPixels.requestBounds(bounds) }
        return out
    }

    /// Every pin unmoved and unrotated: the warp changes nothing.
    static func isIdentity(_ pins: [PuppetPin]) -> Bool { !pins.contains(where: \.isMoved) }

    /// The recorded step (Actions): the pins, mode, density and expansion on
    /// the active layer, as `puppet_warp` takes them.
    static func step(options: PuppetWarpOptions, pins: [PuppetPin]) -> [ActionStep] {
        .step("puppet_warp", [
            "layer": ActionArgs.activeLayer, "pins": pins.map(\.json),
            "mode": options.mode.rawValue, "density": options.density.rawValue,
            "expansion": options.expansion,
        ], note: "Edit ▸ Puppet Warp")
    }

    /// `puppet_warp`'s arguments for the core: mode, density and expansion
    /// as the caller wrote them (the core refuses a bad one by name), and the
    /// pins as a list of objects; nil fields are left to the core's defaults.
    static func agentFields(_ a: [String: Any]) throws -> (pins: [[String: Any]], extra: [String: Any]) {
        guard let list = a["pins"] as? [Any] else {
            throw DoctoredCoreError(message: a["pins"] == nil ? "Puppet Warp needs at least one pin."
                : "puppet_warp pins must be a list of {from, to, depth, rotate} objects.")
        }
        let pins = try list.map { item -> [String: Any] in
            guard let pin = item as? [String: Any] else {
                throw DoctoredCoreError(message: "puppet_warp pins must be a list of {from, to, depth, rotate} objects.")
            }
            return pin
        }
        var extra: [String: Any] = [:]
        for key in ["mode", "density", "expansion"] where a[key] != nil {
            extra[key] = a[key]
        }
        return (pins, extra)
    }
}
