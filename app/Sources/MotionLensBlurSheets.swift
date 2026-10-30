import AppKit

/// Filters ▸ Blur ▸ Lens Blur…, Motion Blur… and Tilt-Shift…. Each runs one core kernel
/// (`dr_image_filter_op` at 8 bits, the selected-filter request of the same name at 16), so a
/// sheet, its recorded Action step and `apply_filter` agree, exactly as `BlurSheets.swift`'s three
/// do. All are destructive and cancellable, and none takes a frame: a selection confines the
/// result by coverage while the kernel still samples the whole target.
///
/// One `params` closure feeds the recorded step, the preview and the core plan. A length in pixels
/// (Motion's distance, the two radii) is divided by the preview's proxy factor, at least 1; angles
/// and the fractions of the image (Tilt-Shift's centre, focus and transition) do not change with
/// scale.
extension SliderSheetController {
    static func motionBlur(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in
            ["angle": Int(values[0].rounded()), "distance": Int(values[1].rounded())]
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Motion Blur", actionName: "Motion Blur",
            sliders: [
                // Positive is counterclockwise, as the brush tip's angle is.
                FilterSliderDescriptor(
                    label: "Angle:", min: -90, max: 90, initial: 0, isInteger: true,
                    format: { String(format: "%.0f°", $0.rounded()) }),
                FilterSliderDescriptor(
                    label: "Distance:", min: 1, max: 999, initial: 10, isInteger: true,
                    format: { String(format: "%.0f px", $0.rounded()) }),
            ],
            step: { values in .filter("motion_blur", params(values), target: target) },
            proxy: { values, factor in [values[0], max(1, (values[1] / factor).rounded())] },
            request: { ByteFilterRequest(op: "motion_blur", params: params($0)) },
            compute: { image, values, control in
                do { return try image.filterOp("motion_blur", params: params(values), control: control) ?? image }
                catch { return nil }
            })
    }

    static func lensBlur(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in
            [
                "radius": Int(values[0].rounded()), "brightness": Int(values[1].rounded()),
                "threshold": Int(values[2].rounded()),
            ]
        }
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Lens Blur", actionName: "Lens Blur",
            sliders: [
                FilterSliderDescriptor(
                    label: "Radius:", min: 1, max: 100, initial: 15, isInteger: true,
                    format: { String(format: "%.0f px", $0.rounded()) }),
                // Specular highlights: how much brighter pixels above the
                // threshold weigh in the disc, so they bloom as discs.
                FilterSliderDescriptor(
                    label: "Brightness:", min: 0, max: 100, initial: 0, isInteger: true,
                    format: { String(format: "%.0f", $0.rounded()) }),
                // In 8-bit levels at every depth, as Surface Blur's.
                FilterSliderDescriptor(
                    label: "Threshold:", min: 0, max: 255, initial: 255, isInteger: true,
                    format: { String(format: "%.0f lv", $0.rounded()) }),
            ],
            step: { values in .filter("lens_blur", params(values), target: target) },
            proxy: { values, factor in [max(1, (values[0] / factor).rounded()), values[1], values[2]] },
            request: { ByteFilterRequest(op: "lens_blur", params: params($0)) },
            compute: { image, values, control in
                do { return try image.filterOp("lens_blur", params: params(values), control: control) ?? image }
                catch { return nil }
            })
    }

    /// Filters ▸ Blur ▸ Tilt-Shift…: sharp along a line through a centre
    /// picked on the image, blurring away from it. The centre is a fraction of
    /// the TARGET's extent — never of the selection's bounds, because the
    /// filter takes no frame (`FilterFrame.pickerFrame(…, selectionBounds:
    /// false)`).
    static func tiltShift(document: ImageDocument, canvas: ImageCanvasView) -> SliderSheetController {
        let target = document.planeEditTarget.layerEditAgentName(in: document.doc)
        let params: ([Double]) -> [String: Any] = { values in
            [
                "radius": Int(values[0].rounded()), "angle": Int(values[1].rounded()),
                "focus": ActionArgs.number(values[2] / 100),
                "transition": ActionArgs.number(values[3] / 100),
                "center_x": ActionArgs.number(values[4]), "center_y": ActionArgs.number(values[5]),
            ]
        }
        let size = CGSize(width: document.doc?.width ?? 0, height: document.doc?.height ?? 0)
        let picker = RadialBlurCenterAccessory(
            image: canvas.image, canvas: size,
            frame: FilterFrame.pickerFrame(document: document, target: document.planeEditTarget,
                                           layer: document.activeLayerIndex, selectionBounds: false))
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Tilt-Shift", actionName: "Tilt-Shift",
            sliders: [
                FilterSliderDescriptor(
                    label: "Blur:", min: 1, max: 100, initial: 15, isInteger: true,
                    format: { String(format: "%.0f px", $0.rounded()) }),
                // 0 is a horizontal focus line; positive is counterclockwise.
                FilterSliderDescriptor(
                    label: "Angle:", min: -90, max: 90, initial: 0, isInteger: true,
                    format: { String(format: "%.0f°", $0.rounded()) }),
                // Both in percent of the image height: how far from the line
                // stays sharp, and how far the blur takes to reach full.
                FilterSliderDescriptor(
                    label: "Focus:", min: 0, max: 100, initial: 15, isInteger: true,
                    format: { String(format: "%.0f%%", $0.rounded()) }),
                FilterSliderDescriptor(
                    label: "Transition:", min: 0, max: 100, initial: 25, isInteger: true,
                    format: { String(format: "%.0f%%", $0.rounded()) }),
            ],
            accessories: [picker],
            step: { values in .filter("tilt_shift", params(values), target: target) },
            proxy: { values, factor in
                [max(1, (values[0] / factor).rounded())] + Array(values.dropFirst())
            },
            request: { ByteFilterRequest(op: "tilt_shift", params: params($0)) },
            compute: { image, values, control in
                do { return try image.filterOp("tilt_shift", params: params(values), control: control) ?? image }
                catch { return nil }
            })
    }
}

extension EditorViewController {
    @objc func showMotionBlur(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.motionBlur(document: document, canvas: canvas))
    }
    @objc func showLensBlur(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.lensBlur(document: document, canvas: canvas))
    }
    @objc func showTiltShift(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.tiltShift(document: document, canvas: canvas))
    }
}
