import AppKit

// `PasteMask` (read at publish time, refused in Quick Mask) gives the pasted layer a canvas-sized
// unlinked mask centred on the selection's bounds and consumes the selection in the same row, and
// `addingLayerMask(_:fromSelection:)` / `(_:fromCoverage:)` is the canvas-sized 8-bit From
// Selection over `dr_doc_set_mask`.

/// Edit ▸ Paste Special ▸ Paste Into / Paste Outside and the
/// canvas-sized From Selection mask.
///
/// A masked paste lands the pasted layer centred on the selection's bounds
/// (`PastePlacement.offset(…frame:)`) and gives it a CANVAS-sized, UNLINKED
/// mask at (0, 0): the selection's coverage (Into), or its inverse with
/// outside coverage 255 (Outside). Canvas-sized, never cropped to the layer,
/// because a mask never grows when it is painted (`paint_mask`) — a
/// layer-sized mask could never be revealed past the pasted pixels — and it
/// is exactly what 16-bit From Selection already makes. The selection is
/// consumed (`previous` keeps it for Reselect) in the same history row, and
/// Quick Mask refuses (the Transform/Refine Selection precedent).
struct PasteMask {
    let selection: CanvasSelection
    /// The selection's integral bounds clipped to the canvas: the paste's frame.
    let frame: CGRect
    /// Paste Outside: the inverted coverage with outside coverage 255.
    let outside: Bool
    private let canvasWidth: Int
    private let canvasHeight: Int

    /// Throws the refusal sentence for Quick Mask, no selection, or a
    /// selection entirely off the canvas. `placement` must be masked.
    init(placement: PastePlacement, selection: CanvasSelection?, quickMask: Bool,
         in doc: RasterDocument) throws {
        let verb = placement == .outside ? "Paste Outside" : "Paste Into"
        guard placement.masksSelection else {
            throw DoctoredCoreError(message: "\(verb) needs a masked placement.")
        }
        if quickMask {
            throw DoctoredCoreError(message: "Exit Quick Mask before \(verb): the selection is "
                + "still being edited.")
        }
        guard let selection else {
            throw DoctoredCoreError(message: "\(verb) needs an active selection to mask the pasted "
                + "layer. Make a selection first.")
        }
        guard let frame = doc.clipboardWindow(selection) else {
            throw DoctoredCoreError(message: "The selection lies entirely outside the canvas.")
        }
        self.selection = selection
        self.frame = frame
        outside = placement == .outside
        canvasWidth = doc.width
        canvasHeight = doc.height
    }

    var outsideCoverage: UInt8 { outside ? 255 : 0 }

    /// The history row and the recorded note's verb.
    var historyName: String { outside ? "Paste Outside" : "Paste Into" }

    /// 8 bits: the canvas-sized byte coverage the mask stores.
    func bytes() -> [UInt8] {
        let bytes = selection.maskBytes()
        return outside ? bytes.map { 255 - $0 } : bytes
    }

    /// 16 bits and float: the exact u16 coverage, with the selection's own
    /// provenance.
    func samples(width: Int, height: Int) throws -> PrecisionScalarBuffer {
        let coverage = try PrecisionStrokeCoverage.selection(
            width: width, height: height, selection: selection)
        guard outside else { return coverage }
        return try PrecisionScalarBuffer(
            samples: coverage.samples.map { 65535 - $0 }, width: width, height: height,
            provenance: coverage.provenance)
    }

    /// The selection state the paste publishes: the selection consumed,
    /// kept as `previous` for Select ▸ Reselect.
    func consumed(_ state: SelectionHistoryState) -> SelectionHistoryState {
        state.consumedByPaste()
    }

    /// The MCP reply's `mask` object.
    var json: [String: Any] {
        ["x": 0, "y": 0, "width": canvasWidth, "height": canvasHeight, "linked": false,
         "outside_coverage": Int(outsideCoverage),
         "frame": [Int(frame.minX), Int(frame.minY), Int(frame.width), Int(frame.height)]]
    }
}

extension SelectionHistoryState {
    /// A paste that consumes the selection (Paste Into / Outside, and
    /// every pixel paste made with one, `PasteKind`): nothing
    /// selected, the selection kept as `previous` for Select ▸ Reselect.
    func consumedByPaste() -> SelectionHistoryState {
        SelectionHistoryState(selection: nil, previous: selection ?? previous,
                              quickMaskActive: false)
    }
}

extension RasterDocument {
    /// Layer ▸ Layer Mask ▸ From Selection at 8 bits: a CANVAS-sized
    /// linked mask at (0, 0) — 16-bit's shape — so it is never cropped to the
    /// layer's rect. A group keeps `add_mask` (its mask is canvas-sized
    /// already and its offset positions it). nil at depth, and for the
    /// core's identical-mask no-op (`hasLayerMask(_:fromCoverage:)`).
    func addingLayerMask(_ idx: Int, fromSelection selection: CanvasSelection) -> RasterDocument? {
        addingLayerMask(idx, fromCoverage: selection.maskBytes())
    }

    /// The same rule over canvas-sized byte coverage — Select ▸ Refine
    /// Selection's layer-mask output, which holds coverage, not a selection.
    func addingLayerMask(_ idx: Int, fromCoverage coverage: [UInt8]) -> RasterDocument? {
        guard !isHighDepth else { return nil }
        if layerInfo(idx)?.isGroup == true {
            return addingLayerMask(idx, kind: DR_MASK_FROM_SELECTION, selection: coverage)
        }
        return settingLayerMask(idx, coverage: coverage, width: width, height: height,
                                x: 0, y: 0, linked: true, outsideCoverage: 0)
    }

    /// Whether `addingLayerMask(_:fromCoverage:)` on raster layer `idx` is
    /// the core's identical-mask no-op (the purity rule answers nil): the
    /// mask it would set equals the current one — checked by setting it
    /// unlinked, then linked again, and comparing.
    func hasLayerMask(_ idx: Int, fromCoverage coverage: [UInt8]) -> Bool {
        guard !isHighDepth, layerInfo(idx)?.isGroup == false,
              addingLayerMask(idx, fromCoverage: coverage) == nil else { return false }
        func set(_ d: RasterDocument, linked: Bool) -> RasterDocument? {
            d.settingLayerMask(idx, coverage: coverage, width: d.width, height: d.height,
                               x: 0, y: 0, linked: linked, outsideCoverage: 0)
        }
        guard let unlinked = set(self, linked: false), let relinked = set(unlinked, linked: true)
        else { return false }
        return relinked.hasSameLayerMask(as: self, index: idx)
    }
}

extension ImageDocument {
    /// After a masked paste publishes: the pasted layer is active and the
    /// LAYER (not its new mask) is the edit target, in every editor.
    /// `planeEditTarget`'s `didSet` refreshes the current history entry, so
    /// one Undo restores stack, selection and target together.
    func landMaskedPaste(at landing: Int) {
        activeLayerIndex = landing
        planeEditTarget = .layer
        for editor in editors {
            editor.setPaintTarget(.layer)
            editor.activeLayerDidChange()
        }
    }
}
