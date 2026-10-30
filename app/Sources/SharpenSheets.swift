import AppKit

// Unsharp Mask, High Pass, Reduce Noise and Dust & Scratches run the core's shared filter kernels
// (`filter_kernels.rs`) and write colour only. Their sliders show display units
// (amount in percent, thresholds and the denoise strength in 8-bit levels) while recording the
// normalized values the core takes; the flat argument is `radius`.
// Phase 9F routes all four and the legacy spatial filters through the
// selected native path in 16-bit mode; byte meanings stay unchanged.

/// The sharpening filters (Unsharp Mask, High Pass). They share one
/// kernel — binary64 radius, ceil(3*radius) support below radius 4 and the
/// radius-independent recursive Gaussian from 4 up, clamped edges,
/// premultiplied sampling — and write colour only, so alpha survives exactly.
/// Like every other destructive filter here they run through
/// `OperationCapture`, which confines the write to the active selection's
/// coverage while the neighborhood still samples the whole target.
///
/// One `params` closure feeds the preview and the core plan. The compute
/// closure is non-throwing because `SliderSheetController` is: an over-budget
/// plan is refused with a sentence on the sheet's status line (and at Apply),
/// as the MCP twin (`apply_filter`) refuses it.
extension SliderSheetController {
    static func unsharpMask(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        // The ambient plane/channel target, captured with the sheet: a
        // recorded filter step names its target explicitly, because the UI
        // reads it from the document where the agent reads an argument.
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in
            ["radius": values[0], "amount": values[1], "threshold": values[2]]
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Unsharp Mask", actionName: "Unsharp Mask",
            sliders: [
                FilterSliderDescriptor(
                    label: "Radius:", min: 0, max: 25, initial: 1.5,
                    format: { String(format: "%.2f px", $0) }),
                // Shown as a percentage; stored and recorded as the
                // multiplier the core takes, so 100% is exactly 1.
                FilterSliderDescriptor(
                    label: "Amount:", min: 0, max: 500, initial: 100,
                    map: { $0 / 100 },
                    format: { String(format: "%.0f%%", $0 * 100) }),
                // Shown in 8-bit levels, stored normalized: one level is
                // 1/255, which is what the core's 0–1 threshold means.
                FilterSliderDescriptor(
                    label: "Threshold:", min: 0, max: 64, initial: 0,
                    map: { $0 / 255 },
                    format: { String(format: "%.0f lv", ($0 * 255).rounded()) }),
            ],
            step: { values in
                .filter("unsharp_mask", [
                    "radius": ActionArgs.number(values[0]),
                    "amount": ActionArgs.number(values[1]),
                    "threshold": ActionArgs.number(values[2]),
                ], target: target)
            },
            // The proxy divides the radius by the proxy factor.
            proxy: { values, factor in [values[0] / factor, values[1], values[2]] },
            request: { ByteFilterRequest(op: "unsharp_mask", params: params($0)) },
            compute: { image, values, control in
                // A nil result with no error is an exact bypass: show the
                // unchanged picture. A refusal clears the preview instead.
                do {
                    return try image.filterOp("unsharp_mask", params: params(values), control: control) ?? image
                } catch { return nil }
            })
    }

    static func highPass(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in ["radius": values[0]] }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "High Pass", actionName: "High Pass",
            sliders: [
                FilterSliderDescriptor(
                    label: "Radius:", min: 0, max: 64, initial: 3,
                    format: { String(format: "%.2f px", $0) })
            ],
            step: { values in
                .filter("high_pass", ["radius": ActionArgs.number(values[0])], target: target)
            },
            proxy: { values, factor in [values[0] / factor] },
            request: { ByteFilterRequest(op: "high_pass", params: params($0)) },
            compute: { image, values, control in
                do { return try image.filterOp("high_pass", params: params(values), control: control) ?? image }
                catch { return nil }
            })
    }
}

extension EditorViewController {
    @objc func showUnsharpMask(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.unsharpMask(document: document, canvas: canvas))
    }
    @objc func showHighPass(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.highPass(document: document, canvas: canvas))
    }
}
