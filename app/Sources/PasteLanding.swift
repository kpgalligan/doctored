import AppKit

// `PastePlacement.auto` resolves through `PasteContext.landing` (the destination selection's
// `clipboardWindow` outside Quick Mask, then the view's `EditorViewState.visible` — MCP's `view`,
// view 1 without one, the canvas with no window) to `selection` (centred, unmasked), `view`
// (centred on `visible.integral`) or `centered`; `.inPlace` resolves through
// `PasteContext.inPlace` (the copy's `ClipboardSource`) to `inPlace` or `relativeInPlace`.
// `PasteWork.landing` decides it before `byteTransform` / `typedDocument`, every framed placement
// reaches the core as `into` plus a frame, `PasteOutcome.placement` is the landed value, and
// `pasteStep` (the saved-payload step only; a live step records the request itself) records an
// `auto` / `at` request, a `relativeInPlace` landing and an external image's `inPlace` as `at` with
// the landed `x`/`y`, and writes `profile: "preserve"` only for Don't Convert (never `convert`).
// `PasteKind.resultingSelection` is what each paste leaves selected, published in its one step.

/// Where Edit ▸ Paste (⌘V) and Paste in Place (⇧⌘V) land a copy —
/// measured against Photoshop 26.10.0
/// (`scripts/photoshop-probe/paste_placement.py`):
///
/// 1. ⌘V with an active destination selection (not in Quick Mask, not
///    wholly off the canvas): centred on the selection's bounds
///    (`clipboardWindow`, the frame Paste Into uses). No mask is made.
/// 2. Otherwise ⌘V is centred on the visible part of the canvas in the
///    paste's view — NEVER at its source position: all nine measured pixel
///    pastes without a selection moved, a wholly visible copy in its own
///    document included, and all four layer pastes likewise. (Batch 173 kept
///    a wholly visible copy in place; the measurement overturned it.)
/// 3. With no editor window (MCP only) "visible" is the whole canvas, so
///    rule 2 is the canvas centre.
///
/// ⇧⌘V keeps the copy's source coordinates when the destination canvas is
/// the source's size, and otherwise keeps its CENTRE at the same fraction of
/// the canvas (`ClipboardSource.relativeOffset`), whatever the view and any
/// destination selection. Every frame placement lands by the core's
/// `div_euclid` centring on its frame (`PastePlacement.offset(…frame:)`),
/// and the core receives `into` plus that frame.
struct PasteContext {
    /// The destination selection's frame; nil without a selection, in Quick
    /// Mask, or when the selection lies entirely off the canvas.
    let selectionFrame: CGRect?
    /// The view's `EditorViewState.visible`; nil when there is no window or
    /// the visible area is empty.
    let visible: CGRect?
    /// The copy's source canvas (`ClipboardSource.read`); nil for a payload
    /// file and for a paste that has none.
    let origin: ClipboardSource?

    init(selection: CanvasSelection?, quickMask: Bool, in doc: RasterDocument, visible: CGRect?,
         origin: ClipboardSource?) {
        selectionFrame = selection != nil && !quickMask ? doc.clipboardWindow(selection) : nil
        if let visible, !visible.isNull, visible.width > 0, visible.height > 0 {
            self.visible = visible
        } else {
            self.visible = nil
        }
        self.origin = origin
    }

    /// `.auto` → `.selection` / `.view` / `.centered`, in the order above.
    var landing: PasteLanding {
        if let selectionFrame {
            return PasteLanding(placement: .selection, frame: selectionFrame)
        }
        if let visible {
            return PasteLanding(placement: .view, frame: visible.integral)
        }
        return PasteLanding(placement: .centered, frame: nil)
    }

    /// `.inPlace` of a copy at `source`: `.inPlace` when the copy came from
    /// a canvas of `canvas`'s size (or its source is unknown), otherwise
    /// `.relativeInPlace` at `ClipboardSource.relativeOffset`.
    func inPlace(source: (x: Int, y: Int), size: (width: Int, height: Int),
                 canvas: (width: Int, height: Int)) -> PasteLanding {
        guard let origin, origin.width != canvas.width || origin.height != canvas.height else {
            return PasteLanding(placement: .inPlace, frame: nil)
        }
        let at = ClipboardSource.relativeOffset(of: source, size: size,
                                                from: (origin.width, origin.height),
                                                into: canvas)
        return PasteLanding(placement: .relativeInPlace,
                            frame: CGRect(x: at.x, y: at.y, width: size.width,
                                          height: size.height))
    }
}

/// A resolved paste placement: never `.auto`, and `frame` is non-nil exactly
/// when `placement.usesFrame`.
struct PasteLanding {
    let placement: PastePlacement
    let frame: CGRect?
}

/// Which measured selection verdict a paste follows.
/// A PIXEL paste made with a selection deselects — ⌘V and ⇧⌘V alike, in
/// the same document and another — and a paste of copied LAYERS keeps the
/// selection, ⌘V and ⇧⌘V alike.
enum PasteKind {
    case pixels
    case pixelsInPlace
    case layers

    /// A pixel paste's kind for its requested placement.
    init(pixels placement: PastePlacement) {
        self = placement == .inPlace ? .pixelsInPlace : .pixels
    }

    /// The selection state the paste publishes in its one step; nil =
    /// unchanged. Keyed on the kind and whether a selection was active
    /// (outside Quick Mask, where a paste ignores it), never on where the
    /// paste landed.
    func resultingSelection(_ state: SelectionHistoryState) -> SelectionHistoryState? {
        guard state.selection != nil, !state.quickMaskActive else { return nil }
        switch self {
        case .pixels, .pixelsInPlace: return state.consumedByPaste()
        case .layers: return nil
        }
    }
}
