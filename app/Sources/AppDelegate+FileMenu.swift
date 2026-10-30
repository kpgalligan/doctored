import AppKit

extension AppDelegate {
    func fileMenu() -> NSMenu {
        let menu = NSMenu(title: "File")
        // The chords (`designs/v1-shortcuts-fade.md`): File ▸ New… is ⌘N,
        // and New from Clipboard takes ⌥⌘N. A user's stored override still
        // wins over either.
        menu.addItem(item("New…", #selector(newDocument(_:)), "n"))
        menu.addItem(
            item("New from Clipboard", #selector(newFromClipboard(_:)), "n", [.command, .option]))
        menu.addItem(item("Open…", #selector(NSDocumentController.openDocument(_:)), "o"))
        // The internal Phase 9I annotation experiment; the item only exists
        // when AssistedAnnotation.isEnabled, so an ordinary build has no
        // entry point to it at all.
        if AssistedAnnotation.isEnabled {
            menu.addItem(
                item("Open Assisted Annotation Case…", #selector(openAssistedAnnotationCase(_:))))
        }

        // AppKit auto-populates a submenu whose sole item's action is
        // clearRecentDocuments: as the Open Recent menu.
        let openRecent = NSMenu(title: "Open Recent")
        openRecent.addItem(
            item("Clear Menu", #selector(NSDocumentController.clearRecentDocuments(_:))))
        let openRecentItem = NSMenuItem(title: "Open Recent", action: nil, keyEquivalent: "")
        openRecentItem.submenu = openRecent
        menu.addItem(openRecentItem)
        // v1.x: the open-as-HDR choice, an app-wide preference
        // beside Open (HDRPhotoRequest.swift); checked when on, the default.
        menu.addItem(item("Open HDR Photos as HDR", #selector(toggleOpenHDRPhotosAsHDR(_:))))
        // Several files as the layers of one new document.
        menu.addItem(item("Load Files into Stack…", #selector(loadFilesIntoStack(_:))))
        // Bracketed exposures as one new 32-bit float document.
        menu.addItem(item("Merge to HDR…", #selector(mergeToHDR(_:))))

        // File ▸ Automate — Actions, Batch and Merge Panorama. All five live on AppDelegate
        // rather than the editor because `EditorViewController
        // .validateUserInterfaceItem` returns false at its first line with no
        // document open, and every one of these must work with none.
        let automate = NSMenu(title: "Automate")
        // ⌥F9 (U+F70C, NSF9FunctionKey): the Actions window.
        automate.addItem(item("Actions…", #selector(showActions(_:)), "\u{F70C}", [.option]))
        // The title flips to Stop Recording.
        automate.addItem(item("Start Recording", #selector(toggleActionRecording(_:))))
        // …and this one to Play "<name>".
        automate.addItem(item("Play Last Action", #selector(playLastAction(_:))))
        automate.addItem(.separator())
        automate.addItem(item("Batch…", #selector(showBatch(_:))))
        // A one-dialog panorama, a new document.
        automate.addItem(item("Merge Panorama…", #selector(mergePanorama(_:))))
        let automateItem = NSMenuItem(title: "Automate", action: nil, keyEquivalent: "")
        automateItem.submenu = automate
        menu.addItem(automateItem)

        menu.addItem(.separator())
        menu.addItem(item("Close", #selector(NSWindow.performClose(_:)), "w"))
        let recover = item("Recover Unsaved Work…", #selector(RecoveryCoordinator.showRecoveryWindow(_:)))
        recover.target = RecoveryCoordinator.shared
        menu.addItem(recover)
        menu.addItem(item("Save", #selector(NSDocument.save(_:)), "s"))
        menu.addItem(item("Save As…", #selector(NSDocument.saveAs(_:)), "s", [.command, .shift]))
        // F12 (U+F70F, NSF12FunctionKey): Revert.
        menu.addItem(
            item("Revert to Saved", #selector(NSDocument.revertToSaved(_:)), "\u{F70F}", []))
        menu.addItem(.separator())
        // ⌥⇧⌘W: Export…; ⌘E is Layer ▸ Merge Down.
        menu.addItem(
            item(
                "Export…", #selector(EditorViewController.showExportWorkspace(_:)), "w",
                [.command, .option, .shift]))
        // The default preset with no dialog; it reports the path it wrote.
        menu.addItem(
            item(
                "Quick Export", #selector(EditorViewController.quickExport(_:)), "e",
                [.option, .command]))
        // Phase 11B production outputs, after Quick Export.
        menu.addItem(
            item("Export Layers…", #selector(EditorViewController.exportLayers(_:))))
        menu.addItem(
            item("Contact Sheet…", #selector(EditorViewController.showContactSheet(_:))))
        menu.addItem(item("Export as PSD…", #selector(ImageDocument.exportPSD(_:))))
        // Phase 11D: one flattened page, at the document's own print size.
        menu.addItem(item("Export as PDF…", #selector(EditorViewController.exportAsPDF(_:))))
        menu.addItem(.separator())
        // The creator, copyright notice, description and keywords the
        // document carries into its exports, on ⌥⇧⌘I.
        menu.addItem(
            item(
                "File Info…", #selector(EditorViewController.showFileInfo(_:)), "i",
                [.command, .option, .shift]))
        menu.addItem(.separator())
        // Both selectors are NSDocument's own and reach the document through
        // the responder chain; ImageDocument.validateUserInterfaceItem gates
        // them on there being an image.
        menu.addItem(
            item(
                "Page Setup…", #selector(NSDocument.runPageLayout(_:)), "p",
                [.command, .shift]))
        menu.addItem(item("Print…", #selector(NSDocument.printDocument(_:)), "p"))
        return menu
    }
}
