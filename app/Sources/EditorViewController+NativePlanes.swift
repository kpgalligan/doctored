import AppKit

extension EditorViewController {
    func loadNativePlaneSelection(from source: SelectionSource, mode: SelectionCombineMode, invert: Bool) {
        guard let document else { return }
        do {
            let capture = try PrecisionEditorCapture(document)
            guard !capture.quickMask else { throw DoctoredCoreError(message: OperationScope.quickMaskRefusal) }
            var samples = try PrecisionScalarOperations.canvasPlane(capture.facade, source: source).samples
            if invert { samples = samples.map { 65535 - $0 } }
            try capture.requireCurrent(document)
            let selection = CanvasSelection(shape: .mask16(samples), canvasWidth: capture.facade.width,
                canvasHeight: capture.facade.height)
            let combined: CanvasSelection?
            if let selection { combined = CanvasSelection.combine(capture.selection, with: selection, mode: mode) }
            else { combined = mode == .replace || mode == .intersect ? nil : capture.selection }
            commitCanvasSelection(combined, actionName: "Load Selection", record:
                .loadSelection(from: source, mode: mode.agentName, invert: invert, in: capture.facade))
        } catch { document.presentError(error) }
    }

    func saveNativePlaneSelection(to destination: SelectionDestination, mode: SelectionCombineMode) {
        guard let document else { return }
        do {
            let capture = try PrecisionEditorCapture(document), doc = capture.facade
            guard let selection = capture.selection else { throw DoctoredCoreError(message: "Make a selection first.") }
            let index: Int?, name: String, samples: [UInt16]
            switch destination {
            case .newChannel(let channelName):
                guard channelBudgetAllowsOneMore(doc) else { return }
                index = nil; name = channelName; samples = selection.maskSamples16()
            case .channel(let channelIndex):
                index = channelIndex; name = doc.channelInfo(channelIndex)?.name ?? "Alpha"
                samples = try PrecisionScalarOperations.combine(
                    PrecisionScalarOperations.read(doc, target: "channel", index: channelIndex).samples,
                    selection.maskSamples16(), mode: mode)
            }
            let edit = try PrecisionScalarWork(source: capture.source, command: .setChannel(index: index, name: name), samples: samples)
            _ = try capture.publish(edit.run(title: "Save Selection"), to: document, name: "Save Selection",
                record: .saveSelection(to: destination, mode: mode.agentName, in: doc))
        } catch { document.presentError(error) }
    }

    func fillNativePlane(at point: CGPoint) {
        guard let document else { return }
        do {
            let capture = try PrecisionEditorCapture(document)
            let options = ToolOptionsStore.shared.fill
            let x = Int(floor(point.x)), y = Int(floor(point.y))
            if let selected = capture.selection?.maskSamples16(), x >= 0, y >= 0,
               x < capture.facade.width, y < capture.facade.height,
               selected[y * capture.facade.width + x] == 0 { return }
            let opacity = min(max(options.opacity, 0), 100) / 100
            // The bar's Contents: foreground or background.
            let contents = fillToolColor
            let color = contents.withAlphaComponent(contents.alphaComponent * CGFloat(opacity))
            let ink = try PrecisionInk(color: color,
                space: ColorProfile.coverageSpace(document.nsColorSpace, for: color))
            let value = try PrecisionScalarOperations.ink(ink)
            let edit = try PrecisionScalarOperations.edit(capture,
                operation: ["kind": "bucket_fill", "x": Int(point.x), "y": Int(point.y),
                    "tolerance": Int(options.tolerance.rounded()) * 257, "contiguous": options.contiguous,
                    "value": value, "opacity": ink.rgba[3]])
            if capture.target.isPlane, !document.confirmRasterize(layer: capture.layer) { return }
            try capture.requireCurrent(document)
            var record: [ActionStep] = .fill(at: point, color: color, tolerance: Int(options.tolerance.rounded()),
                contiguous: options.contiguous, target: capture.target.agentName(in: capture.facade))
            for index in record.indices {
                record[index].arguments["rgba"] = ink.rgba; record[index].arguments["color_space"] = "document"
                record[index].arguments["fill_mode"] = "color_match"
            }
            _ = try capture.publish(edit.run(title: "Fill"), to: document, name: "Fill", record: record)
        } catch { document.presentError(error) }
    }
}
