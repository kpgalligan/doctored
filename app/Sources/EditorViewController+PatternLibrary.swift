import AppKit
import UniformTypeIdentifiers

/// Edit ▸ Patterns: Delete Pattern…, Import Patterns… and Export Patterns… (Doctored's own
/// `.doctoredpatterns` file) for the user's pattern library, which Edit ▸ Define Pattern… writes
/// (`PatternLibrary`). Each is a preference write: no undo step, no dirty flag, no Action step,
/// and a document that uses a deleted pattern keeps its embedded tile. MCP mirrors:
/// `delete_pattern`, `import_patterns`, `export_patterns` (`AgentServer+Patterns.swift`).
/// Validation: `validateDefinePatternItem`; at every depth and in every mode through
/// `depthIndependentWindowActions`, since none reads a pixel.
extension EditorViewController {
    /// One row of Delete Pattern…'s popup: a library pattern by `name`, or a library `file` that
    /// holds no readable pattern (the `delete_pattern {file}` twin).
    struct PatternDeleteRow {
        let title: String
        let name: String?
        let file: String?
        let image: NSImage?
    }

    /// Delete Pattern…: an alert whose popup lists the library's patterns, then each file that is
    /// not one ("file — reason"); Delete removes the chosen row's file.
    @objc func deleteLibraryPattern(_ sender: Any?) {
        let rows: [PatternDeleteRow]
        do {
            rows = Self.patternDeleteRows(try PatternLibrary.listing())
        } catch {
            patternLibraryFailure("Could Not Read the Patterns", error.localizedDescription)
            return
        }
        guard !rows.isEmpty else {
            patternLibraryFailure("No Patterns to Delete",
                                  "Edit ▸ Define Pattern… adds a pattern to the library.")
            return
        }
        let popup = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 280, height: 26),
                                  pullsDown: false)
        popup.setAccessibilityIdentifier("delete-pattern-row")
        for row in rows {
            let entry = NSMenuItem(title: row.title, action: nil, keyEquivalent: "")
            entry.image = row.image
            popup.menu?.addItem(entry)
        }
        let alert = NSAlert()
        alert.messageText = "Delete a Pattern"
        alert.informativeText = "Documents that use it keep their own copy of the tile. Deleting "
            + "a pattern is a preference: it changes no picture and adds no undo step."
        alert.accessoryView = popup
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn,
              rows.indices.contains(popup.indexOfSelectedItem) else { return }
        if let refusal = Self.deletePatternRow(rows[popup.indexOfSelectedItem]) {
            patternLibraryFailure("Could Not Delete the Pattern", refusal)
        }
    }

    /// Delete Pattern…'s rows: the library's patterns by name with their swatches, then each
    /// broken file.
    static func patternDeleteRows(_ listing: PreferenceLibraryListing<LibraryPattern>)
        -> [PatternDeleteRow] {
        listing.entries.map {
            PatternDeleteRow(title: $0.item.name, name: $0.item.name, file: nil,
                             image: $0.item.swatch)
        } + listing.broken.map {
            PatternDeleteRow(title: "\($0.file) — \($0.reason)", name: nil, file: $0.file,
                             image: nil)
        }
    }

    /// Delete Pattern…'s commit: nil when the row's pattern or file is gone, else the library's
    /// refusal.
    static func deletePatternRow(_ row: PatternDeleteRow) -> String? {
        do {
            if let file = row.file {
                try PatternLibrary.delete(file: file)
            } else {
                try PatternLibrary.delete(name: row.name ?? "")
            }
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    private static var patternFileType: UTType {
        UTType(filenameExtension: PatternLibrary.fileExtension, conformingTo: .json) ?? .json
    }

    /// Import Patterns…: one Doctored pattern file into the library.
    @objc func importPatternLibrary(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [Self.patternFileType, .json]
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        if let report = Self.importPatternFile(url) {
            patternLibraryFailure(report.title, report.detail)
        }
    }

    /// Import Patterns…'s commit: nil when every entry imported, else what to tell the person —
    /// the file's refusal, or the entries skipped with their reasons.
    static func importPatternFile(_ url: URL) -> (title: String, detail: String)? {
        let result: PatternLibrary.ImportResult
        do {
            result = try PatternLibrary.importFile(url)
        } catch {
            return ("Could Not Import the Patterns", error.localizedDescription)
        }
        guard !result.skipped.isEmpty else { return nil }
        return ("Imported \(result.imported.count) of "
                    + "\(result.imported.count + result.skipped.count) Patterns",
                result.skipped.map {
                    "\($0["name"] as? String ?? "?"): \($0["reason"] as? String ?? "")"
                }.joined(separator: "\n"))
    }

    /// Export Patterns…: every library pattern to one Doctored pattern file.
    @objc func exportPatternLibrary(_ sender: Any?) {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [Self.patternFileType]
        panel.nameFieldStringValue = "Patterns.\(PatternLibrary.fileExtension)"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        if let refusal = Self.exportPatternFile(to: url) {
            patternLibraryFailure("Could Not Export the Patterns", refusal)
        }
    }

    /// Export Patterns…'s commit: nil when the file is written, else the library's refusal (an
    /// empty library, more patterns than one file holds, an unwritable path).
    static func exportPatternFile(to url: URL) -> String? {
        do {
            _ = try PatternLibrary.exportFile(to: url, names: nil)
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    private func patternLibraryFailure(_ title: String, _ detail: String) {
        NSSound.beep()
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = detail
        alert.runModal()
    }
}
