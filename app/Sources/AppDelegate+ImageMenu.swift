import AppKit

extension AppDelegate {
    func imageMenu() -> NSMenu {
        let menu = NSMenu(title: "Image")
        // No chords: ⌘R is View ▸ Rulers and ⇧⌘R Filters ▸
        // Lens Correction… (`designs/v1-shortcuts-fade.md`).
        menu.addItem(
            item("Rotate 90° Clockwise", #selector(EditorViewController.rotateCW(_:))))
        menu.addItem(
            item(
                "Rotate 90° Counterclockwise", #selector(EditorViewController.rotateCCW(_:))))
        menu.addItem(item("Rotate 180°", #selector(EditorViewController.rotate180(_:))))
        // Phase 12F-1b3. Deliberately NO key equivalent and no Rotate
        // submenu: the Image menu is flat.
        menu.addItem(
            item("Rotate Arbitrary…", #selector(EditorViewController.rotateArbitrary(_:))))
        menu.addItem(item("Flip Horizontal", #selector(EditorViewController.flipH(_:))))
        menu.addItem(item("Flip Vertical", #selector(EditorViewController.flipV(_:))))
        menu.addItem(.separator())
        // No chord; ⌘K is Settings… (its hidden twin).
        menu.addItem(item("Crop", #selector(EditorViewController.cropToSelection(_:))))
        menu.addItem(
            item("Image Size…", #selector(EditorViewController.resizeImage(_:)), "i", [.command, .option]))
        menu.addItem(
            item("Canvas Size…", #selector(EditorViewController.showCanvasSize(_:)), "c", [.command, .option]))
        // The two commands that fit the canvas to the picture.
        menu.addItem(item("Trim…", #selector(EditorViewController.showTrim(_:))))
        menu.addItem(item("Reveal All", #selector(EditorViewController.revealAllCanvas(_:))))
        menu.addItem(.separator())
        // Colour is document state too, so Mode sits with Image Size and
        // Canvas Size rather than under a preferences window.
        menu.addItem(submenuItem(modeMenu()))
        // Evaluation sits with Mode because it is the same kind of fact: how
        // the document's numbers are to be READ. Mode says which space they
        // are in; Evaluation says whether the projection composites them as
        // stored or in linear light (EditorViewController+Evaluation.swift).
        menu.addItem(
            item("Evaluation…", #selector(EditorViewController.showEvaluation(_:))))
        // Tone Map sits beside Evaluation for the same reason: it is how a
        // 32-bit float document's extended-range numbers are to be READ on
        // screen and in every SDR deliverable
        // (EditorViewController+FloatDocument.swift).
        menu.addItem(
            item("Tone Map…", #selector(EditorViewController.showToneMap(_:))))
        menu.addItem(.separator())
        // The destructive adjustments live here, next to Mode and Image
        // Size, rather than under Filters: they change the picture's tone
        // and colour, while Filters holds the true pixel filters (blur,
        // sharpen, pixelate, noise, edge detect, emboss). Every item is the
        // twin of an adjustment layer of the same name and runs the
        // identical core op.
        menu.addItem(submenuItem(adjustmentsMenu()))
        menu.addItem(.separator())
        menu.addItem(
            item("Auto Tone", #selector(EditorViewController.autoTone(_:)), "l",
                 [.command, .shift]))
        menu.addItem(
            item("Auto Contrast", #selector(EditorViewController.autoContrast(_:)), "l",
                 [.command, .shift, .option]))
        menu.addItem(
            item("Auto Color", #selector(EditorViewController.autoColor(_:)), "b",
                 [.command, .shift]))
        menu.addItem(.separator())
        // Channel arithmetic and the channel list itself live under Image,
        // not Layer: channels are DOCUMENT state, next to Image Size and
        // Canvas Size.
        menu.addItem(item("Apply Image…", #selector(EditorViewController.applyImageSheet(_:))))
        menu.addItem(
            item("Calculations…", #selector(EditorViewController.calculationsSheet(_:))))
        menu.addItem(submenuItem(channelsMenu()))
        return menu
    }

    /// Image > Adjustments: every tone and colour adjustment that rewrites
    /// the active layer's pixels, grouped tonal, then colour, then mapping,
    /// then the odds and ends. The thirteen tagged ops share one selector,
    /// each item's tag indexing `AdjustmentMenuOrder.newOps` (layerStyleMenu's
    /// idiom); the older items keep the selectors and shortcuts they already
    /// had.
    private func adjustmentsMenu() -> NSMenu {
        let menu = NSMenu(title: "Adjustments")
        func destructive(
            _ op: AdjustmentLayerOp, _ key: String = "",
            _ modifiers: NSEvent.ModifierFlags = [.command]
        ) -> NSMenuItem {
            let entry = item(
                op.displayName + "…", #selector(EditorViewController.showAdjustmentSheet(_:)),
                key, modifiers)
            // Every op passed here is in the list, so the fallback is
            // unreachable; it exists so a mis-edit beeps rather than
            // opening the wrong op's dialog.
            entry.tag = AdjustmentMenuOrder.newOps.firstIndex(of: op) ?? -1
            return entry
        }
        menu.addItem(
            item(
                "Brightness/Contrast/Saturation…",
                #selector(EditorViewController.showAdjustments(_:)), "a",
                [.command, .option]))
        menu.addItem(item("Levels…", #selector(EditorViewController.showLevels(_:)), "l"))
        // ⌘M; Window ▸ Minimize moved to ⌃⌘M.
        menu.addItem(item("Curves…", #selector(EditorViewController.showCurves(_:)), "m"))
        menu.addItem(destructive(.exposure))
        menu.addItem(.separator())
        menu.addItem(destructive(.vibrance))
        // ⌘U, ⌘B and ⌥⇧⌘B.
        menu.addItem(destructive(.hueSaturation, "u"))
        menu.addItem(destructive(.colorBalance, "b"))
        menu.addItem(destructive(.blackAndWhite, "b", [.command, .option, .shift]))
        menu.addItem(destructive(.photoFilter))
        menu.addItem(destructive(.channelMixer))
        menu.addItem(destructive(.colorLookup))
        menu.addItem(.separator())
        menu.addItem(item("Invert", #selector(EditorViewController.applyInvert(_:)), "i"))
        menu.addItem(item("Equalize", #selector(EditorViewController.equalize(_:))))
        menu.addItem(item("Posterize…", #selector(EditorViewController.showPosterize(_:))))
        menu.addItem(item("Threshold…", #selector(EditorViewController.showThreshold(_:))))
        menu.addItem(destructive(.gradientMap))
        menu.addItem(destructive(.selectiveColor))
        menu.addItem(.separator())
        // Desaturate takes ⇧⌘U, which was free.
        menu.addItem(
            item(
                "Desaturate", #selector(EditorViewController.applyDesaturate(_:)), "u",
                [.command, .shift]))
        menu.addItem(destructive(.replaceColor))
        menu.addItem(.separator())
        menu.addItem(destructive(.shadowsHighlights))
        // HDR Toning flattens and tone maps (EditorViewController+HdrToning.swift).
        menu.addItem(item("HDR Toning…", #selector(EditorViewController.showHdrToning(_:))))
        menu.addItem(destructive(.whiteBalance))
        menu.addItem(item("Hue Rotate…", #selector(EditorViewController.showHueRotate(_:))))
        menu.addItem(item("Grayscale", #selector(EditorViewController.applyGrayscale(_:))))
        menu.addItem(item("Sepia", #selector(EditorViewController.applySepia(_:))))
        return menu
    }

    /// Image > Mode: the document's colour profile (relabel vs. convert)
    /// and, one level down, the app-wide working space new opens are
    /// converted into. Neither profile command earns a key equivalent —
    /// they are deliberate, occasional acts. Enablement and the working
    /// space's check marks come from
    /// EditorViewController.validateUserInterfaceItem.
    private func modeMenu() -> NSMenu {
        let menu = NSMenu(title: "Mode")
        // #547: the colour mode's Grayscale, RGB Color and CMYK
        // Color items; the current one is checked
        // (EditorViewController+CmykMode.swift, +GrayMode.swift).
        // #547 Bitmap phase 1a: Bitmap, through its sheet (+BitmapMode.swift), above Grayscale.
        menu.addItem(item("Bitmap…", #selector(EditorViewController.convertToBitmapMode(_:))))
        menu.addItem(item("Grayscale", #selector(EditorViewController.convertToGrayscaleMode(_:))))
        // #547 Duotone phase 1a: Duotone, through its sheet (+DuotoneMode.swift).
        menu.addItem(item("Duotone…", #selector(EditorViewController.convertToDuotoneMode(_:))))
        // #547 Indexed phase 1a: Indexed Color, through its sheet (+IndexedMode.swift).
        menu.addItem(item("Indexed Color…", #selector(EditorViewController.convertToIndexedMode(_:))))
        menu.addItem(item("RGB Color…", #selector(EditorViewController.convertToRGBMode(_:))))
        menu.addItem(item("CMYK Color…", #selector(EditorViewController.convertToCMYKMode(_:))))
        // #547 Lab phase 1: Lab Color, converted at once (+LabMode.swift).
        menu.addItem(item("Lab Color", #selector(EditorViewController.convertToLabMode(_:))))
        menu.addItem(.separator())
        menu.addItem(item("Document Precision…", #selector(EditorViewController.showDocumentPrecision(_:))))
        menu.addItem(item("Convert Precision…", #selector(EditorViewController.convertDocumentPrecision(_:))))
        // #547 phase 5: 8/16 Bits/Channel, the current one checked
        // (EditorViewController+CmykProfile.swift): a CMYK document's depth,
        // HDR Toning at that depth for a 32-bit RGB one, or
        // Convert Precision on that mode for an 8- or 16-bit RGB one.
        menu.addItem(item("8 Bits/Channel", #selector(EditorViewController.convertToEightBits(_:))))
        menu.addItem(item("16 Bits/Channel", #selector(EditorViewController.convertToSixteenBits(_:))))
        menu.addItem(item("32 Bits/Channel…", #selector(EditorViewController.convertToFloatPrecision(_:))))
        menu.addItem(.separator())
        // #547 Indexed phase 1c: an Indexed document's palette (+ColorTable.swift).
        menu.addItem(item("Color Table…", #selector(EditorViewController.showColorTable(_:))))
        menu.addItem(.separator())
        menu.addItem(
            item("Assign Profile…", #selector(EditorViewController.assignProfile(_:))))
        menu.addItem(
            item("Convert to Profile…", #selector(EditorViewController.convertToProfile(_:))))
        menu.addItem(.separator())
        menu.addItem(submenuItem(workingSpaceMenu()))
        return menu
    }

    /// Image > Mode > Working Space: one check-marked radio item per
    /// WorkingSpace, its tag indexing `WorkingSpace.allCases` (the
    /// layerStyleMenu tag idiom). It affects FUTURE opens only.
    private func workingSpaceMenu() -> NSMenu {
        let menu = NSMenu(title: "Working Space")
        for (tag, space) in WorkingSpace.allCases.enumerated() {
            let entry = item(space.displayName, #selector(setWorkingSpace(_:)))
            entry.tag = tag
            menu.addItem(entry)
        }
        menu.addItem(.separator())
        menu.addItem(item("Ask When Pasting", #selector(toggleAskWhenPasting(_:))))
        return menu
    }

    private func channelsMenu() -> NSMenu {
        let menu = NSMenu(title: "Channels")
        menu.addItem(item("New Channel", #selector(EditorViewController.newChannel(_:))))
        // #547: the menu pair for spot colour channels.
        menu.addItem(item("New Spot Channel…", #selector(EditorViewController.newSpotChannel(_:))))
        menu.addItem(
            item("Spot Channel Options…", #selector(EditorViewController.spotChannelOptions(_:))))
        menu.addItem(
            item("Merge Spot Channel", #selector(EditorViewController.mergeSpotChannel(_:))))
        menu.addItem(
            item("Duplicate Channel", #selector(EditorViewController.duplicateChannel(_:))))
        menu.addItem(item("Delete Channel", #selector(EditorViewController.deleteChannel(_:))))
        menu.addItem(
            item("Channel Options…", #selector(EditorViewController.channelOptions(_:))))
        menu.addItem(item("Invert Channel", #selector(EditorViewController.invertChannel(_:))))
        menu.addItem(item("Modify Native Plane…", #selector(EditorViewController.modifyNativePlane(_:))))
        menu.addItem(.separator())
        menu.addItem(
            item(
                "Load Channel as Selection",
                #selector(EditorViewController.loadChannelAsSelection(_:))))
        return menu
    }
}
