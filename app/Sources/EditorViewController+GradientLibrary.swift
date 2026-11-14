import AppKit
import UniformTypeIdentifiers

/// Edit ▸ Gradients: Delete Gradient…, Import Gradients… and Export Gradients… (Doctored's own
/// `.doctoredgradients` file) for the user's gradient library, which the stop editor's Preset
/// popup saves to (`GradientPresetLibrary`, `GradientPresetRow`). Each is a preference write: no
/// undo step, no dirty flag, no Action step, and a layer or recorded step that used a deleted
/// gradient keeps its own stops. MCP mirrors: `delete_gradient`, `import_gradients`,
/// `export_gradients` (`AgentServer+Gradients.swift`). Validation: `validateGradientLibraryItem`;
/// at every depth and in every mode through `depthIndependentWindowActions`, since none reads a
/// pixel.
extension EditorViewController {
    /// Delete Gradient…: an alert whose popup lists the library's gradients, then each file that
    /// is not one ("file — reason"); Delete removes the chosen row's file.
    @objc func deleteLibraryGradient(_ sender: Any?) {
        let listing: PreferenceLibraryListing<GradientPreset>
        do {
            listing = try GradientPresetLibrary.listing()
        } catch {
            gradientLibraryFailure("Could Not Read the Gradients", error.localizedDescription)
            return
        }
        let rows: [(title: String, name: String?, file: String?)] =
            listing.entries.map { ($0.item.name, $0.item.name, nil) }
            + listing.broken.map { ("\($0.file) — \($0.reason)", nil, $0.file) }
        guard !rows.isEmpty else {
            gradientLibraryFailure("No Gradients to Delete",
                                   "The stop editor's Preset popup saves a gradient to the library.")
            return
        }
        let popup = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 280, height: 26),
                                  pullsDown: false)
        popup.setAccessibilityIdentifier("delete-gradient-row")
        popup.addItems(withTitles: rows.map(\.title))
        let alert = NSAlert()
        alert.messageText = "Delete a Gradient"
        alert.informativeText = "Layers and recorded steps that use it keep their own stops. "
            + "Deleting a gradient is a preference: it changes no picture and adds no undo step."
        alert.accessoryView = popup
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn,
              rows.indices.contains(popup.indexOfSelectedItem) else { return }
        let row = rows[popup.indexOfSelectedItem]
        do {
            if let file = row.file {
                try GradientPresetLibrary.delete(file: file)
            } else {
                try GradientPresetLibrary.delete(name: row.name ?? "")
            }
        } catch {
            gradientLibraryFailure("Could Not Delete the Gradient", error.localizedDescription)
        }
    }

    private static var gradientFileType: UTType {
        UTType(filenameExtension: GradientPresetLibrary.fileExtension, conformingTo: .json) ?? .json
    }

    /// Import Gradients…: one Doctored gradient file into the library.
    @objc func importGradientLibrary(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [Self.gradientFileType, .json]
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let result: GradientPresetLibrary.ImportResult
        do {
            result = try GradientPresetLibrary.importFile(url)
        } catch {
            gradientLibraryFailure("Could Not Import the Gradients", error.localizedDescription)
            return
        }
        guard !result.skipped.isEmpty else { return }
        gradientLibraryFailure(
            "Imported \(result.imported.count) of "
                + "\(result.imported.count + result.skipped.count) Gradients",
            result.skipped.map {
                "\($0["name"] as? String ?? "?"): \($0["reason"] as? String ?? "")"
            }.joined(separator: "\n"))
    }

    /// Export Gradients…: every library gradient to one Doctored gradient file.
    @objc func exportGradientLibrary(_ sender: Any?) {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [Self.gradientFileType]
        panel.nameFieldStringValue = "Gradients.\(GradientPresetLibrary.fileExtension)"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            _ = try GradientPresetLibrary.exportFile(to: url, names: nil)
        } catch {
            gradientLibraryFailure("Could Not Export the Gradients", error.localizedDescription)
        }
    }

    /// The Edit ▸ Gradients items; nil for any other item. They read no document, so nothing
    /// turns them off.
    func validateGradientLibraryItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard let action = item.action, [#selector(deleteLibraryGradient(_:)),
                                         #selector(importGradientLibrary(_:)),
                                         #selector(exportGradientLibrary(_:))].contains(action)
        else { return nil }
        return true
    }

    private func gradientLibraryFailure(_ title: String, _ detail: String) {
        NSSound.beep()
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = detail
        alert.runModal()
    }
}

/// The stop editor's Preset row (`GradientStopsPane`): a popup of the library's gradients by
/// name and Save Preset…. Choosing a gradient hands its stops and Smoothness to the pane; Save
/// asks a name and writes the pane's stops at the host's Smoothness.
enum GradientPresetRow {
    /// The popup's title row (a pull-down's first item is its face).
    static let title = "Presets"
    static let saveTitle = "Save Preset…"

    /// Fills `popup` (a pull-down): the face, the library's gradients, then Save Preset….
    static func fill(_ popup: NSPopUpButton) {
        GradientPresetLibrary.reload()
        popup.removeAllItems()
        popup.addItem(withTitle: title)
        for preset in GradientPresetLibrary.presets {
            popup.addItem(withTitle: preset.name)
            popup.lastItem?.representedObject = preset.name
        }
        popup.menu?.addItem(.separator())
        popup.addItem(withTitle: saveTitle)
    }

    /// The library gradient `item` names, nil for the face and Save Preset….
    static func preset(for item: NSMenuItem?) -> GradientPreset? {
        guard let name = item?.representedObject as? String else { return nil }
        return GradientPresetLibrary.presets.first { $0.name == name }
    }

    /// Save Preset…: a name alert; saves `stops` at `smoothness` and answers the saved preset,
    /// nil on Cancel. A refusal (an invalid name, one in use) shows and answers nil.
    static func promptSave(stops: [GradientStop], smoothness: Double) -> GradientPreset? {
        let alert = NSAlert()
        alert.messageText = "Save Gradient"
        alert.informativeText = "Saves the stops and Smoothness to your gradient library."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 240, height: 24))
        field.placeholderString = "Gradient name"
        alert.accessoryView = field
        alert.addButton(withTitle: "Save")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let unInked = stops.map { stop -> GradientStop in
            var plain = stop
            plain.ink = nil
            return plain
        }
        do {
            return try GradientPresetLibrary.save(
                name: field.stringValue, stops: unInked, smoothness: smoothness, replace: false)
        } catch {
            NSSound.beep()
            let failure = NSAlert()
            failure.messageText = "Could Not Save the Gradient"
            failure.informativeText = error.localizedDescription
            failure.runModal()
            return nil
        }
    }
}
