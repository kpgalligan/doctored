import Foundation

/// The Spot Healing Brush's Type, the options-bar
/// choice: Content-Aware (the default, PatchMatch from the band around the
/// stroke), Create Texture (a texture built from the pixels under the
/// stroke; a second stroke rebuilds it from the first's result) and
/// Proximity Match (the best-matching neighbouring patch, copied whole).
/// Each replaces only the core's synthesis; the healing blend is the same.
///
/// The raw value is the wire spelling — MCP `spot_heal_stroke`'s `type`, an
/// Action's recorded `type` and the typed retouch request's `"type"` — and
/// `code` is `dr_doc_spot_heal_layer_job`'s `spot_type`.
enum SpotHealType: String, Codable, CaseIterable {
    case contentAware = "content_aware"
    case createTexture = "create_texture"
    case proximityMatch = "proximity_match"

    /// The options bar's titles.
    var displayName: String {
        switch self {
        case .contentAware: return "Content-Aware"
        case .createTexture: return "Create Texture"
        case .proximityMatch: return "Proximity Match"
        }
    }

    /// The C export's `spot_type`.
    var code: UInt32 {
        switch self {
        case .contentAware: return 0
        case .createTexture: return 1
        case .proximityMatch: return 2
        }
    }

    /// An MCP or Action argument dictionary's `type`: absent is Content-Aware,
    /// the meaning every request before the option had; anything that is not
    /// one of the three names is refused by name.
    static func argument(_ a: [String: Any]) throws -> SpotHealType {
        guard let raw = a["type"] else { return .contentAware }
        guard let name = raw as? String, let type = SpotHealType(rawValue: name) else {
            throw AgentServer.ToolError(
                message: "type must be content_aware, create_texture or proximity_match")
        }
        return type
    }
}
