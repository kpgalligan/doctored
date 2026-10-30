import AppKit

// The painting and retouching tools on a CMYK document (#1's retouch batch): which canvas tools
// and MCP tools run on ink at 8 and 16 bits, and the sentence each refused one carries. Dodge,
// Burn, Sponge, Blur, Sharpen, Smudge, the History Brush, Content-Aware Fill, the Clone Stamp and
// the Healing Brush run at both depths, Spot Healing, Patch, Remove and Content-Aware Move follow
// the Healing Brush and Content-Aware Fill, and Auto-Blend, Red Eye and Auto-Align stay refused.
// `scripts/photoshop-probe/cmyk_retouch_take.py` take1 checked the table against Photoshop: it
// found the first eight enabled at both depths and Auto-Blend disabled, could not drive the Clone
// Stamp or the Healing Brush (no source point) on RGB either, and found Red Eye and Auto-Align
// unknown on RGB and CMYK alike. The core runs every admitted tool on ink (`cmyk_retouch_brush`,
// `cmyk_retouch`); the gate is `CmykDocument.allows` and
// `CmykDocument.refusal(tool:arguments:depth:)`, which ask this table.

enum CmykRetouch {
    /// One tool's enablement in CMYK at each declared depth, and why it is refused (nil when it
    /// runs at both).
    struct Availability {
        let eight: Bool
        let sixteen: Bool
        let reason: String?
    }

    private static let runs = Availability(eight: true, sixteen: true, reason: nil)
    private static let rgbOnly = Availability(eight: false, sixteen: false,
                                              reason: "Doctored offers it on RGB documents only")

    /// The canvas retouch tools (take1's `avail_*` rows).
    static let tools: [EditorTool: Availability] = [
        .dodge: runs,
        .sponge: runs,
        .blurBrush: runs,
        .sharpenBrush: runs,
        .smudge: runs,
        .historyBrush: runs,
        // The Art History Brush runs on an 8-bit CMYK document and refuses at 16 bits; the core
        // refuses the 16-bit document by name (`precision_art_history.rs`), at mouse-down and
        // over MCP alike, so the canvas tool stays selectable to say why.
        .artHistoryBrush: runs,
        .clone: runs,
        .heal: runs,
        .spotHeal: runs,
        .patch: runs,
        .remove: runs,
        .contentAwareMove: runs,
        .redEye: rgbOnly,
    ]

    /// The MCP tools of the same tools (and Content-Aware Fill's), each with the canvas tool whose
    /// row answers it.
    static let agentTools: [String: EditorTool] = [
        "dodge_burn": .dodge,
        "sponge_stroke": .sponge,
        "blur_stroke": .blurBrush,
        "sharpen_stroke": .sharpenBrush,
        "smudge_stroke": .smudge,
        "history_brush_stroke": .historyBrush,
        "clone_stamp": .clone,
        "heal_stroke": .heal,
        "spot_heal_stroke": .spotHeal,
        "patch_region": .patch,
        "remove_stroke": .remove,
        "content_aware_move": .contentAwareMove,
        "content_aware_fill": .remove,
        "preview_content_aware_fill": .remove,
        "discard_content_aware_fill_preview": .remove,
    ]

    /// Whether canvas tool `tool` runs on a CMYK document at `depth` (8 or 16); false for a tool
    /// this table does not hold (`CmykDocument.allows` answers those).
    static func allows(_ tool: EditorTool, depth: Int) -> Bool {
        guard let entry = tools[tool] else { return false }
        return depth == 8 ? entry.eight : entry.sixteen
    }

    /// The refusal of a retouch tool this table refuses, in the core's words.
    static func refusal(_ tool: EditorTool) -> String {
        let reason = tools[tool]?.reason.map { ": \($0)" } ?? ""
        return "\(tool.displayName) is unavailable in a CMYK document\(reason)."
    }

    /// The MCP narrowing of an admitted retouch tool on a CMYK document: nil to admit the call,
    /// else its refusal. The layer's ink and (for Dodge/Burn, as at 16 bits) its mask are the
    /// targets; the Sponge on a mask keeps its every-depth refusal; Pass Through is a group's.
    static func refusal(tool: String, arguments: [String: Any], depth: Int) -> String? {
        guard let canvas = agentTools[tool] else { return nil }
        if !allows(canvas, depth: depth) { return refusal(canvas) }
        if let target = arguments["target"] as? String, target != "layer" {
            if tool == "sponge_stroke", target == "mask" { return FloatRetouch.spongeCoverageRefusal }
            if let refusal = AgentServer.sampledTargetRefusal(arguments) { return refusal }
            if !(tool == "dodge_burn" && target == "mask") {
                return "A CMYK document retouches a layer's ink: target \"\(target)\" is unavailable."
            }
        }
        if let blend = arguments["blend_mode"] as? String,
           blend.lowercased().replacingOccurrences(of: " ", with: "") == "passthrough" {
            return "The Pass Through blend mode is unavailable in a CMYK document."
        }
        return nil
    }
}
