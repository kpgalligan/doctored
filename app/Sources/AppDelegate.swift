import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuItemValidation {
    func applicationWillFinishLaunching(_ notification: Notification) {
        // Resolves the working limit and hands it to core before any document work.
        _ = RasterResources.shared
        WorkingCMYK.install()
        AutomationAlerts.installIfEnabled()
        BackgroundLaunch.applyActivationPolicy()
        NSApp.mainMenu = makeMainMenu()
        // Debug-only capture of the menu tree and the tool sets (RegistryDump.swift).
        #if DEBUG
        RegistryDump.writeIfRequested()
        #endif
        // Gradient end colors (and brush colors) may carry alpha.
        NSColorPanel.shared.showsAlpha = true
    }

    // AppKit calls this only when the app is launched or reactivated with no
    // documents to open, so it can't race launch-time file opens the way a
    // check in applicationDidFinishLaunching does. There is no blank-canvas
    // "New" document; show the welcome window (design proposal 4) instead
    // of an untitled file.
    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool {
        WelcomeWindowController.shared.show()
        return false
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        RecoveryCoordinator.shared.start()
        AgentServer.shared.bindSocket()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationWillTerminate(_ notification: Notification) {
        AgentServer.shared.unbindSocket()
        Clipboard.releasePrivatePasteboard()
        // A quit approved from any other window still records the annotation
        // session it interrupted, with no prompt (see the controller's header;
        // there is deliberately no applicationShouldTerminate(_:) here).
        AssistedAnnotationWindowController.current?.finishForTermination()
    }

    // MARK: - New from Clipboard

    /// Opens the frontmost pasteboard image as a new untitled document
    /// (⌥⌘N; Preview's ⌘N behavior). `RasterImage.fromPasteboard` decodes the
    /// bitmap into its own colour space and reports the profile those
    /// numbers belong to, so the document is tagged and then adopted into
    /// the working space exactly as an opened file is.
    /// File ▸ New… (⌘N): a document with no seed image. It lives here, not
    /// on `EditorViewController`, for `newFromClipboard`'s reason — it has to
    /// work at the Welcome window, before any document exists.
    @objc func newDocument(_ sender: Any?) {
        guard let request = NewDocumentSheetController.run() else { return }
        do {
            let document = try ImageDocument.makeNew(request)
            NSDocumentController.shared.addDocument(document)
            document.makeWindowControllers()
            document.showWindows()
        } catch {
            let alert = NSAlert()
            alert.messageText = "Could Not Create the Document"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }

    @objc func newFromClipboard(_ sender: Any?) {
        NewFromClipboard.open()
    }

    // MARK: - Working space

    /// Image > Mode > Working Space ▸ — the profile FUTURE opens are
    /// converted into. The sender's tag indexes `WorkingSpace.allCases`,
    /// the layer-style menu's idiom, so the menu needs no parallel list to
    /// stay in step with the enum.
    ///
    /// It lives HERE and not on `EditorViewController` — where the rest of
    /// Image > Mode lives — because it is an app-wide preference that by
    /// design touches no document: it has to be settable at the Welcome
    /// window, before the first open, which is exactly the moment a
    /// document-scoped responder does not exist. Being the only responder
    /// that implements it also keeps it a single implementation, whether or
    /// not a document window is key.
    @objc func setWorkingSpace(_ sender: Any?) {
        let spaces = WorkingSpace.allCases
        let tag = (sender as? NSMenuItem)?.tag ?? -1
        guard spaces.indices.contains(tag) else {
            NSSound.beep()
            return
        }
        ColorSettings.workingSpace = spaces[tag]
    }

    /// Image > Mode > Working Space ▸ Ask When Pasting: the Ask When
    /// Pasting profile-mismatch policy. An app-wide
    /// preference beside the working space, for the same reason it lives
    /// here (`setWorkingSpace`).
    @objc func toggleAskWhenPasting(_ sender: Any?) {
        ColorSettings.askWhenPasting.toggle()
    }

    /// Edit ▸ Color Settings… (#1 phase 5): the working spaces, the colour-management policies
    /// and the conversion options. Here, beside the working space, for its reason: an app-wide
    /// preference settable before the first open.
    @objc func showColorSettings(_ sender: Any?) {
        ColorSettingsWindowController.run()
    }

    /// File ▸ Open HDR Photos as HDR (v1.x): a gain-map or PQ/HLG photo
    /// opens as a 32-bit float document when on (the default), its SDR base
    /// (gain map) or a refusal (PQ/HLG) when off. App-wide, like Ask When
    /// Pasting; MCP `open_document hdr_photo` overrides it for one open.
    @objc func toggleOpenHDRPhotosAsHDR(_ sender: Any?) {
        HDRPhotoRequest.preference = HDRPhotoRequest.preference == .hdr ? .sdr : .hdr
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if item.action == #selector(newDocument(_:)) { return true }
        if item.action == #selector(toggleOpenHDRPhotosAsHDR(_:)) {
            item.state = HDRPhotoRequest.preference == .hdr ? .on : .off
            return true
        }
        if item.action == #selector(toggleAskWhenPasting(_:)) {
            item.state = ColorSettings.askWhenPasting ? .on : .off
            return true
        }
        if item.action == #selector(newFromClipboard(_:)) {
            return NewFromClipboard.isAvailable
        }
        if item.action == #selector(setWorkingSpace(_:)) {
            // Radio marks against the stored preference, so the menu shows
            // which space new opens use — with or without a document.
            let spaces = WorkingSpace.allCases
            item.state = spaces.indices.contains(item.tag)
                && spaces[item.tag] == ColorSettings.workingSpace ? .on : .off
            return true
        }
        // The four File ▸ Automate items and the palette, all of which must
        // work with no document open — which is why they are here and not in
        // the editor's table.
        if item.action == #selector(toggleActionRecording(_:)) {
            item.title = ActionRecorder.shared.isRecording ? "Stop Recording" : "Start Recording"
            return true
        }
        if item.action == #selector(playLastAction(_:)) {
            // The SUMMARY only. This runs on every File-menu update, and
            // decoding the steps of the last recorded paint session to put a
            // name in a menu item was the single worst stall in the feature
            // (Action.decodeSummary). The steps are read by the command
            // itself, once, when it is actually chosen.
            let last = ActionLibrary.mostRecentlyModified()
            item.title = last.map { "Play “\($0.summary.name)”" } ?? "Play Last Action"
            // The SAME lookup the command itself uses, and the same one the
            // Actions window's Play button uses: `currentDocument` is nil
            // whenever a non-document window is main — including the Actions
            // window this item is meant to be used beside — and this item
            // used to grey itself out with a document plainly open behind it.
            return last != nil && ActionPlayer.frontDocument() != nil
        }
        return true
    }

    // MARK: - Automate (File ▸ Automate, Tools ▸ Command Palette…)

    /// File ▸ Automate ▸ Actions… — the app-wide library window.
    @objc func showActions(_ sender: Any?) {
        ActionsWindowController.shared.show()
    }

    /// File ▸ Automate ▸ Start/Stop Recording, and the Actions window's own
    /// Record and Stop buttons, which target this selector through the
    /// responder chain. Stopping opens the library window, which is where the
    /// recording is named and kept.
    ///
    /// Starting DISCARDS a stopped-but-unsaved recording, so it asks first:
    /// those steps live only in memory, no file holds them, and no undo
    /// covers Application Support. The window lists them as "Unsaved
    /// recording — N steps" and its Save Recording button is the only way to
    /// keep them, so the alert points at exactly that.
    @objc func toggleActionRecording(_ sender: Any?) {
        let recorder = ActionRecorder.shared
        if recorder.isRecording {
            recorder.stop()
        } else {
            guard confirmDiscardingRecording() else { return }
            recorder.start()
        }
        ActionsWindowController.shared.show()
    }

    /// True when recording may start: nothing is pending, or the user said to
    /// throw it away.
    private func confirmDiscardingRecording() -> Bool {
        let recorder = ActionRecorder.shared
        guard recorder.hasUnsavedRecording else { return true }
        let count = recorder.steps.count
        let alert = NSAlert()
        alert.messageText =
            "Discard the unsaved recording of \(count) step\(count == 1 ? "" : "s")?"
        alert.informativeText =
            "Starting a new recording replaces it. To keep it, cancel and use Save Recording "
            + "in the Actions window."
        alert.addButton(withTitle: "Discard and Record")
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }

    /// File ▸ Automate ▸ Play "<name>" — the most recently modified action,
    /// on the frontmost document, one History state per step that changed
    /// something.
    ///
    /// The steps are read HERE, by the file the menu item named, and not by
    /// name: two files may claim one display name only through a hand-edit,
    /// and the item must play the one it was titled after. A file whose
    /// header read cleanly can still fail to decode — a malformed symbol, a
    /// non-finite number — and that reason is shown rather than beeped away,
    /// since it names the step and the argument that need fixing.
    @objc func playLastAction(_ sender: Any?) {
        guard let last = ActionLibrary.mostRecentlyModified(),
              let document = ActionPlayer.frontDocument()
        else {
            NSSound.beep()
            return
        }
        let action: Action
        switch ActionLibrary.action(at: last.url) {
        case .ok(let decoded):
            action = decoded
        case .broken(let file, let reason):
            NSSound.beep()
            let alert = NSAlert()
            alert.messageText = "“\(last.summary.name)” could not be read"
            alert.informativeText = "\(file): \(reason)"
            alert.runModal()
            return
        }
        // The same prompt every other Play path puts up
        // (`ActionParametersSheet`); no sheet at all for an action that
        // declares no parameters.
        guard let parameters = ActionParametersSheet.values(for: action) else { return }
        let report = ActionPlayer.run(
            action, on: document, parameters: parameters, stopOnError: nil,
            progress: ActionRunProgress(), prompt: ActionFailurePrompt.ask,
            history: .perStep)
        ActionPlayer.notePlayed(action, report, on: document)
        // A failure the user just answered through the prompt does not beep.
        if report.needsAttention { NSSound.beep() }
    }

    /// File ▸ Automate ▸ Batch…
    @objc func showBatch(_ sender: Any?) {
        BatchWindowController.present()
    }

    /// Tools ▸ Command Palette… (⌘F).
    @objc func showCommandPalette(_ sender: Any?) {
        CommandPaletteWindowController.shared.show()
    }

    // MARK: - Menu construction

    /// What this build's menus ARE, before any user override. Internal, not
    /// private: `AppDelegate+Shortcuts.makeMainMenu()` is the one caller and
    /// wraps it with `ShortcutMenu.installStored`.
    /// Each top-level builder lives in its own `AppDelegate+<Menu>Menu.swift`;
    /// this function is their one ordered list.
    func buildMainMenu() -> NSMenu {
        let mainMenu = NSMenu(title: "MainMenu")
        mainMenu.addItem(submenuItem(appMenu()))
        mainMenu.addItem(submenuItem(fileMenu()))
        mainMenu.addItem(submenuItem(editMenu()))
        mainMenu.addItem(submenuItem(imageMenu()))
        mainMenu.addItem(submenuItem(layerMenu()))
        mainMenu.addItem(submenuItem(selectMenu()))
        mainMenu.addItem(submenuItem(filtersMenu()))
        mainMenu.addItem(submenuItem(toolsMenu()))
        mainMenu.addItem(submenuItem(viewMenu()))

        let windowMenu = self.windowMenu()
        mainMenu.addItem(submenuItem(windowMenu))
        NSApp.windowsMenu = windowMenu

        let helpMenu = self.helpMenu()
        mainMenu.addItem(submenuItem(helpMenu))
        NSApp.helpMenu = helpMenu

        return mainMenu
    }

    // Internal, like `buildMainMenu` above: the `AppDelegate+<Menu>Menu.swift`
    // builders and the +ProofPresets and +Workspaces files use the same two helpers.
    func submenuItem(_ menu: NSMenu) -> NSMenuItem {
        let item = NSMenuItem(title: menu.title, action: nil, keyEquivalent: "")
        item.submenu = menu
        return item
    }

    func item(
        _ title: String,
        _ action: Selector?,
        _ key: String = "",
        _ modifiers: NSEvent.ModifierFlags = [.command]
    ) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        if !key.isEmpty {
            item.keyEquivalentModifierMask = modifiers
        }
        return item
    }
}
