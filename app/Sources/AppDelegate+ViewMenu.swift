import AppKit

extension AppDelegate {
    func viewMenu() -> NSMenu {
        let menu = NSMenu(title: "View")
        menu.addItem(item("Zoom In", #selector(EditorViewController.zoomInAction(_:)), "+"))
        // Hidden alternate so plain ⌘= also zooms in.
        let zoomInEquals = item("Zoom In", #selector(EditorViewController.zoomInAction(_:)), "=")
        zoomInEquals.isHidden = true
        zoomInEquals.allowsKeyEquivalentWhenHidden = true
        menu.addItem(zoomInEquals)
        menu.addItem(item("Zoom Out", #selector(EditorViewController.zoomOutAction(_:)), "-"))
        // ⌘1 (100%) with ⌥⌘0 as a hidden twin, and ⌘0 Fit.
        menu.addItem(item("Actual Size", #selector(EditorViewController.zoomActualAction(_:)), "1"))
        let actualSizeTwin = item(
            "Actual Size", #selector(EditorViewController.zoomActualAction(_:)), "0",
            [.command, .option])
        actualSizeTwin.isHidden = true
        actualSizeTwin.allowsKeyEquivalentWhenHidden = true
        menu.addItem(actualSizeTwin)
        menu.addItem(item("Zoom to Fit", #selector(EditorViewController.zoomFitAction(_:)), "0"))
        // View-only rotation (Phase 12D-2). No chord for Rotate View…: ⌥⌘R is
        // Select ▸ Select and Mask…. Reset keeps ⌃⌥⌘R.
        // On the event shape a keyboard actually delivers, a menu key
        // equivalent matches its chord EXACTLY — measured in both directions,
        // cases M1–M7 of `archive/phase-12e-layer-ops-status.md`, "The
        // measured Shift rule". The Shift leniency 12D-2 inferred is a
        // property of hand-built `NSEvent.keyEvent` presses only, which is
        // also why the 9H shortcut probe's own test chord moved to ⌥⇧⌘K: that
        // is the probe's synthesized-event problem, not a leniency in AppKit.
        menu.addItem(
            item("Rotate View…", #selector(EditorViewController.rotateView(_:))))
        menu.addItem(
            item("Reset View Rotation", #selector(EditorViewController.resetViewRotation(_:)),
                 "r", [.command, .control, .option]))
        menu.addItem(.separator())
        // Soft proofing (Phase 11C) on ⌘Y and ⇧⌘Y, which were both free.
        // Proof Setup ▸ Custom… / saved presets (AppDelegate+ProofPresets).
        menu.addItem(submenuItem(proofSetupMenu()))
        menu.addItem(
            item("Proof Colors", #selector(EditorViewController.toggleProofColors(_:)), "y"))
        menu.addItem(
            item(
                "Gamut Warning", #selector(EditorViewController.toggleGamutWarning(_:)), "y",
                [.command, .shift]))
        // The persistent clipping overlay, a view preference.
        // ⌥⌘J was free.
        menu.addItem(
            item(
                "Show Clipping", #selector(EditorViewController.toggleShowClipping(_:)), "j",
                [.command, .option]))
        // A view-only exposure for a 32-bit float window, in stops. No
        // key equivalent (EditorViewController+ViewExposure.swift).
        menu.addItem(
            item("32-bit Exposure…", #selector(EditorViewController.showViewExposure(_:))))
        menu.addItem(
            item("Reset 32-bit Exposure", #selector(EditorViewController.resetViewExposure(_:))))
        // v1.x: Show HDR, per window, checked when on
        // (EditorViewController+HDRPreview.swift).
        menu.addItem(item("HDR Preview", #selector(EditorViewController.toggleHDRPreview(_:))))
        menu.addItem(.separator())
        // Title toggles between Show/Hide Panels in validateUserInterfaceItem.
        // Deliberately no Tab key equivalent: a modifier-less equivalent is
        // resolved ahead of the first responder and would steal Tab from
        // every text field, so the canvas handles Tab in keyDown instead.
        let hidePanels = item("Hide Panels", #selector(EditorViewController.togglePanels(_:)))
        hidePanels.toolTip =
            "Tab on the canvas hides or shows the tool rail, the options bar and the side panels."
        menu.addItem(hidePanels)
        // Title toggles between Show/Hide Layers in validateUserInterfaceItem.
        // F7 (U+F70A, NSF7FunctionKey).
        let hideLayers = item(
            "Hide Layers", #selector(EditorViewController.toggleLayersPanel(_:)), "\u{F70A}", [])
        hideLayers.toolTip = "Shift-Tab on the canvas also hides or shows the side panels."
        menu.addItem(hideLayers)
        menu.addItem(
            item(
                "Channels", #selector(EditorViewController.showChannels(_:)), "c",
                [.command, .control]))
        menu.addItem(
            item(
                "Assistant", #selector(EditorViewController.showAssistant(_:)), "a",
                [.command, .control]))
        // F8 (U+F70B, NSF8FunctionKey).
        menu.addItem(
            item("Info", #selector(EditorViewController.showInfo(_:)), "\u{F70B}", []))
        // The View menu's own convention again (Channels ⌃⌘C, Assistant
        // ⌃⌘A): ⌃⌘N was free.
        menu.addItem(
            item(
                "Navigator", #selector(EditorViewController.showNavigator(_:)), "n",
                [.command, .control]))
        menu.addItem(.separator())
        // ⌘R (`designs/v1-shortcuts-fade.md`).
        menu.addItem(item("Rulers", #selector(EditorViewController.toggleRulers(_:)), "r"))
        menu.addItem(submenuItem(rulerUnitsMenu()))
        menu.addItem(.separator())
        menu.addItem(
            item("Show Guides", #selector(EditorViewController.toggleGuides(_:)), ";"))
        menu.addItem(
            item(
                "Lock Guides", #selector(EditorViewController.toggleLockGuides(_:)), ";",
                [.command, .option]))
        menu.addItem(item("Clear Guides", #selector(EditorViewController.clearGuides(_:))))
        menu.addItem(item("New Guide…", #selector(EditorViewController.newGuide(_:))))
        menu.addItem(item("Show Color Samplers", #selector(EditorViewController.toggleColorSamplers(_:))))
        menu.addItem(item("Clear Color Samplers", #selector(EditorViewController.clearColorSamplers(_:))))
        menu.addItem(submenuItem(guideColorMenu()))
        menu.addItem(.separator())
        menu.addItem(item("Show Grid", #selector(EditorViewController.toggleGrid(_:)), "'"))
        menu.addItem(submenuItem(gridSpacingMenu()))
        menu.addItem(submenuItem(gridSubdivisionsMenu()))
        menu.addItem(.separator())
        menu.addItem(
            item(
                "Snap", #selector(EditorViewController.toggleSnap(_:)), ";",
                [.command, .shift]))
        menu.addItem(submenuItem(snapToMenu()))
        return menu
    }

    /// View ▸ Ruler Units: one radio item per CanvasUnit, its tag indexing
    /// `CanvasUnit.allCases` (the workingSpaceMenu idiom).
    private func rulerUnitsMenu() -> NSMenu {
        let menu = NSMenu(title: "Ruler Units")
        for (tag, unit) in CanvasUnit.allCases.enumerated() {
            let entry = item(unit.displayName, #selector(EditorViewController.setRulerUnit(_:)))
            entry.tag = tag
            menu.addItem(entry)
        }
        return menu
    }

    /// View ▸ Guide Color, tag-indexing `GuideColor.allCases`.
    private func guideColorMenu() -> NSMenu {
        let menu = NSMenu(title: "Guide Color")
        for (tag, color) in GuideColor.allCases.enumerated() {
            let entry = item(
                color.displayName, #selector(EditorViewController.setGuideColor(_:)))
            entry.tag = tag
            menu.addItem(entry)
        }
        return menu
    }

    /// View ▸ Grid Spacing — presets in the CURRENT ruler unit, so the same
    /// 100 is 100 px, 100 in or 100 % depending on View ▸ Ruler Units.
    private func gridSpacingMenu() -> NSMenu {
        let menu = NSMenu(title: "Grid Spacing")
        for (tag, spacing) in CanvasGrid.spacingPresets.enumerated() {
            let entry = item(
                "\(Int(spacing))", #selector(EditorViewController.setGridSpacing(_:)))
            entry.tag = tag
            menu.addItem(entry)
        }
        return menu
    }

    private func gridSubdivisionsMenu() -> NSMenu {
        let menu = NSMenu(title: "Grid Subdivisions")
        for (tag, count) in CanvasGrid.subdivisionPresets.enumerated() {
            let entry = item(
                "\(count)", #selector(EditorViewController.setGridSubdivisions(_:)))
            entry.tag = tag
            menu.addItem(entry)
        }
        return menu
    }

    /// View ▸ Snap To: five independent bits plus All / None — the Layer ▸
    /// Lock template with a different set, so five toggles cost ONE selector
    /// (the tag indexes `SnapTarget.named`).
    private func snapToMenu() -> NSMenu {
        let menu = NSMenu(title: "Snap To")
        for (tag, target) in SnapTarget.named.enumerated() {
            let entry = item(
                target.name, #selector(EditorViewController.toggleSnapTarget(_:)))
            entry.tag = tag
            menu.addItem(entry)
        }
        menu.addItem(.separator())
        menu.addItem(item("All", #selector(EditorViewController.snapToAll(_:))))
        menu.addItem(item("None", #selector(EditorViewController.snapToNone(_:))))
        return menu
    }
}
