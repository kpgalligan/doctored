import AppKit

extension EditorViewController {
    @objc func showMaskProperties(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        do { presentAsSheet(try MaskPropertiesSheetController(document: document, canvas: canvas)) }
        catch { NSApp.presentError(error) }
    }

    @objc func transformMask(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        do { presentAsSheet(try MaskTransformSheetController(document: document, canvas: canvas)) }
        catch { NSApp.presentError(error) }
    }

    @objc func toggleMaskContentLink(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        do {
            let capture = try MaskEditCapture(document)
            let p = capture.properties
            try document.applyMaskEdit(.properties(linked: !p.linked, density: p.density, feather: p.feather),
                capture: capture, name: p.linked ? "Unlink Mask" : "Link Mask",
                record: .step("set_mask_properties", ["layer": ActionArgs.activeLayer, "linked": !p.linked],
                              note: "Layer ▸ Mask ▸ Link with Content"))
        } catch { document.presentError(error) }
    }

    func validateIndependentMaskItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        guard let document, let source = document.doc,
              let properties = source.layerMaskProperties(document.activeLayerIndex) else { return false }
        if item.action == #selector(toggleMaskContentLink(_:)), let menu = item as? NSMenuItem {
            menu.state = properties.linked ? .on : .off
        }
        return !document.isLiveEditing && source.lockBlockingFlags(document.activeLayerIndex, kind: DR_EDIT_MASK).isEmpty
    }
}
