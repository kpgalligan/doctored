import AppKit

// One `params` closure feeds both the recorded `apply_filter` step and the preview, Surface Blur's
// threshold is shown and recorded in 8-bit levels at every depth, and Add Grain draws one seed per
// sheet open, recorded as `add_noise` records its seed.

/// Filters ▸ Blur ▸ Gaussian Blur…, Box Blur…, Radial Blur… and Surface Blur…: each runs one core kernel
/// (`dr_image_filter_op` at 8 bits, the selected-filter request of the same
/// name at 16), so a sheet, its recorded Action step and `apply_filter` agree.
/// Like every other destructive filter here they run through
/// `OperationCapture`, which confines the write to the active selection's
/// coverage while the window still samples the whole target.
///
/// One `params` closure feeds the recorded step, the preview and the core plan,
/// so what is applied is exactly what was previewed and admitted. The
/// compute closure is non-throwing because `SliderSheetController` is: an
/// over-budget plan is refused with a sentence on the sheet's status line (and
/// at Apply), as `apply_filter` refuses it.
extension SliderSheetController {
    /// Filters ▸ Blur ▸ Gaussian Blur…. At 8 bits it is
    /// `dr_image_filter_op` `blur`: planned and admitted, previewed under a
    /// cancellable control, and applied as a cancellable job, like the other
    /// sheets here. The recorded step is `apply_filter` `blur` with `sigma`,
    /// as it always was.
    static func gaussianBlur(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in
            ["sigma": ActionArgs.number(values[0])]
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Gaussian blur", actionName: "Gaussian Blur",
            sliders: [
                FilterSliderDescriptor(
                    label: "Radius (sigma):", min: 0.5, max: 25, initial: 2,
                    format: { String(format: "%.1f", $0) })
            ],
            step: { values in .filter("blur", params(values), target: target) },
            // Sigma is a length, so it is divided by the proxy factor.
            proxy: { values, factor in [values[0] / factor] },
            request: { ByteFilterRequest(op: "blur", params: params($0)) },
            compute: { image, values, control in
                do { return try image.filterOp("blur", params: params(values), control: control) ?? image }
                catch { return nil }
            })
    }

    static func boxBlur(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        // The ambient plane/channel target, captured with the sheet: a recorded
        // filter step names its target explicitly.
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in
            ["radius": Int(values[0].rounded())]
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Box Blur", actionName: "Box Blur",
            sliders: [
                FilterSliderDescriptor(
                    label: "Radius:", min: 1, max: 250, initial: 5, isInteger: true,
                    format: { String(format: "%.0f px", $0.rounded()) })
            ],
            step: { values in .filter("box_blur", params(values), target: target) },
            // An integer radius divided by the proxy factor, at least 1.
            proxy: { values, factor in [max(1, (values[0] / factor).rounded())] },
            request: { ByteFilterRequest(op: "box_blur", params: params($0)) },
            compute: { image, values, control in
                do { return try image.filterOp("box_blur", params: params(values), control: control) ?? image }
                catch { return nil }
            })
    }

    /// Filters ▸ Blur ▸ Radial Blur…: Spin or Zoom about a
    /// centre picked on the image. The centre is a fraction of the FRAME —
    /// the selection's bounds on the target, or the target's extent — which
    /// is what the picker shows and what the kernel reads (`FilterFrame`).
    static func radialBlur(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let capture = document.captureOperation()
        let methods = ["spin", "zoom"], qualities = ["draft", "good", "best"]
        let params: ([Double]) -> [String: Any] = { values in
            [
                "amount": Int(values[0].rounded()),
                "method": methods[min(methods.count - 1, max(0, Int(values[1])))],
                "quality": qualities[min(qualities.count - 1, max(0, Int(values[2])))],
                "center_x": ActionArgs.number(values[3]), "center_y": ActionArgs.number(values[4]),
            ]
        }
        // The 8-bit request: the capture's frame as `region_*`, scaled to a proxy.
        let framed: ([Double], RasterImage) -> [String: Any] = { values, image in
            let kernel = (try? FilterFrame.kernelParameters("radial_blur", params(values),
                                                            target: document.planeEditTarget)) ?? params(values)
            return capture?.framedParameters(kernel, op: "radial_blur", for: image) ?? kernel
        }
        let size = CGSize(width: document.doc?.width ?? 0, height: document.doc?.height ?? 0)
        let picker = RadialBlurCenterAccessory(
            image: canvas.image, canvas: size,
            frame: FilterFrame.pickerFrame(document: document, target: document.planeEditTarget,
                                           layer: document.activeLayerIndex))
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Radial Blur", actionName: "Radial Blur",
            sliders: [
                FilterSliderDescriptor(
                    label: "Amount:", min: 1, max: 100, initial: 10, isInteger: true,
                    format: { String(format: "%.0f", $0.rounded()) })
            ],
            accessories: [
                FilterChoiceAccessory(label: "Blur Method:", titles: ["Spin", "Zoom"], initial: 0),
                FilterChoiceAccessory(label: "Quality:", titles: ["Draft", "Good", "Best"], initial: 1),
                picker,
            ],
            step: { values in .filter("radial_blur", params(values), target: target) },
            // An angle and a fraction of the radius do not change with scale.
            proxy: { values, _ in values },
            request: { values in
                ByteFilterRequest(op: "radial_blur", params: capture.map { framed(values, $0.source) } ?? params(values))
            },
            compute: { image, values, control in
                do { return try image.filterOp("radial_blur", params: framed(values, image), control: control) ?? image }
                catch { return nil }
            })
    }

    static func surfaceBlur(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in
            ["radius": Int(values[0].rounded()), "threshold": Int(values[1].rounded())]
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Surface Blur", actionName: "Surface Blur",
            sliders: [
                FilterSliderDescriptor(
                    label: "Radius:", min: 1, max: 100, initial: 5, isInteger: true,
                    format: { String(format: "%.0f px", $0.rounded()) }),
                // In 8-bit levels at every depth: a step
                // of 2.5× the threshold or more is left alone.
                FilterSliderDescriptor(
                    label: "Threshold:", min: 2, max: 255, initial: 15, isInteger: true,
                    format: { String(format: "%.0f lv", $0.rounded()) }),
            ],
            step: { values in .filter("surface_blur", params(values), target: target) },
            proxy: { values, factor in [max(1, (values[0] / factor).rounded()), values[1]] },
            request: { ByteFilterRequest(op: "surface_blur", params: params($0)) },
            compute: { image, values, control in
                do { return try image.filterOp("surface_blur", params: params(values), control: control) ?? image }
                catch { return nil }
            })
    }
}

extension EditorViewController {
    @objc func showBoxBlur(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.boxBlur(document: document, canvas: canvas))
    }
    @objc func showRadialBlur(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.radialBlur(document: document, canvas: canvas))
    }
    @objc func showSurfaceBlur(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.surfaceBlur(document: document, canvas: canvas))
    }
}
