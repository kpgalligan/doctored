import AppKit

/// One 8-bit plane of a colour image — mirrors `DrPlane` in the C header
/// (core/include/doctored_core.h, "Channels"). A plane crossing the FFI is
/// always canvas-sized u8, row 0 = top: the same representation selections
/// and layer masks already use.
enum RasterPlane: Equatable, CaseIterable {
    case red
    case green
    case blue
    case alpha
    /// Rec. 709 luma. READ-ONLY — nothing writes it, and "take the result's
    /// gray" is defined as reading it back off an op's result.
    case luma
    /// A layer's mask. READ-ONLY here and valid on a layer only; the mask as
    /// a PAINT target is `PaintTarget.mask`, its own case.
    case mask

    var dr: DrPlane {
        switch self {
        case .red: return DR_PLANE_RED
        case .green: return DR_PLANE_GREEN
        case .blue: return DR_PLANE_BLUE
        case .alpha: return DR_PLANE_ALPHA
        case .luma: return DR_PLANE_LUMA
        case .mask: return DR_PLANE_MASK
        }
    }

    var displayName: String {
        switch self {
        case .red: return "Red"
        case .green: return "Green"
        case .blue: return "Blue"
        case .alpha: return "Alpha"
        case .luma: return "Luma"
        case .mask: return "Mask"
        }
    }

    /// The agent's spelling (`target: "red"`, `render {channel: "luma"}`).
    var agentName: String {
        switch self {
        case .red: return "red"
        case .green: return "green"
        case .blue: return "blue"
        case .alpha: return "alpha"
        case .luma: return "luma"
        case .mask: return "mask"
        }
    }

    /// Parses `agentName`; nil for anything else (a channel name, a typo).
    static func named(_ name: String) -> RasterPlane? {
        allCases.first { $0.agentName == name }
    }
}

/// What brush, eraser, fill, gradient, the destructive filters and the
/// adjustments edit on the active layer: its pixels, its layer mask, one
/// COLOUR PLANE of its pixels, or one of the document's alpha CHANNELS.
/// Pure UI state owned by EditorViewController — not undoable, not
/// persisted; reset to `.layer` when the active layer changes under a mask
/// target, when the mask goes away, when the channel goes away, or when the
/// document is replaced.
///
/// A CHANNEL is document state with no relation to the active layer, so it
/// is the one target an adjustment layer does NOT hijack. A `.plane` still
/// rewrites layer pixels, so it is coerced exactly as `.layer` is.
///
/// Declared `: Equatable` deliberately: the dozen `==`/`!=` sites across the
/// editor, the panels and the canvas would stop compiling the moment a case
/// gained an associated value without it, and there is no exhaustive switch
/// anywhere to make the compiler point at them.
enum PaintTarget: Equatable {
    case layer
    case mask
    /// `.red`/`.green`/`.blue`/`.alpha` only — see `isPaintable`.
    case plane(RasterPlane)
    /// Index into the document's channel list.
    case channel(Int)
    /// One ink plate of a CMYK layer (#547): the Move tool, its nudge and ⌘T move that plate's
    /// selected pixels, and painting, filling, filtering and adjusting it edit the plate's stored
    /// `1 − ink` as a colour plane's samples are edited (`cmyk_plate_scalar`): a grey g leaves
    /// ink 1 − g.
    case ink(CmykPlane)

    /// A COVERAGE target: the stroke is coverage, not colour, and commits
    /// once at mouse-up (the mask stroke route). True for mask, plane and
    /// channel.
    var isCoverage: Bool { self != .layer }

    /// Color planes and saved channels use their own grayscale display.
    /// Masks are also scalar edit targets, but can be viewed on the composite.
    var targetsPlaneOrChannel: Bool {
        switch self {
        case .layer, .mask: return false
        case .plane, .channel, .ink: return true
        }
    }

    /// One ink plate of a CMYK layer (#547).
    var isInk: Bool {
        if case .ink = self { return true }
        return false
    }

    /// One colour plane of the active layer's pixels.
    var isPlane: Bool {
        if case .plane = self { return true }
        return false
    }

    /// One of the document's alpha channels.
    var isChannel: Bool {
        if case .channel = self { return true }
        return false
    }

    /// A target a stroke may write: `.plane(.luma)` and `.plane(.mask)` are
    /// read-only and never become targets (see `paintablePlanes`).
    var isPaintable: Bool {
        switch self {
        case .layer, .mask, .channel, .ink: return true
        case .plane(let plane): return Self.paintablePlanes.contains(plane)
        }
    }

    /// The paintable colour planes — Luma is derived and Mask is its own
    /// case, so neither is ever a plane target.
    static let paintablePlanes: [RasterPlane] = [.red, .green, .blue, .alpha]

    /// Status-bar suffix (" · Mask", " · Red", " · Channel: Sky", …). A
    /// channel whose index has gone reports its number rather than lying.
    func statusSuffix(in doc: RasterDocument?) -> String {
        switch self {
        case .layer: return ""
        case .mask: return " · Mask"
        case .plane(let plane): return " · \(plane.displayName)"
        case .channel(let index):
            let name = doc?.channelInfo(index)?.name ?? "\(index)"
            return " · Channel: \(name)"
        case .ink(let plane): return " · \(plane.name)"
        }
    }

    /// The agent's `target` vocabulary:
    /// "layer" | "mask" | "red" | "green" | "blue" | "alpha" | "channel:<name>", and on a CMYK
    /// document "cyan" | "magenta" | "yellow" | "black".
    func agentName(in doc: RasterDocument?) -> String {
        switch self {
        case .layer: return "layer"
        case .mask: return "mask"
        case .plane(let plane):
            // #547 Lab phase 2e: a Lab document's colour planes are lightness, a and b, the names
            // its gate admits, so a recorded step replays.
            if doc?.isLab == true, plane != .alpha {
                return LabDocument.channelName(plane).lowercased()
            }
            return plane.agentName
        case .channel(let index):
            return "channel:\(doc?.channelInfo(index)?.name ?? String(index))"
        case .ink(let plane): return plane.key
        }
    }

    /// Destructive edits record the exact target they modify, including masks.
    func layerEditAgentName(in doc: RasterDocument?) -> String { agentName(in: doc) }

    /// The undo action name a coverage stroke on this target registers.
    /// `.layer` never reaches here (the canvas names layer strokes itself),
    /// but it answers the layer names so the function is total.
    func strokeActionName(erasing: Bool, in doc: RasterDocument?) -> String {
        switch self {
        case .layer: return erasing ? "Erase" : "Brush Stroke"
        case .mask: return erasing ? "Erase Mask" : "Paint Mask"
        case .plane(let plane):
            let name = doc?.isLab == true ? LabDocument.channelName(plane) : plane.displayName
            return erasing ? "Erase \(name)" : "Paint \(name)"
        case .channel: return erasing ? "Erase Channel" : "Paint Channel"
        case .ink(let plane): return erasing ? "Erase \(plane.name)" : "Paint \(plane.name)"
        }
    }
}
