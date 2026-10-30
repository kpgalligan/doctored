import AppKit

/// The ONE ordered list of phase-5 adjustment ops the three menus build
/// themselves from — Image ▸ Adjustments, Layer ▸ New Adjustment Layer and
/// the Layers panel's footer menu — so they cannot disagree about which ops
/// exist or in what order.
///
/// The legacy nine keep their own selectors (they predate the tag idiom);
/// these thirteen share one selector each, the menu item's tag indexing this
/// array, exactly as `layerStyleMenu` indexes `LayerStyleEffectKind`.
enum AdjustmentMenuOrder {
    /// The Adjustments grouping: the tonal ops, then the colour ops, then
    /// the mapping ops.
    static let newOps: [AdjustmentLayerOp] = [
        .exposure,
        .vibrance,
        .hueSaturation,
        .colorBalance,
        .blackAndWhite,
        .photoFilter,
        .channelMixer,
        .colorLookup,
        .gradientMap,
        .selectiveColor,
        .replaceColor,
        .shadowsHighlights,
        .whiteBalance,
    ]

    /// The op a tagged menu item names, or nil for a tag from a stale menu.
    static func op(for tag: Int) -> AdjustmentLayerOp? {
        newOps.indices.contains(tag) ? newOps[tag] : nil
    }
}

extension EditorViewController {
    /// Image ▸ Adjustments ▸ <op>: the DESTRUCTIVE twin — the same sheet,
    /// the same controls and the same core op the adjustment layer
    /// composites (`dr_image_adjust_op`), committed onto the active layer's
    /// pixels as one undo step.
    @objc func showAdjustmentSheet(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document = document, document.doc != nil else {
            NSSound.beep()
            return
        }
        let tag = (sender as? NSMenuItem)?.tag ?? 0
        guard let op = AdjustmentMenuOrder.op(for: tag),
              let sheet = AdjustmentSheets.make(
                op: op, document: document, canvas: canvas, mode: .destructive)
        else {
            NSSound.beep()
            return
        }
        presentAsSheet(sheet)
    }

    /// Image ▸ Adjustments ▸ Curves…: the destructive Curves, the
    /// layer dialog's own curve editor in `.destructive` mode, committed onto
    /// the active layer's (or targeted plane's) pixels under the selection as
    /// one undo step and recorded as the `apply_filter` curves step.
    @objc func showCurves(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document = document, document.doc != nil else {
            NSSound.beep()
            return
        }
        presentAsSheet(
            CurvesAdjustmentSheetController(document: document, canvas: canvas, mode: .destructive))
    }

    /// Image ▸ Adjustments ▸ Desaturate (⇧⌘U): Hue/Saturation's
    /// master Saturation at −100 on the active layer (or targeted plane),
    /// one undo step named "Desaturate". It records and mirrors as
    /// `apply_filter {filter:"hue_saturation", params:{saturation:-1}}` — the
    /// same op, so an Action replays it at any depth.
    @objc func applyDesaturate(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let params: [String: Any] = ["saturation": -1.0]
        if doc.isHighDepth {
            let before = document.doc
            _ = applyPrecisionFilter("hue_saturation", params: params)
            if document.doc !== before { document.undoManager?.setActionName("Desaturate") }
            return
        }
        let capture = document.captureOperation(previewBase: canvas.channelDisplay?.baseKind)
        document.applyCapturedOperation(
            capture, "Desaturate",
            record: .filter(
                "hue_saturation", ["params": params], target: capture?.agentTarget ?? "layer")
        ) {
            capture?.adjustmentImage($0, op: "hue_saturation", params: params)
        }
    }

    /// Layer ▸ New Adjustment Layer ▸ <op> (and the panel footer's menu):
    /// the non-destructive twin, through the same path the legacy nine take.
    @objc func newAdjustmentLayerOp(_ sender: Any?) {
        let tag = (sender as? NSMenuItem)?.tag ?? 0
        guard let op = AdjustmentMenuOrder.op(for: tag) else {
            NSSound.beep()
            return
        }
        newAdjustmentLayer(op)
    }
}
