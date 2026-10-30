import AppKit

/// Edit > Content-Aware Fill…: the command that opens the sheet, and the
/// exact-candidate sheet. Publication lives in ContentAwareFillPublication.
///
/// The sheet and MCP use ContentAwareFillWork for the same native request,
/// and ContentAwareFillPublication commits the reviewed final owner once.
extension EditorViewController {
    /// Edit > Content-Aware Fill… — opens the sheet over the current
    /// selection.
    ///
    /// `validateUserInterfaceItem` already disables the item without a
    /// selection, on an adjustment layer, in Quick Mask and with a colour
    /// plane or an alpha channel targeted, but a key equivalent can still
    /// arrive at a disabled item, so every one of those questions is asked
    /// again here. The two that have a sentence to say (an adjustment layer,
    /// a plane or channel target) say it through the shared refusals rather
    /// than a bare beep.
    @objc func contentAwareFill(_ sender: Any?) {
        guard document?.doc != nil, canvas.selection != nil, !canvas.quickMaskActive else {
            NSSound.beep()
            return
        }
        // A fill rewrites the LAYER's own pixels: an adjustment layer has
        // none, and a plane or channel target says an edit lands somewhere
        // else entirely (EditorViewController+Channels' rule, the one Clear
        // and Cut already follow).
        if refuseAdjustmentPixelEdit() { return }
        if refusePlaneTargetEdit() { return }
        // A GROUP has no pixels to fill, and a lock refuses the fill outright:
        // said before the sheet opens, so the user is not asked to preview and
        // then rasterize a described layer for an edit the core will refuse.
        if refuseGroupPixelEdit() { return }
        if let document = document,
           refuseLockedEdit(layer: document.activeLayerIndex, kind: DR_EDIT_PIXELS) {
            return
        }
        presentAsSheet(ContentAwareFillSheetController(editor: self))
    }

}

extension RasterDocument {
    /// A cheap value that identifies ONE layer well enough for a sheet to
    /// prove, when it commits, that the layer it previewed is still the layer
    /// at that index.
    ///
    /// A document-modal sheet does not stop the main run loop, so an agent's
    /// `delete_layer`, `add_layer` or `reorder_layers` can land behind it and
    /// silently renumber everything above the change. The core has no stable
    /// per-layer identity to ask for, so this is the next best thing: the
    /// stack's height plus everything `layerInfo` knows that a fill does not
    /// itself change — the name and the layer's own box. A rename behind the
    /// sheet therefore also refuses, which is the safe direction to be wrong
    /// in: the user is told the layer moved rather than shown a fill on a
    /// layer they never previewed.
    ///
    /// nil for an index that is out of range, so a deleted top layer refuses
    /// on this test as well as on the range check.
    ///
    /// It carries the entry's DEPTH and its parent chain too: re-parenting a
    /// layer into a group behind the sheet can leave the stack's height, the
    /// layer's box and its name all identical while what it composites
    /// against — and therefore what the preview showed — has changed
    /// completely.
    func layerFingerprint(_ index: Int) -> String? {
        guard let info = layerInfo(index) else { return nil }
        let tree = layerTree
        let chain = ([index] + tree.ancestors(of: index))
            .map { "\($0):\(tree.depth(of: $0))" }
            .joined(separator: ">")
        return "\(layerCount)/\(info.offsetX),\(info.offsetY),\(info.width)x\(info.height)/"
            + "\(chain)/" + info.name
    }
}
