import Foundation

// At 32-bit float a set is applied at
// once and keeps no field: `get_warp` answers null, the sheet disables Remove Warp and Bake Warp,
// says each Apply warps the current pixels again (sets compound; Undo adjusts or removes one), and
// an identity Apply (`PersistedWarpField.isIdentity`) beeps and stays open instead of closing
// silently. Core bindings, never a host sidecar, own persistence and original pixels.

enum PersistedWarpTarget: String { case pixels, mask }
enum PersistedWarpFilter: String { case bilinear, bicubic }

struct PersistedWarpField {
    var columns: Int
    var rows: Int
    var displacements: [[Double]]
    var filter: PersistedWarpFilter

    /// Every displacement component is exactly 0 — the Swift twin of the
    /// core's `WarpField::identity` (`warp.rs`), so a negative zero counts.
    var isIdentity: Bool {
        displacements.allSatisfy { $0.allSatisfy { $0 == 0 } }
    }

    static func identity(columns: Int = 3, rows: Int = 3,
                         filter: PersistedWarpFilter = .bicubic) -> Self {
        Self(columns: columns, rows: rows,
             displacements: Array(repeating: [0, 0], count: columns * rows), filter: filter)
    }
}

enum PersistedWarpRequest {
    case set(layer: Int, target: PersistedWarpTarget, field: PersistedWarpField)
    case clear(layer: Int)
    case bake(layer: Int)

    var layer: Int {
        switch self {
        case .set(let layer, _, _), .clear(let layer), .bake(let layer): return layer
        }
    }

    func jsonRequest() throws -> String {
        switch self {
        case .set(_, let target, let field):
            let nodes = try field.displacements.map { pair -> String in
                guard pair.count == 2, pair.allSatisfy(\.isFinite) else {
                    throw DoctoredCoreError(message: "Warp nodes need two finite values.")
                }
                // Swift's Double spelling round-trips its binary64 bits.
                // Foundation JSONSerialization rounds some low-order decimal
                // digits here, which would change the re-editable field on
                // an Action replay even when the rendered pixels coincide.
                return "[\(String(pair[0])),\(String(pair[1]))]"
            }.joined(separator: ",")
            return "{\"op\":\"set\",\"layer\":\(layer),\"target\":\"\(target.rawValue)\","
                + "\"columns\":\(field.columns),\"rows\":\(field.rows),"
                + "\"displacements\":[\(nodes)],\"filter\":\"\(field.filter.rawValue)\"}"
        case .clear:
            return "{\"op\":\"clear\",\"layer\":\(layer)}"
        case .bake:
            return "{\"op\":\"bake\",\"layer\":\(layer)}"
        }
    }
}

/// Foundation's JSON writer sometimes chooses a longer decimal which its own
/// reader rounds one binary64 code away. An NSDecimalNumber carrying Swift's
/// shortest round-trip spelling writes exactly the decimal the reader needs.
enum PersistedWarpAction {
    static func exactArguments(_ arguments: [String: Any]) -> [String: Any] {
        guard let rows = arguments["displacements"] as? [[NSNumber]] else { return arguments }
        var copy = arguments
        copy["displacements"] = rows.map { row in
            row.map { NSDecimalNumber(string: String($0.doubleValue)) }
        }
        return copy
    }
}
