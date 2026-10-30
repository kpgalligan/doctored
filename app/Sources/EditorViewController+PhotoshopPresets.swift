import AppKit
import UniformTypeIdentifiers

// Layer ▸ New Adjustment Layer ▸ From Photoshop Preset… reads a
// `.acv`/`.alv`/`.ahu`/`.eap`/`.blw`/`.cha` (at most 64 KiB + 1 byte; the core refuses the rest)
// and opens the op's CREATE dialog with `preset:` — `AdjustmentSheets.make`,
// `AdjustmentLayerSheetController.make` and the Curves controller all take it, and it prefills
// `.create` only — so Apply records the resolved `add_adjustment_layer` and no preset name or path
// reaches an Action.

/// Layer ▸ New Adjustment Layer ▸ From Photoshop Preset…: opens
/// a Photoshop `.acv` / `.alv` / `.ahu` / `.eap` / `.blw` / `.cha` file and shows the
/// op's own create dialog PREFILLED with its values. Nothing is committed
/// until that dialog's Apply, which records the resolved
/// `add_adjustment_layer` — the preset's name and path never reach an Action.
/// MCP twin: `read_photoshop_adjustment_preset`, then `add_adjustment_layer`.
extension EditorViewController {
    @objc func newAdjustmentLayerFromPhotoshopPreset(_ sender: Any?) {
        guard let document = document, document.doc != nil, let window = view.window else {
            NSSound.beep()
            return
        }
        // Never leave a canvas session hanging behind a modal panel.
        guard commitPendingSessions() else { return }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        // #1 phase 5: a CMYK document reads a Levels or Curves preset's ink channels.
        let cmyk = document.doc?.isCmyk == true
        panel.message = cmyk
            ? "Choose a Photoshop CMYK Curves or Levels preset."
            : "Choose a Photoshop Curves, Levels, Hue/Saturation, Exposure, "
                + "Black & White or Channel Mixer preset."
        panel.prompt = "Open"
        panel.allowedContentTypes = (cmyk ? PhotoshopAdjustmentPreset.cmykFileExtensions
            : PhotoshopAdjustmentPreset.fileExtensions).compactMap {
            UTType(filenameExtension: $0)
        }
        panel.directoryURL = PhotoshopAdjustmentPreset.installedPresetsDirectory()
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            self?.newAdjustmentLayer(fromPhotoshopPreset: url)
        }
    }

    /// The preset at `url` as the create dialog of its op, prefilled. A file
    /// the core refuses is named in an alert with the core's sentence.
    func newAdjustmentLayer(fromPhotoshopPreset url: URL) {
        guard let document = document, document.doc != nil else {
            NSSound.beep()
            return
        }
        let payload: AdjustmentLayerPayload
        do {
            payload = try PhotoshopAdjustmentPreset.read(url, cmyk: document.doc?.isCmyk == true)
        } catch {
            let alert = NSAlert()
            alert.messageText = "This preset cannot be used as an adjustment layer."
            alert.informativeText = error.localizedDescription
            alert.runModal()
            return
        }
        guard let op = payload.knownOp,
              let sheet = AdjustmentLayerSheetController.make(
                op: op, document: document, canvas: canvas,
                mode: .create(selection: canvas.selection?.maskBytes()), preset: payload,
                onCommitted: { [weak self] idx in self?.didCommitAdjustmentLayer(idx) })
        else {
            NSSound.beep()
            return
        }
        presentAsSheet(sheet)
    }
}
