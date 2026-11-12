import AppKit
import UniformTypeIdentifiers

/// The paint options bar's Preset popup: Custom, the built-in presets, the
/// user's presets (`BrushPresetLibrary`), then Save Preset…, Delete Preset…
/// (the user preset the tip matches), Import Presets… and Export Presets…
/// (Doctored's own `.doctoredbrushes` file). Choosing a preset fills the
/// tool's options, which are app-wide preferences; the four actions write the
/// library, a preference too: no undo step, no dirty flag, no Action step.
/// MCP mirrors them with `AgentServer+BrushPresets.swift`.
extension EditorViewController {
    private static let presetActions = ["Save Preset…", "Delete Preset…", "Import Presets…",
                                        "Export Presets…"]

    /// The popup's descriptor for `paintClusters`. Presenting the bar re-reads
    /// the library, which is how another instance's writes arrive.
    func brushPresetDescriptor(read: @escaping () -> PaintToolOptions,
                               write: @escaping (PaintToolOptions) -> Void) -> OptionDescriptor {
        BrushPresetLibrary.reload()
        let presets = BrushPreset.all()
        let items = ["Custom"] + presets.map(\.name) + [OptionPopupControl.separatorTitle]
            + Self.presetActions
        return OptionDescriptor(
            id: "paint.preset", overflowLabel: "Preset",
            kind: .popup(
                width: 100, items: items,
                get: { BrushPreset.matchIndex(of: read(), in: presets) },
                set: { [weak self] index in
                    self?.brushPresetRowChosen(index, presets: presets, read: read, write: write)
                }))
    }

    private func brushPresetRowChosen(_ index: Int, presets: [BrushPreset],
                                      read: () -> PaintToolOptions,
                                      write: (PaintToolOptions) -> Void) {
        // End any live field edit FIRST: a field editor skips value refreshes
        // while active, and its stale text would commit right back over the
        // preset at the next blur.
        view.window?.makeFirstResponder(nil)
        if presets.indices.contains(index - 1) {
            write(presets[index - 1].applied(to: read()))
            syncCanvasPaintState()
            return
        }
        // Row 0 is "Custom" — what shows when the values match no preset;
        // picking it, or the separator, changes nothing.
        switch index - presets.count - 2 {
        case 0: saveBrushPreset(read())
        case 1:
            let row = BrushPreset.matchIndex(of: read(), in: presets)
            deleteBrushPreset(row > BrushPreset.builtIns.count ? presets[row - 1].name : nil)
        case 2: importBrushPresets()
        case 3: exportBrushPresets()
        default: break
        }
    }

    /// Save Preset…: the tool's six tip values under a new name.
    private func saveBrushPreset(_ options: PaintToolOptions) {
        let alert = NSAlert()
        alert.messageText = "Save Brush Preset"
        alert.informativeText = "A preset keeps the tip — hardness, flow, spacing, angle, roundness "
            + "and airbrush — not the size or opacity. It is a preference: saving one changes no "
            + "picture and adds no undo step."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24))
        DSField.style(field)
        field.placeholderString = "Preset name"
        field.setAccessibilityIdentifier("brush-preset-name")
        alert.accessoryView = field
        alert.addButton(withTitle: "Save Preset")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        do {
            try BrushPresetLibrary.save(name: field.stringValue,
                                        values: Self.brushPresetValues(options), replace: false)
        } catch {
            presetFailure("Could Not Save the Preset", error)
        }
    }

    /// The six tip values of `options`, as a preset holds them.
    static func brushPresetValues(_ options: PaintToolOptions) -> [String: Any] {
        ["hardness": options.hardness, "flow": options.flow, "spacing": options.spacing,
         "angle": options.angle, "roundness": options.roundness, "airbrush": options.airbrush]
    }

    /// Delete Preset…: the user preset the tip matches; any other row beeps.
    private func deleteBrushPreset(_ name: String?) {
        guard let name else { NSSound.beep(); return }
        let alert = NSAlert()
        alert.messageText = "Delete the brush preset “\(name)”?"
        alert.informativeText = "The tool keeps its current values."
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        do { try BrushPresetLibrary.delete(name: name) }
        catch { presetFailure("Could Not Delete the Preset", error) }
    }

    private static var brushPresetFileType: UTType {
        UTType(filenameExtension: BrushPresetLibrary.fileExtension, conformingTo: .json) ?? .json
    }

    /// Import Presets…: one Doctored brush preset file into the library.
    private func importBrushPresets() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [Self.brushPresetFileType, .json]
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let result = try BrushPresetLibrary.importFile(url)
            guard !result.skipped.isEmpty else { return }
            let alert = NSAlert()
            alert.messageText = "Imported \(result.imported.count) of "
                + "\(result.imported.count + result.skipped.count) Brush Presets"
            alert.informativeText = result.skipped.map {
                "\($0["name"] as? String ?? "?"): \($0["reason"] as? String ?? "")"
            }.joined(separator: "\n")
            alert.runModal()
        } catch {
            presetFailure("Could Not Import the Presets", error)
        }
    }

    /// Export Presets…: every user preset to one Doctored brush preset file.
    private func exportBrushPresets() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [Self.brushPresetFileType]
        panel.nameFieldStringValue = "Brush Presets.\(BrushPresetLibrary.fileExtension)"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do { _ = try BrushPresetLibrary.exportFile(to: url, names: nil) }
        catch { presetFailure("Could Not Export the Presets", error) }
    }

    private func presetFailure(_ title: String, _ error: Error) {
        NSSound.beep()
        let failure = NSAlert()
        failure.messageText = title
        failure.informativeText = error.localizedDescription
        failure.runModal()
    }

    /// A library write (this window's, another window's or an MCP call's):
    /// the bar re-presents once the run loop turns, so an import's many
    /// writes rebuild it once.
    @objc func brushPresetsDidChange(_ note: Notification) {
        NSObject.cancelPreviousPerformRequests(
            withTarget: self, selector: #selector(representBrushPresets), object: nil)
        perform(#selector(representBrushPresets), with: nil, afterDelay: 0)
    }

    @objc private func representBrushPresets() {
        func holds(_ view: NSView) -> Bool {
            view.identifier?.rawValue == "paint.preset" || view.subviews.contains(where: holds)
        }
        guard currentTool.usesBrushTip, holds(optionsBar) else { return }
        presentToolOptions()
    }
}
