import AppKit

extension AppDelegate {
    func layerMenu() -> NSMenu {
        let menu = NSMenu(title: "Layer")
        menu.addItem(
            item("New Layer", #selector(EditorViewController.newLayer(_:)), "n", [.command, .shift]))
        menu.addItem(submenuItem(newFillLayerMenu()))
        menu.addItem(submenuItem(newAdjustmentLayerMenu()))
        menu.addItem(
            item("Layer Content Options…", #selector(EditorViewController.layerContentOptions(_:))))
        menu.addItem(item("Place Embedded…", #selector(EditorViewController.placeEmbeddedImage(_:))))
        menu.addItem(item("Place Linked…", #selector(EditorViewController.placeLinkedImage(_:))))
        menu.addItem(item("Redevelop RAW…", #selector(EditorViewController.redevelopRaw(_:))))
        menu.addItem(item("RAW Presets…", #selector(EditorViewController.showRawPresets(_:))))
        menu.addItem(item("Synchronize RAW Settings…", #selector(EditorViewController.synchronizeRawSettings(_:))))
        menu.addItem(item("Migrate RAW Decoder…", #selector(EditorViewController.migrateRawDecoder(_:))))
        menu.addItem(item("RAW Source…", #selector(EditorViewController.showRawSource(_:))))
        menu.addItem(item("Rasterize RAW Layer", #selector(EditorViewController.rasterizeEmbeddedImage(_:))))
        menu.addItem(item("Artwork Colors…", #selector(EditorViewController.editArtworkColors(_:))))
        menu.addItem(item("Rasterize Artwork", #selector(EditorViewController.rasterizeArtwork(_:))))
        // Places a Live Photo's still as an ordinary layer.
        menu.addItem(
            item("Place Live Photo…", #selector(EditorViewController.placeLivePhoto(_:))))
        // ⌘J moves to Layer Via Copy below; Duplicate Layer keeps its item
        // and the panel's button. The ACTION behind ⌘J routes back here for a
        // group, an adjustment layer, a described layer, several layers
        // selected, or no selection (EditorViewController+MergeStamp.swift) —
        // layer_via is a single-entry op, so acting on the primary alone would
        // silently drop the rest of a multi-selection.
        menu.addItem(item("Duplicate Layer", #selector(EditorViewController.duplicateLayer(_:))))
        // #547 phase 4c: "Duplicate Layer… ▸ Destination" as a
        // sheet whose Destination is a new document or any open one.
        menu.addItem(item("Duplicate to Document…",
                          #selector(EditorViewController.duplicateToNewDocument(_:))))
        menu.addItem(item("Delete Layer", #selector(EditorViewController.deleteLayer(_:))))
        menu.addItem(.separator())
        // Layer groups on ⌘G / ⇧⌘G; ⌥⌘G (Create Clipping
        // Mask, below) is untouched.
        menu.addItem(item("New Group", #selector(EditorViewController.groupLayers(_:)), "g"))
        menu.addItem(
            item(
                "Ungroup Layers", #selector(EditorViewController.ungroupLayers(_:)), "g",
                [.command, .shift]))
        menu.addItem(.separator())
        menu.addItem(item("Layer Via Copy", #selector(EditorViewController.layerViaCopy(_:)), "j"))
        menu.addItem(
            item(
                "Layer Via Cut", #selector(EditorViewController.layerViaCut(_:)), "j",
                [.command, .shift]))
        menu.addItem(.separator())
        menu.addItem(submenuItem(arrangeMenu()))
        menu.addItem(submenuItem(alignMenu()))
        menu.addItem(submenuItem(distributeMenu()))
        // A per-pixel statistic across the selected layers, and the
        // same after aligning them.
        menu.addItem(submenuItem(stackModeMenu()))
        menu.addItem(item("Reduce Noise from Stack…",
                          #selector(EditorViewController.showReduceNoiseFromStack(_:))))
        // The selected layers, as exposures, into a new float document.
        menu.addItem(
            item("Merge Layers to HDR…", #selector(EditorViewController.mergeLayersToHDR(_:))))
        menu.addItem(submenuItem(lockMenu()))
        menu.addItem(submenuItem(labelMenu()))
        menu.addItem(.separator())
        menu.addItem(item("Link Layers", #selector(EditorViewController.linkLayers(_:))))
        menu.addItem(item("Unlink Layers", #selector(EditorViewController.unlinkLayers(_:))))
        menu.addItem(.separator())
        // Modal on-canvas session, not a tool: Return commits, Escape cancels.
        menu.addItem(item("Free Transform", #selector(EditorViewController.freeTransform(_:)), "t"))
        // ⌥⌘T: Free Transform a copy of the selected pixels.
        menu.addItem(item("Free Transform a Copy",
                          #selector(EditorViewController.freeTransformCopy(_:)), "t", [.command, .option]))
        // ⇧⌘T / ⌥⇧⌘T: repeat the document's last Free Transform.
        menu.addItem(item("Transform Again",
                          #selector(EditorViewController.transformAgain(_:)), "t", [.command, .shift]))
        menu.addItem(item("Transform Again with Copy",
                          #selector(EditorViewController.transformAgainWithCopy(_:)), "t",
                          [.command, .shift, .option]))
        menu.addItem(item("Warp…", #selector(EditorViewController.showPersistedWarp(_:))))
        menu.addItem(.separator())
        menu.addItem(submenuItem(layerMaskMenu()))
        menu.addItem(submenuItem(vectorMaskMenu()))
        // A Generative Layer's kept pictures (GenerativeVariationsSheet.swift).
        menu.addItem(item("Generative Variations…",
                          #selector(EditorViewController.showGenerativeVariations(_:))))
        // One toggling item, retitled "Release Clipping Mask" in
        // EditorViewController.validateUserInterfaceItem when the active
        // layer is already clipped.
        menu.addItem(
            item(
                "Create Clipping Mask",
                #selector(EditorViewController.toggleClippingMask(_:)), "g",
                [.command, .option]))
        // Re-opens the active adjustment layer's dialog; enabled only for an
        // adjustment layer whose op has one (validation in the editor).
        menu.addItem(
            item("Adjustment Options…", #selector(EditorViewController.adjustmentOptions(_:))))
        menu.addItem(submenuItem(layerStyleMenu()))
        menu.addItem(.separator())
        // Retitled "Merge Layers" by the editor's validation when several
        // entries are selected.
        // ⌘E and ⇧⌘E (`designs/v1-shortcuts-fade.md`).
        menu.addItem(item("Merge Down", #selector(EditorViewController.mergeDown(_:)), "e"))
        menu.addItem(
            item(
                "Merge Visible", #selector(EditorViewController.mergeVisible(_:)), "e",
                [.command, .shift]))
        menu.addItem(
            item(
                "Stamp Visible", #selector(EditorViewController.stampVisible(_:)), "e",
                [.command, .shift, .option]))
        // #547 phase 4c: Stamp Selected. No shortcut: ⌥⌘E is
        // Quick Export here.
        menu.addItem(item("Stamp Selected", #selector(EditorViewController.stampSelected(_:))))
        menu.addItem(item("Flatten Image", #selector(EditorViewController.flattenImage(_:))))
        return menu
    }

    /// Layer ▸ Arrange. Each item moves the entry among its SIBLINGS only —
    /// never into or out of a group; a drag in the panel, or reorder_layer
    /// with a depth, is how an entry changes level.
    private func arrangeMenu() -> NSMenu {
        let menu = NSMenu(title: "Arrange")
        menu.addItem(
            item(
                "Bring to Front", #selector(EditorViewController.bringToFront(_:)), "]",
                [.command, .shift]))
        menu.addItem(
            item("Bring Forward", #selector(EditorViewController.bringForward(_:)), "]"))
        menu.addItem(
            item("Send Backward", #selector(EditorViewController.sendBackward(_:)), "["))
        menu.addItem(
            item(
                "Send to Back", #selector(EditorViewController.sendToBack(_:)), "[",
                [.command, .shift]))
        return menu
    }

    /// Layer ▸ Align. Aligns the selection's CONTENT bounds — the box of
    /// actually opaque pixels, which for a canvas-sized layer is nothing
    /// like its pixel rect.
    private func alignMenu() -> NSMenu {
        let menu = NSMenu(title: "Align")
        menu.addItem(item("Left Edges", #selector(EditorViewController.alignLeft(_:))))
        menu.addItem(
            item("Horizontal Centers", #selector(EditorViewController.alignCenterX(_:))))
        menu.addItem(item("Right Edges", #selector(EditorViewController.alignRight(_:))))
        menu.addItem(.separator())
        menu.addItem(item("Top Edges", #selector(EditorViewController.alignTop(_:))))
        menu.addItem(item("Vertical Centers", #selector(EditorViewController.alignCenterY(_:))))
        menu.addItem(item("Bottom Edges", #selector(EditorViewController.alignBottom(_:))))
        return menu
    }

    /// Layer ▸ Stack Mode: one item per `StackMode`, all on one
    /// selector; an item's `tag` is its mode's position in `allCases`.
    private func stackModeMenu() -> NSMenu {
        let menu = NSMenu(title: StackModes.title)
        for (position, mode) in StackMode.allCases.enumerated() {
            let entry = item(mode.title, #selector(EditorViewController.stackMode(_:)))
            entry.tag = position
            menu.addItem(entry)
        }
        return menu
    }

    /// Layer ▸ Distribute. Equalizes the GAPS between adjacent entries, so
    /// it needs three of them (validation in the editor).
    private func distributeMenu() -> NSMenu {
        let menu = NSMenu(title: "Distribute")
        menu.addItem(
            item(
                "Horizontal Spacing",
                #selector(EditorViewController.distributeHorizontally(_:))))
        menu.addItem(
            item("Vertical Spacing", #selector(EditorViewController.distributeVertically(_:))))
        return menu
    }

    /// Layer ▸ Lock. Each item toggles its own bit over the selection, with
    /// the checkmark showing the active layer's state; Lock All is the three
    /// bits together (PSD's own spelling), on ⌘/.
    private func lockMenu() -> NSMenu {
        let menu = NSMenu(title: "Lock")
        menu.addItem(
            item("Transparency", #selector(EditorViewController.lockTransparency(_:))))
        menu.addItem(item("Pixels", #selector(EditorViewController.lockPixels(_:))))
        menu.addItem(item("Position", #selector(EditorViewController.lockPosition(_:))))
        menu.addItem(.separator())
        menu.addItem(item("Lock All", #selector(EditorViewController.lockAll(_:)), "/"))
        return menu
    }

    /// Layer > Label: the colour label a row is marked with, mirrored by the
    /// layers panel's row menu. The item's TAG is the label's code, so the
    /// editor's one handler serves both menus.
    private func labelMenu() -> NSMenu {
        let menu = NSMenu(title: "Label")
        for label in LayerLabel.allCases {
            let entry = item(label.displayName, #selector(EditorViewController.setLayerLabel(_:)))
            entry.tag = Int(label.rawValue)
            menu.addItem(entry)
            if label == .none { menu.addItem(.separator()) }
        }
        return menu
    }

    /// Layer > New Adjustment Layer: one item per adjustment op, named like
    /// the destructive Filters items where they overlap, grouped
    /// parameterized (dialog) before parameterless (immediate) the way the
    /// Filters menu groups its own.
    /// Layer ▸ New Fill Layer: a layer that is one colour, one
    /// gradient, or one tiled pattern, over the whole canvas
    /// (EditorViewController+FillLayers.swift).
    private func newFillLayerMenu() -> NSMenu {
        let menu = NSMenu(title: "New Fill Layer")
        menu.addItem(
            item("Solid Color…", #selector(EditorViewController.newSolidColorFillLayer(_:))))
        menu.addItem(
            item("Gradient…", #selector(EditorViewController.newGradientFillLayer(_:))))
        menu.addItem(
            item("Pattern…", #selector(EditorViewController.newPatternFillLayer(_:))))
        return menu
    }

    private func newAdjustmentLayerMenu() -> NSMenu {
        let menu = NSMenu(title: "New Adjustment Layer")
        menu.addItem(
            item(
                "Brightness/Contrast/Saturation…",
                #selector(EditorViewController.newAdjustmentLayerBCS(_:))))
        menu.addItem(
            item("Curves…", #selector(EditorViewController.newAdjustmentLayerCurves(_:))))
        menu.addItem(
            item("Levels…", #selector(EditorViewController.newAdjustmentLayerLevels(_:))))
        menu.addItem(
            item("Hue Rotate…", #selector(EditorViewController.newAdjustmentLayerHueRotate(_:))))
        menu.addItem(
            item("Posterize…", #selector(EditorViewController.newAdjustmentLayerPosterize(_:))))
        menu.addItem(
            item("Threshold…", #selector(EditorViewController.newAdjustmentLayerThreshold(_:))))
        // The phase-5 ops, in AdjustmentMenuOrder's one order (shared with
        // Image > Adjustments and the panel footer), tag-indexed into it.
        for (tag, op) in AdjustmentMenuOrder.newOps.enumerated() {
            let entry = item(
                op.displayName + "…",
                #selector(EditorViewController.newAdjustmentLayerOp(_:)))
            entry.tag = tag
            menu.addItem(entry)
        }
        menu.addItem(.separator())
        menu.addItem(
            item("Invert", #selector(EditorViewController.newAdjustmentLayerInvert(_:))))
        menu.addItem(
            item("Grayscale", #selector(EditorViewController.newAdjustmentLayerGrayscale(_:))))
        menu.addItem(
            item("Sepia", #selector(EditorViewController.newAdjustmentLayerSepia(_:))))
        menu.addItem(.separator())
        menu.addItem(
            item(
                "From Photoshop Preset…",
                #selector(EditorViewController.newAdjustmentLayerFromPhotoshopPreset(_:))))
        return menu
    }

    /// Layer > Mask. Enablement (and the Enable item's checkmark) comes from
    /// EditorViewController.validateUserInterfaceItem.
    private func layerMaskMenu() -> NSMenu {
        let menu = NSMenu(title: "Mask")

        let add = NSMenu(title: "Add Layer Mask")
        add.addItem(
            item("Reveal All", #selector(EditorViewController.addLayerMaskRevealAll(_:))))
        add.addItem(item("Hide All", #selector(EditorViewController.addLayerMaskHideAll(_:))))
        add.addItem(
            item(
                "From Selection", #selector(EditorViewController.addLayerMaskFromSelection(_:))))
        menu.addItem(submenuItem(add))

        menu.addItem(item("Delete Layer Mask", #selector(EditorViewController.deleteLayerMask(_:))))
        menu.addItem(item("Apply Layer Mask", #selector(EditorViewController.applyLayerMask(_:))))
        menu.addItem(.separator())
        menu.addItem(
            item("Enable Layer Mask", #selector(EditorViewController.toggleLayerMaskEnabled(_:))))
        menu.addItem(item("Link with Content", #selector(EditorViewController.toggleMaskContentLink(_:))))
        menu.addItem(item("Properties…", #selector(EditorViewController.showMaskProperties(_:))))
        menu.addItem(item("Transform Mask…", #selector(EditorViewController.transformMask(_:))))
        return menu
    }

    /// Layer > Vector Mask. Enablement and the two check marks come
    /// from EditorViewController+VectorMasks.swift's `validateVectorMaskItem`.
    private func vectorMaskMenu() -> NSMenu {
        let menu = NSMenu(title: "Vector Mask")
        menu.addItem(
            item(
                "Current Path",
                #selector(EditorViewController.addVectorMaskFromCurrentPath(_:))))
        menu.addItem(
            item("Reveal All", #selector(EditorViewController.addVectorMaskRevealAll(_:))))
        menu.addItem(item("Delete", #selector(EditorViewController.deleteVectorMask(_:))))
        menu.addItem(.separator())
        menu.addItem(item("Enable", #selector(EditorViewController.toggleVectorMaskEnabled(_:))))
        menu.addItem(item("Link", #selector(EditorViewController.toggleVectorMaskLinked(_:))))
        return menu
    }

    /// Layer > Layer Style: Blending Options…, one item per effect (each
    /// opens the sheet on that effect, turned on — the item's tag indexes
    /// LayerStyleEffectKind.allCases, the dialog's order), then
    /// Copy / Paste / Clear. Enablement comes from
    /// EditorViewController.validateUserInterfaceItem.
    private func layerStyleMenu() -> NSMenu {
        let menu = NSMenu(title: "Layer Style")
        menu.addItem(
            item("Blending Options…", #selector(EditorViewController.layerStyle(_:))))
        menu.addItem(.separator())
        for (tag, kind) in LayerStyleEffectKind.allCases.enumerated() {
            let entry = item(
                kind.title + "…", #selector(EditorViewController.layerStyleEffect(_:)))
            entry.tag = tag
            menu.addItem(entry)
        }
        menu.addItem(.separator())
        menu.addItem(
            item("Copy Layer Style", #selector(EditorViewController.copyLayerStyle(_:))))
        menu.addItem(
            item("Paste Layer Style", #selector(EditorViewController.pasteLayerStyle(_:))))
        menu.addItem(
            item("Clear Layer Style", #selector(EditorViewController.clearLayerStyle(_:))))
        return menu
    }
}
