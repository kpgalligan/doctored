import AppKit

/// Filter ▸ Sharpen ▸ Smart Sharpen…: Amount, Radius, Reduce Noise, Remove (Gaussian, Lens
/// or Motion Blur, with an Angle) and the Shadows and Highlights fades on the active layer or
/// plane, through one core kernel (`dr_image_filter_op` "smart_sharpen" at 8 bits, the
/// selected-filter request of the same name at 16 and on a 32-bit float layer's display encoding),
/// so the sheet, its recorded Action step and `apply_filter` agree, as `PresenceSheet.swift`'s do.
/// Destructive and cancellable; it takes no frame, so a selection confines the result by coverage
/// while the kernel reads the whole target. It is never the core's bypass, so OK is one undo step.
///
/// Radius and the two tonal radii are pixels, so the preview's proxy divides them by its factor
/// (Radius to at least 0.1 px, a tonal radius to at least 1); the percentages and the angle are
/// scale-free.
extension SliderSheetController {
    static func smartSharpen(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let removes = SmartSharpenRemove.allCases
        // Ten sliders, then the Remove popup's index.
        let settings: ([Double]) -> SmartSharpenSettings = { values in
            var s = SmartSharpenSettings()
            s.amount = values[0].rounded()
            s.radius = values[1]
            s.noise = values[2].rounded()
            s.angle = values[3].rounded()
            s.shadowFade = values[4].rounded()
            s.shadowTonalWidth = values[5].rounded()
            s.shadowRadius = Int(values[6].rounded())
            s.highlightFade = values[7].rounded()
            s.highlightTonalWidth = values[8].rounded()
            s.highlightRadius = Int(values[9].rounded())
            s.remove = removes[min(removes.count - 1, max(0, Int(values[10])))]
            return s
        }
        let params: ([Double]) -> [String: Any] = { settings($0).params }
        let percent: (String, Double, Double, Double) -> FilterSliderDescriptor = {
            FilterSliderDescriptor(
                label: $0, min: $1, max: $2, initial: $3, isInteger: true,
                format: { String(format: "%.0f%%", $0.rounded()) })
        }
        let pixels: (String) -> FilterSliderDescriptor = {
            FilterSliderDescriptor(
                label: $0, min: 1, max: 100, initial: 1, isInteger: true,
                format: { String(format: "%.0f px", $0.rounded()) })
        }
        let defaults = SmartSharpenSettings()
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Smart Sharpen", actionName: "Smart Sharpen",
            sliders: [
                percent("Amount:", 1, 500, defaults.amount),
                FilterSliderDescriptor(
                    label: "Radius:", min: 0.1, max: 64, initial: defaults.radius,
                    format: { String(format: "%.1f px", $0) }),
                percent("Reduce Noise:", 0, 100, defaults.noise),
                FilterSliderDescriptor(
                    label: "Angle:", min: -180, max: 180, initial: defaults.angle, isInteger: true,
                    format: { String(format: "%.0f°", $0.rounded()) }),
                percent("Shadows Fade:", 0, 100, defaults.shadowFade),
                percent("Shadows Tonal Width:", 0, 100, defaults.shadowTonalWidth),
                pixels("Shadows Radius:"),
                percent("Highlights Fade:", 0, 100, defaults.highlightFade),
                percent("Highlights Tonal Width:", 0, 100, defaults.highlightTonalWidth),
                pixels("Highlights Radius:"),
            ],
            accessories: [
                FilterChoiceAccessory(
                    label: "Remove:", titles: removes.map(\.title),
                    initial: removes.firstIndex(of: defaults.remove) ?? 1),
            ],
            step: { values in .filter("smart_sharpen", params(values), target: target) },
            proxy: { values, factor in
                var scaled = values
                scaled[1] = max(0.1, values[1] / factor)
                scaled[6] = max(1, (values[6] / factor).rounded())
                scaled[9] = max(1, (values[9] / factor).rounded())
                return scaled
            },
            request: { ByteFilterRequest(op: "smart_sharpen", params: params($0)) },
            compute: { image, values, control in
                do {
                    return try image.filterOp("smart_sharpen", params: params(values), control: control)
                        ?? image
                } catch { return nil }
            })
    }
}

extension EditorViewController {
    @objc func showSmartSharpen(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.smartSharpen(document: document, canvas: canvas))
    }
}
