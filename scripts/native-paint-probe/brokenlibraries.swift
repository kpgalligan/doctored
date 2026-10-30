import AppKit

// Broken preference-library files, listed in the UI. Each
// case plants two files the shared reader cannot list — `{}` (no
// schema_version) and a schema_version 99 file — into the library's own
// private folder (the runner sets every DR_*_DIR), drives the actual modal
// card or menu, and asserts what the user would see and what Delete File
// leaves on disk. Both planted files are `unknown_version`, so neither is
// repairable, and outside RAW no Repair button exists at all.

private let emptyObject = "{}"
private let futureVersion = "{\"schema_version\": 99, \"name\": \"x\"}"

private func plant(_ directory: URL, _ file: String, _ text: String) throws {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    try Data(text.utf8).write(to: directory.appendingPathComponent(file))
}

private func unplant(_ directory: URL, _ files: [String]) {
    for file in files {
        try? FileManager.default.removeItem(at: directory.appendingPathComponent(file))
    }
}

/// The section is HIDDEN when its popup is absent from the walk (a stack
/// view detaches a hidden arranged view) or the view found reports a hidden
/// ancestor.
private func sectionHidden(_ views: [NSView], _ identifier: String) -> Bool {
    guard let list = views.first(where: { $0.accessibilityIdentifier() == identifier }) else {
        return true
    }
    return list.isHiddenOrHasHiddenAncestor
}

private func shownList(_ views: [NSView], _ identifier: String) throws -> NSPopUpButton {
    guard let list = views.first(where: { $0.accessibilityIdentifier() == identifier })
        as? NSPopUpButton else {
        throw DoctoredCoreError(message: "No \(identifier) popup in the card")
    }
    try require(!list.isHiddenOrHasHiddenAncestor, "\(identifier) is hidden with broken files planted")
    return list
}

private func identifiedButton(_ views: [NSView], _ identifier: String) throws -> NSButton {
    guard let button = views.compactMap({ $0 as? NSButton })
        .first(where: { $0.accessibilityIdentifier() == identifier }) else {
        throw DoctoredCoreError(message: "No \(identifier) button in the card")
    }
    return button
}

private func titledButton(_ views: [NSView], _ title: String) throws -> NSButton {
    guard let button = views.compactMap({ $0 as? NSButton }).first(where: { $0.title == title }) else {
        throw DoctoredCoreError(message: "No \(title) button in the card")
    }
    return button
}

/// Runs a modal card and drives it ONCE from a `.common` + `.modalPanel`
/// timer (`views.swift`'s pattern). The drive ends the session itself; a
/// thrown error or a card that never appears stops it here.
private func driveModal<T>(_ title: String, run: () -> T,
                           drive: @escaping (NSWindow, [NSView]) throws -> Void) throws -> T {
    var problem: Error?
    var visited = false
    let timer = Timer(timeInterval: 0.05, repeats: true) { timer in
        guard !visited, let window = NSApp.windows.first(where: { $0.title == title && $0.isVisible }),
              let card = window.contentView else { return }
        visited = true
        timer.invalidate()
        card.layoutSubtreeIfNeeded()
        do {
            try drive(window, descendants(card))
        } catch {
            problem = error
            NSApp.stopModal(withCode: .cancel)
        }
    }
    let safety = Timer(timeInterval: 20, repeats: false) { _ in
        timer.invalidate()
        if !visited { problem = DoctoredCoreError(message: "The \(title) card never came up") }
        NSApp.stopModal(withCode: .cancel)
    }
    for each in [timer, safety] {
        RunLoop.main.add(each, forMode: .common)
        RunLoop.main.add(each, forMode: .modalPanel)
    }
    let result = run()
    timer.invalidate(); safety.invalidate(); pump()
    if let problem { throw problem }
    try require(visited, "The \(title) card was not driven")
    return result
}

private func fitsScreen(_ window: NSWindow) -> Bool {
    guard let visible = (window.screen ?? NSScreen.main)?.visibleFrame else { return false }
    return visible.contains(window.frame)
}

/// One manager, clean then broken: hidden with no broken file, two rows and
/// no Repair with two, and Delete File removes exactly one file.
private func managerCase(title: String, prefix: String, directory: URL,
                         files: [String], deleteFirst: String?,
                         broken: @escaping () throws -> Int, run: @escaping () -> Void,
                         afterDelete: (() throws -> Void)? = nil) throws -> [String: Any] {
    try require(try broken() == 0, "\(title): the library already holds broken files")
    var cleanHidden = false
    try driveModal(title, run: run) { _, views in
        cleanHidden = sectionHidden(views, prefix + "-broken-list")
        try titledButton(views, "Cancel").performClick(nil)
    }
    try require(cleanHidden, "\(title): the broken-file section shows with no broken file")

    try plant(directory, files[0], emptyObject)
    try plant(directory, files[1], futureVersion)
    defer { unplant(directory, files) }
    var rows = 0, repairPresent = true, fits = false, remaining = -1
    try driveModal(title, run: run) { window, views in
        let list = try shownList(views, prefix + "-broken-list")
        rows = list.numberOfItems
        repairPresent = views.contains { $0.accessibilityIdentifier() == prefix + "-repair" }
        fits = fitsScreen(window)
        if let deleteFirst {
            guard let row = list.itemArray.first(where: { $0.title.hasPrefix(deleteFirst) }) else {
                throw DoctoredCoreError(message: "\(title): no row for \(deleteFirst)")
            }
            list.select(row)
        }
        try identifiedButton(views, prefix + "-delete-file").performClick(nil)
        remaining = try broken()
        try titledButton(descendants(window.contentView ?? NSView()), "Cancel").performClick(nil)
    }
    try require(rows == 2, "\(title): \(rows) broken rows, expected 2")
    try require(!repairPresent, "\(title): a Repair button exists outside RAW")
    try require(fits, "\(title): the card is not inside the visible screen")
    try require(remaining == 1, "\(title): \(remaining) broken files after Delete File, expected 1")
    try afterDelete?()
    return ["title": title, "clean_hidden": cleanHidden, "rows": rows,
            "repair_present": repairPresent, "fits_screen": fits, "remaining_after_delete": remaining]
}

/// broken-library-managers-ui — the export and metadata managers.
/// The export pass also walks a dead end: a broken file sitting
/// on the digest slot "Web" owns refuses a save of "Web" until Delete File
/// clears it.
func brokenLibraryManagersUI() throws -> [String: Any] {
    let slot = PreferenceLibrary<ExportPresetCodec>.url("Web").lastPathComponent
    let web = ExportPreset(name: "Web",
                           settings: try ExportSettings.decode(["format": "png", "profile": "srgb"]),
                           destination: ExportPresetLibrary.directory.path, pattern: "{name}")
    var refusal = ""
    let export = try managerCase(
        title: "Export Presets", prefix: "export-preset",
        directory: ExportPresetLibrary.directory,
        files: [slot, "broken-future.json"], deleteFirst: slot,
        broken: { try ExportPresetLibrary.listing().broken.count },
        run: {
            // The dead end, asked while both files are planted: the first
            // manager run is the clean one, so only the second sees it.
            if (try? ExportPresetLibrary.listing().broken.count) == 2 {
                do { try ExportPresetLibrary.save(web) } catch { refusal = error.localizedDescription }
            }
            _ = ExportPresetManagerSheet.run(current: nil)
        },
        afterDelete: {
            try require(refusal.contains("already in use"),
                        "Saving Web over a broken slot said: \(refusal)")
            try ExportPresetLibrary.save(web)
            try ExportPresetLibrary.delete(name: "Web")
        })

    let metadata = try managerCase(
        title: "Metadata Policies", prefix: "metadata-policy",
        directory: MetadataPolicyLibrary.directory,
        files: ["broken-empty.json", "broken-future.json"], deleteFirst: nil,
        broken: { try MetadataPolicyLibrary.listing().broken.count },
        run: { _ = MetadataPolicyManageSheet.run(current: MetadataPolicy.defaults) })

    for (name, count) in [("export", try ExportPresetLibrary.listing().broken.count),
                          ("metadata", try MetadataPolicyLibrary.listing().broken.count)] {
        try require(count == 0, "The \(name) library kept \(count) planted files after cleanup")
    }
    return ["name": "broken-library-managers-ui", "passed": true,
            "export": export, "metadata": metadata,
            "export_slot_refusal": refusal]
}

/// new-document-broken-presets-ui — File ▸ New…'s card lists the preset
/// library's broken files under its Preset popup.
func newDocumentBrokenPresetsUI() throws -> [String: Any] {
    let directory = NewDocumentPresetLibrary.directory
    let files = ["broken-empty.json", "broken-future.json"]
    try require(try NewDocumentPresetLibrary.listing().broken.isEmpty,
                "The document preset library already holds broken files")
    try plant(directory, files[0], emptyObject)
    try plant(directory, files[1], futureVersion)
    defer { unplant(directory, files) }
    var rows = 0, repairPresent = true, remaining = -1
    let result = try driveModal("New Document", run: { NewDocumentSheetController.run() }) {
        window, views in
        let list = try shownList(views, "new-document-preset-broken-list")
        rows = list.numberOfItems
        repairPresent = views.contains { $0.accessibilityIdentifier() == "new-document-preset-repair" }
        try identifiedButton(views, "new-document-preset-delete-file").performClick(nil)
        remaining = try NewDocumentPresetLibrary.listing().broken.count
        try titledButton(descendants(window.contentView ?? NSView()), "Cancel").performClick(nil)
    }
    try require(rows == 2, "New Document: \(rows) broken rows, expected 2")
    try require(!repairPresent, "New Document: a Repair button exists")
    try require(remaining == 1, "New Document: \(remaining) broken files after Delete File, expected 1")
    try require(result == nil, "Cancel returned a request")
    return ["name": "new-document-broken-presets-ui", "passed": true, "rows": rows,
            "remaining_after_delete": remaining, "cancel_returned_nil": true]
}

/// workspace-broken-files-ui — Window ▸ Workspaces ▸ Delete Workspace grows a
/// "Delete Broken File" group whose row carries the file, validates on a
/// 16-bit editor and deletes through `deleteWorkspaceFile:`.
func workspaceBrokenFilesUI() throws -> [String: Any] {
    // `NSMenu.delegate` is weak: the delegate must outlive every rebuild.
    let delegate = AppDelegate()
    let menu = delegate.makeMainMenu()
    return try withExtendedLifetime(delegate) {
        guard let windowMenu = menu.items.first(where: { $0.title == "Window" })?.submenu,
              let workspaces = windowMenu.items.compactMap({ $0.submenu })
                  .first(where: { $0.title == "Workspaces" }),
              let deletes = workspaces.items.compactMap({ $0.submenu })
                  .first(where: { $0.title == "Delete Workspace" }) else {
            throw DoctoredCoreError(message: "Window ▸ Workspaces ▸ Delete Workspace is missing")
        }
        func group() -> NSMenu? {
            deletes.items.compactMap(\.submenu).first { $0.title == "Delete Broken File" }
        }
        try require(try WorkspaceLibrary.listing().broken.isEmpty,
                    "The workspace library already holds broken files")
        let directory = WorkspaceLibrary.directory
        let file = "broken-future.json"
        try plant(directory, file, futureVersion)
        defer { unplant(directory, [file]) }
        delegate.menuNeedsUpdate(deletes)
        guard let rows = group()?.items, rows.count == 1, let row = rows.first else {
            throw DoctoredCoreError(
                message: "No one-row Delete Broken File group: \(deletes.items.map(\.title))")
        }
        try require(row.representedObject as? String == file,
                    "The broken row carries \(String(describing: row.representedObject))")
        try require(row.action == #selector(EditorViewController.deleteWorkspaceFile(_:)),
                    "The broken row's action is \(String(describing: row.action))")
        let (document, editor, _) = try openDocument()
        defer { document.close(); pump() }
        try require(editor.validateUserInterfaceItem(row),
                    "Delete Broken File is disabled on a 16-bit document")
        editor.deleteWorkspaceFile(row)
        try require(try WorkspaceLibrary.listing().broken.isEmpty,
                    "deleteWorkspaceFile left the broken file")
        delegate.menuNeedsUpdate(deletes)
        try require(group() == nil, "The Delete Broken File group survived an empty rebuild")
        return ["name": "workspace-broken-files-ui", "passed": true, "row_title": row.title,
                "validated_at_16_bits": true, "group_gone_after_delete": true]
    }
}
