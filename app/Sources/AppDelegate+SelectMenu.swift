import AppKit

extension AppDelegate {
    /// Top-level Select menu (after Layer, before Filters).
    /// Enablement comes from EditorViewController.validateUserInterfaceItem,
    /// keyed on the action selectors — so these items validate identically
    /// wherever they live.
    func selectMenu() -> NSMenu {
        let menu = NSMenu(title: "Select")
        menu.addItem(item("Select All", #selector(EditorViewController.selectAll(_:)), "a"))
        menu.addItem(item("Deselect", #selector(EditorViewController.deselect(_:)), "d"))
        menu.addItem(
            item(
                "Invert Selection", #selector(EditorViewController.invertSelection(_:)), "i",
                [.command, .shift]))
        menu.addItem(.separator())
        menu.addItem(item("Reselect", #selector(EditorViewController.reselect(_:)), "d", [.command, .shift]))
        menu.addItem(item("Color Range…", #selector(EditorViewController.showColorRange(_:))))
        menu.addItem(item("Focus Area…", #selector(EditorViewController.showFocusArea(_:))))
        menu.addItem(item("Transform Selection…", #selector(EditorViewController.transformSelection(_:))))
        menu.addItem(
            item(
                "Select and Mask…", #selector(EditorViewController.refineSelection(_:)), "r",
                [.command, .option]))
        menu.addItem(item("Select Objects…", #selector(EditorViewController.showSubjectTargets(_:))))
        menu.addItem(item("Select People…", #selector(EditorViewController.showPeopleTargets(_:))))
        menu.addItem(item("All People", #selector(EditorViewController.selectPeople(_:))))
        menu.addItem(item("Face Parts…", #selector(EditorViewController.showFaceParts(_:))))
        // MediaPipe's hair, skin and clothes classes per person (PersonParts.swift).
        menu.addItem(item("Person Parts…", #selector(EditorViewController.showPersonParts(_:))))
        // Vision segmentation; needs no selection to start from, so it sits
        // with Select All rather than with the modify-the-selection block.
        menu.addItem(
            item("Select Subject", #selector(EditorViewController.selectSubject(_:))))
        // The sky model's soft matte, refined (ModelSelection.swift).
        menu.addItem(item("Sky", #selector(EditorViewController.selectSky(_:))))
        menu.addItem(.separator())
        menu.addItem(submenuItem(modifySelectionMenu()))
        menu.addItem(item("Grow", #selector(EditorViewController.selectGrow(_:))))
        menu.addItem(item("Similar", #selector(EditorViewController.selectSimilar(_:))))
        menu.addItem(.separator())
        // Selections ⇄ alpha channels, and the nine luminosity masks the
        // composite's luma builds.
        menu.addItem(
            item("Save Selection…", #selector(EditorViewController.saveSelectionSheet(_:))))
        menu.addItem(
            item("Load Selection…", #selector(EditorViewController.loadSelectionSheet(_:))))
        // The selection's outline traced into the work path
        // (EditorViewController+Paths.swift, MCP `selection_to_path`).
        menu.addItem(item("Make Work Path…", #selector(EditorViewController.makeWorkPath(_:))))
        menu.addItem(submenuItem(portraitMatteMenu()))
        menu.addItem(.separator())
        menu.addItem(
            item("Add Luminosity Masks", #selector(EditorViewController.addLuminosityMasks(_:))))
        menu.addItem(.separator())
        // Deliberately NO key equivalent: the bare Q toggles the mode from
        // the canvas's keyDown alongside the tool keys, so it can never
        // steal the letter from text editing (the Edit > Clear ⌫ hazard).
        menu.addItem(
            item("Quick Mask Mode", #selector(EditorViewController.toggleQuickMask(_:))))
        return menu
    }

    /// Select ▸ Modify, on the unchanged selectors
    /// (Shortcuts.swift keys its bindings by selector).
    private func modifySelectionMenu() -> NSMenu {
        let menu = NSMenu(title: "Modify")
        menu.addItem(item("Border…", #selector(EditorViewController.borderSelection(_:))))
        menu.addItem(item("Smooth…", #selector(EditorViewController.smoothSelection(_:))))
        menu.addItem(item("Expand…", #selector(EditorViewController.growSelection(_:))))
        menu.addItem(item("Contract…", #selector(EditorViewController.shrinkSelection(_:))))
        // ⇧F6 (U+F709, NSF6FunctionKey): Feather.
        menu.addItem(
            item(
                "Feather…", #selector(EditorViewController.featherSelection(_:)), "\u{F709}",
                [.shift]))
        return menu
    }

    /// Select > Portrait Matte: an iPhone photo's embedded mattes,
    /// each loaded as the selection. The item's TAG is the `PortraitMatte`
    /// raw value, so one editor handler serves them all; no key equivalents.
    private func portraitMatteMenu() -> NSMenu {
        let menu = NSMenu(title: "Portrait Matte")
        for matte in PortraitMatte.allCases {
            let entry = item(matte.title, #selector(EditorViewController.loadPortraitMatte(_:)))
            entry.tag = matte.rawValue
            menu.addItem(entry)
        }
        return menu
    }
}
