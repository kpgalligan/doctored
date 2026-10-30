import AppKit

/// Filters ▸ Presence…: Texture, Clarity and Dehaze on the active layer or plane, through
/// one core kernel (`dr_image_filter_op` "presence" at 8 bits, the selected-filter request of the
/// same name at 16), so the sheet, its recorded Action step and `apply_filter` agree, as
/// `LensCorrectionSheet.swift`'s do. Destructive and cancellable; it takes no frame, so a
/// selection confines the result by coverage while the kernel reads the whole target. All three
/// at 0 is the core's exact bypass: OK then records no undo step. In a 32-bit float document
/// all three run on the layer's display encoding, Dehaze holding a value above white
/// (`sampling_dehaze`'s rule D).
///
/// Every value is scale-free — the kernel's radii are a share of the target's longer side
/// (`sampling_presence.rs`) — so the preview's proxy takes the values unchanged.
extension SliderSheetController {
    static func presence(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        // Texture: contrast of medium-fine detail; negative smooths it and keeps edges.
        // Clarity: midtone local contrast at a wider radius; negative softens.
        // Dehaze: positive removes atmospheric haze; negative adds it.
        let controls = [("texture", "Texture:"), ("clarity", "Clarity:"), ("dehaze", "Dehaze:")]
        let keys = controls.map { $0.0 }
        let params: ([Double]) -> [String: Any] = { values in
            Dictionary(uniqueKeysWithValues: zip(keys, values.map { Int($0.rounded()) }))
        }
        let slider: (String) -> FilterSliderDescriptor = {
            FilterSliderDescriptor(
                label: $0, min: -100, max: 100, initial: 0, isInteger: true,
                format: { String(format: "%+.0f", $0.rounded()) })
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Presence", actionName: "Presence",
            sliders: controls.map { slider($0.1) },
            step: { values in .filter("presence", params(values), target: target) },
            proxy: { values, _ in values },
            request: { ByteFilterRequest(op: "presence", params: params($0)) },
            compute: { image, values, control in
                do {
                    return try image.filterOp("presence", params: params(values), control: control)
                        ?? image
                } catch { return nil }
            })
    }
}

extension EditorViewController {
    @objc func showPresence(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.presence(document: document, canvas: canvas))
    }
}
