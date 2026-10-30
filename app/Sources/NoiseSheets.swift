import AppKit

/// The noise-cleanup filters (Reduce Noise, Dust & Scratches). They run the
/// core's shared filter kernels (`filter_kernels.rs`) — one neighbourhood
/// dispatcher and one combine step. Like every
/// other destructive filter here they run through `OperationCapture`, which
/// confines the write to the active selection's coverage while the
/// neighbourhood still samples the whole target.
///
/// One `params` closure feeds the preview and the core plan. The compute
/// closure is non-throwing because `SliderSheetController` is: an over-budget
/// plan is refused with a sentence on the sheet's status line (and at Apply),
/// as the MCP twin (`apply_filter`) refuses it.
extension SliderSheetController {
    static func reduceNoise(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        // The ambient plane/channel target, captured with the sheet: a recorded
        // filter step names its target explicitly, because the UI reads it from
        // the document where the agent reads an argument.
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in
            ["radius": values[0], "luminance": values[1], "detail": values[2], "color": values[3]]
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Reduce Noise", actionName: "Reduce Noise",
            sliders: [
                FilterSliderDescriptor(
                    label: "Radius:", min: 0, max: 16, initial: 1,
                    format: { String(format: "%.2f px", $0) }),
                // Shown in 8-bit levels, stored normalized: one level is 1/255,
                // which is what the core's 0–0.25 strength means.
                FilterSliderDescriptor(
                    label: "Strength:", min: 0, max: 63, initial: 2.55,
                    map: { $0 / 255 },
                    format: { String(format: "%.1f lv", $0 * 255) }),
                FilterSliderDescriptor(
                    label: "Preserve details:", min: 0, max: 100, initial: 50,
                    map: { $0 / 100 },
                    format: { String(format: "%.0f%%", $0 * 100) }),
                FilterSliderDescriptor(
                    label: "Colour noise:", min: 0, max: 100, initial: 50,
                    map: { $0 / 100 },
                    format: { String(format: "%.0f%%", $0 * 100) }),
            ],
            step: { values in
                .filter("reduce_noise", [
                    "radius": ActionArgs.number(values[0]),
                    "luminance": ActionArgs.number(values[1]),
                    "detail": ActionArgs.number(values[2]),
                    "color": ActionArgs.number(values[3]),
                ], target: target)
            },
            // The proxy divides the radius by the proxy factor.
            proxy: { values, factor in [values[0] / factor, values[1], values[2], values[3]] },
            request: { ByteFilterRequest(op: "reduce_noise", params: params($0)) },
            compute: { image, values, control in
                // A nil result with no error is an exact bypass: show the
                // unchanged picture. A refusal clears the preview instead.
                do {
                    return try image.filterOp("reduce_noise", params: params(values), control: control) ?? image
                } catch { return nil }
            })
    }

    static func dustAndScratches(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in
            ["radius": values[0], "threshold": values[1]]
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Dust & Scratches", actionName: "Dust & Scratches",
            sliders: [
                // Not integer-snapped: the disc takes a genuine binary64 radius
                // and grows in steps as it crosses each ring of pixel centres.
                FilterSliderDescriptor(
                    label: "Radius:", min: 0, max: 16, initial: 2,
                    format: { String(format: "%.2f px", $0) }),
                FilterSliderDescriptor(
                    label: "Threshold:", min: 0, max: 64, initial: 0,
                    map: { $0 / 255 },
                    format: { String(format: "%.0f lv", ($0 * 255).rounded()) }),
            ],
            step: { values in
                .filter("dust_and_scratches", [
                    "radius": ActionArgs.number(values[0]),
                    "threshold": ActionArgs.number(values[1]),
                ], target: target)
            },
            // A proxy radius below one pixel is the core's exact bypass, so
            // that proxy shows the picture unfiltered until the exact lands.
            proxy: { values, factor in [values[0] / factor, values[1]] },
            request: { ByteFilterRequest(op: "dust_and_scratches", params: params($0)) },
            compute: { image, values, control in
                do {
                    return try image.filterOp("dust_and_scratches", params: params(values), control: control) ?? image
                } catch { return nil }
            })
    }

    /// Filters ▸ Noise ▸ Add Grain…: luminance-weighted, positional
    /// grain through one core kernel at both depths (`add_grain`).
    static func addGrain(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        // One seed per sheet open, shared by preview and Apply, crossing as the
        // Int64 BIT PATTERN `add_noise` uses, so a replay lands identical grain.
        let seed = UInt64.random(in: UInt64.min ... UInt64.max)
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in
            [
                "amount": ActionArgs.number(values[0]),
                "size": ActionArgs.number(values[1]),
                "roughness": ActionArgs.number(values[2]),
                "seed": Int(bitPattern: UInt(seed)),
            ]
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Add Grain", actionName: "Add Grain",
            sliders: [
                FilterSliderDescriptor(
                    label: "Amount:", min: 0, max: 100, initial: 25,
                    format: { String(format: "%.0f", $0) }),
                FilterSliderDescriptor(
                    label: "Size:", min: 0.5, max: 4, initial: 1,
                    format: { String(format: "%.2f px", $0) }),
                FilterSliderDescriptor(
                    label: "Roughness:", min: 0, max: 100, initial: 50,
                    format: { String(format: "%.0f", $0) }),
            ],
            step: { values in .filter("add_grain", params(values), target: target) },
            request: { ByteFilterRequest(op: "add_grain", params: params($0)) },
            compute: { image, values, control in
                do { return try image.filterOp("add_grain", params: params(values), control: control) ?? image }
                catch { return nil }
            })
    }
}

extension EditorViewController {
    @objc func showReduceNoise(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.reduceNoise(document: document, canvas: canvas))
    }
    @objc func showDustAndScratches(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.dustAndScratches(document: document, canvas: canvas))
    }
    @objc func showAddGrain(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.addGrain(document: document, canvas: canvas))
    }
}
