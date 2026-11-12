import AppKit

extension AppDelegate {
    func editMenu() -> NSMenu {
        let menu = NSMenu(title: "Edit")
        menu.addItem(item("Undo", Selector(("undo:")), "z"))
        menu.addItem(item("Redo", Selector(("redo:")), "z", [.command, .shift]))
        // ⇧⌘F: the last filter, adjustment, stroke or fill blended
        // with the state before it (EditorViewController+Fade.swift), retitled
        // "Fade <step>…" by its validation.
        menu.addItem(
            item("Fade…", #selector(EditorViewController.showFade(_:)), "f", [.command, .shift]))
        menu.addItem(.separator())
        menu.addItem(item("Cut", #selector(EditorViewController.cut(_:)), "x"))
        menu.addItem(item("Copy", #selector(EditorViewController.copy(_:)), "c"))
        // Copy takes the active layer, Copy Merged the flattened composite.
        menu.addItem(
            item(
                "Copy Merged", #selector(EditorViewController.copyMerged(_:)), "c",
                [.command, .shift]))
        menu.addItem(
            item("Paste", #selector(EditorViewController.paste(_:)), "v"))
        // The Paste Special submenu (EditorViewController+Clipboard.swift):
        // Paste in Place (Phase 12G-1: the typed payload at the offset it was
        // copied from), and Paste Into / Paste Outside (centred on
        // the selection, masked by it or by its inverse).
        let pasteSpecial = NSMenu(title: "Paste Special")
        pasteSpecial.addItem(
            item(
                "Paste in Place", #selector(EditorViewController.pasteInPlace(_:)), "v",
                [.command, .shift]))
        pasteSpecial.addItem(
            item(
                "Paste Into", #selector(EditorViewController.pasteInto(_:)), "v",
                [.command, .option, .shift]))
        pasteSpecial.addItem(
            item("Paste Outside", #selector(EditorViewController.pasteOutside(_:))))
        menu.addItem(submenuItem(pasteSpecial))
        // Bare ⌫: U+0008 (NSBackspaceCharacter) is what AppKit matches the
        // Delete key against and what it draws as ⌫ (U+007F is forward
        // delete, ⌦). A modifier-less key equivalent is resolved ahead of the
        // first responder, so EditorViewController.validateUserInterfaceItem
        // keeps this item disabled unless there is something to clear and no
        // text is being edited — otherwise it would swallow every Delete.
        menu.addItem(
            item("Clear", #selector(EditorViewController.clearSelection(_:)), "\u{8}", []))
        menu.addItem(.separator())
        // Edit ▸ Fill… on ⇧F5 (U+F708, NSF5FunctionKey),
        // with its second chord ⇧⌫ as a hidden twin. Every ⌫ chord here
        // stands down while ⌫ belongs to a field or the canvas
        // (`deleteChordAllowed`, EditorViewController+FillStroke.swift).
        menu.addItem(
            item("Fill…", #selector(EditorViewController.fillSelection(_:)), "\u{F708}", [.shift]))
        let fillFromDelete = item(
            "Fill…", #selector(EditorViewController.fillSelectionFromDeleteKey(_:)), "\u{8}",
            [.shift])
        fillFromDelete.isHidden = true
        fillFromDelete.allowsKeyEquivalentWhenHidden = true
        menu.addItem(fillFromDelete)
        // The four fills with the colour pair, ⌥⌫ / ⌘⌫, and
        // with ⇧ keeping the layer's transparency. Plain visible items, so
        // each stays customizable in the Shortcuts sheet.
        menu.addItem(
            item(
                "Fill with Foreground Color",
                #selector(EditorViewController.fillWithForeground(_:)), "\u{8}", [.option]))
        menu.addItem(
            item(
                "Fill with Background Color",
                #selector(EditorViewController.fillWithBackground(_:)), "\u{8}", [.command]))
        menu.addItem(
            item(
                "Fill with Foreground Color, Preserve Transparency",
                #selector(EditorViewController.fillWithForegroundPreserving(_:)), "\u{8}",
                [.shift, .option]))
        menu.addItem(
            item(
                "Fill with Background Color, Preserve Transparency",
                #selector(EditorViewController.fillWithBackgroundPreserving(_:)), "\u{8}",
                [.shift, .command]))
        // Edit ▸ Stroke…: the selection's band through Fill's
        // commit (EditorViewController+FillStroke.swift). No key equivalent;
        // enabled only with a selection, at 8 bits.
        menu.addItem(item("Stroke…", #selector(EditorViewController.strokeSelection(_:))))
        // Edit ▸ Define Pattern…: the composite in a rectangular selection (or the canvas) into
        // the user's pattern library (EditorViewController+DefinePattern.swift).
        menu.addItem(
            item("Define Pattern…", #selector(EditorViewController.definePattern(_:))))
        // Edit ▸ Patterns: delete, import and export that library
        // (EditorViewController+PatternLibrary.swift).
        let patterns = NSMenu(title: "Patterns")
        patterns.addItem(
            item("Delete Pattern…", #selector(EditorViewController.deleteLibraryPattern(_:))))
        patterns.addItem(
            item("Import Patterns…", #selector(EditorViewController.importPatternLibrary(_:))))
        patterns.addItem(
            item("Export Patterns…", #selector(EditorViewController.exportPatternLibrary(_:))))
        menu.addItem(submenuItem(patterns))
        // Content-Aware Fill keeps its own item, after the fills.
        menu.addItem(
            item(
                "Content-Aware Fill…",
                #selector(EditorViewController.contentAwareFill(_:))))
        // Edit ▸ Generative Fill…: the selection's crop to a cloud provider with the
        // user's own key, onto a new masked layer (GenerativeFillSheet.swift).
        menu.addItem(item("Generative Fill…", #selector(EditorViewController.showGenerativeFill(_:))))
        // Edit ▸ Generative Expand…: the area Canvas Size left transparent, filled
        // the same way (GenerativeFillSheet.swift).
        menu.addItem(item("Generative Expand…", #selector(EditorViewController.showGenerativeExpand(_:))))
        // Edit ▸ Puppet Warp: an on-canvas session on the active layer
        // or the selection's float (EditorViewController+PuppetWarp.swift).
        menu.addItem(.separator())
        menu.addItem(item("Puppet Warp", #selector(EditorViewController.showPuppetWarp(_:))))
        // Edit ▸ Perspective Warp: Layout, then Warp, on the same
        // targets (EditorViewController+PerspectiveWarp.swift).
        menu.addItem(item("Perspective Warp", #selector(EditorViewController.showPerspectiveWarp(_:))))
        // Edit ▸ Auto-Align Layers…: the projection dialog, then
        // the selected layers registered to the bottom one and warped (AutoAlignSheetController).
        menu.addItem(item("Auto-Align Layers…", #selector(EditorViewController.autoAlignLayers(_:))))
        // Edit ▸ Auto-Blend Layers…: Panorama's seams or Stack
        // Images' focus stack as masks on the selected layers (AutoBlendSheetController).
        menu.addItem(
            item("Auto-Blend Layers…", #selector(EditorViewController.showAutoBlendLayers(_:))))
        // Edit ▸ Sky Replacement…: the sky model's mask, a library or user sky, one new
        // layer (SkyReplacementSheet.swift).
        menu.addItem(item("Sky Replacement…", #selector(EditorViewController.showSkyReplacement(_:))))
        // Edit ▸ Color Settings… on ⇧⌘K (#1 phase 5): the working spaces, policies
        // and conversion options (ColorSettingsWindowController.swift). On AppDelegate, so it
        // works with no document open.
        menu.addItem(.separator())
        menu.addItem(item("Color Settings…", #selector(AppDelegate.showColorSettings(_:)), "k",
                          [.command, .shift]))
        return menu
    }
}
