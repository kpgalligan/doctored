import AppKit

/// Filters ▸ Other ▸ Offset…: Offset, through one core
/// kernel (`dr_image_filter_op` at 8 bits, the selected filter `offset` at 16).
/// Content moves right and down by the two integers, within the FRAME — the
/// selection's bounds on the target, or the whole target — and the vacated
/// area is Set to Transparent on a layer (there is no Background layer kind) or
/// Set to Background on a mask, colour plane or channel, Repeat Edge Pixels, or
/// Wrap Around (`FilterFrame` translates the first for the kernel). What is
/// recorded is the host-level choice and the background colour, so a replay on
/// another target reads it the way that target means it.
extension SliderSheetController {
    static func offset(document: ImageDocument, canvas: ImageCanvasView, background: NSColor) -> SliderSheetController {
        let editTarget = document.planeEditTarget
        let target = editTarget.layerEditAgentName(in: document.doc)
        let capture = document.captureOperation()
        let first = editTarget == .layer ? "transparent" : "background"
        let modes = [first, "repeat", "wrap"]
        let color = ActionArgs.hex(background)
        let params: ([Double]) -> [String: Any] = { values in
            let mode = modes[min(modes.count - 1, max(0, Int(values[2])))]
            var result: [String: Any] = [
                "horizontal": Int(values[0].rounded()), "vertical": Int(values[1].rounded()),
                "undefined_areas": mode,
            ]
            if mode == "background" { result["background_color"] = color }
            return result
        }
        let framed: ([Double], RasterImage) -> [String: Any] = { values, image in
            let kernel = (try? FilterFrame.kernelParameters("offset", params(values), target: editTarget,
                                                            lab: document.doc?.isLab == true))
                ?? params(values)
            return capture?.framedParameters(kernel, op: "offset", for: image) ?? kernel
        }
        let width = max(1, document.doc?.width ?? 1), height = max(1, document.doc?.height ?? 1)
        return SliderSheetController(
            document: document, canvas: canvas,
            title: "Offset", actionName: "Offset",
            sliders: [],
            accessories: [
                FilterIntegerFieldAccessory(label: "Horizontal:", range: -width...width, initial: 0,
                                            unit: "px right"),
                FilterIntegerFieldAccessory(label: "Vertical:", range: -height...height, initial: 0,
                                            unit: "px down"),
                FilterChoiceAccessory(
                    label: "Undefined Areas:",
                    titles: [editTarget == .layer ? "Set to Transparent" : "Set to Background",
                             "Repeat Edge Pixels", "Wrap Around"],
                    initial: 2),
            ],
            step: { values in .filter("offset", params(values), target: target) },
            // The offsets in proxy pixels, rounded.
            proxy: { values, factor in [(values[0] / factor).rounded(), (values[1] / factor).rounded(), values[2]] },
            request: { values in
                ByteFilterRequest(op: "offset", params: capture.map { framed(values, $0.source) } ?? params(values))
            },
            compute: { image, values, control in
                do { return try image.filterOp("offset", params: framed(values, image), control: control) ?? image }
                catch { return nil }
            })
    }
}

extension EditorViewController {
    @objc func showOffset(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        presentAsSheet(SliderSheetController.offset(document: document, canvas: canvas, background: backgroundColor))
    }
}
