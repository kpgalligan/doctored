import AppKit

extension EditorViewController {
    func addPrecisionMask(kind: DrMaskKind) {
        guard let document else { return }
        do {
            let capture = try PrecisionEditorCapture(document)
            let width = capture.facade.width, height = capture.facade.height
            let input: PrecisionScalarBuffer
            if kind == DR_MASK_HIDE_ALL {
                _ = try RasterResources.shared.admit(scratch: UInt64(width * height) * 2 + 512)
                input = try PrecisionScalarBuffer(samples: [UInt16](repeating: 0, count: width * height), width: width, height: height)
            } else {
                if kind == DR_MASK_FROM_SELECTION, capture.selection == nil {
                    throw DoctoredCoreError(message: "Make a selection before creating its mask.")
                }
                input = try PrecisionStrokeCoverage.selection(width: width, height: height,
                    selection: kind == DR_MASK_FROM_SELECTION ? capture.selection : nil)
            }
            let work = try PrecisionPixelEdit(capture.source,
                command: .setMask(layer: capture.layer, width: width, height: height), input: input)
            let name = kind == DR_MASK_HIDE_ALL ? "hide_all" : kind == DR_MASK_FROM_SELECTION ? "from_selection" : "reveal_all"
            try capture.publish(work.run(), to: document, name: "Add Layer Mask", record: .addLayerMask(kind: name))
        } catch { presentPrecisionError(error) }
    }

    func clearPrecisionSelection() {
        guard let document else { return }
        do {
            let capture = try PrecisionEditorCapture(document)
            guard capture.selection != nil else { throw DoctoredCoreError(message: "Make a selection to clear.") }
            if capture.target.isCoverage {
                if capture.target.isPlane, !document.confirmRasterize(layer: capture.layer) { return }
                try capture.requireCurrent(document)
                let edit = try PrecisionScalarOperations.edit(capture, operation: ["kind": "fill", "value": 0])
                var record: [ActionStep] = .clearSelection(note: "Edit ▸ Clear")
                for i in record.indices { record[i].arguments["target"] = capture.target.agentName(in: capture.facade) }
                _ = try capture.publish(edit.run(title: "Clear Selection"), to: document, name: "Clear Selection", record: record)
                return
            }
            let coverage = try PrecisionStrokeCoverage.selection(width: capture.facade.width,
                height: capture.facade.height, selection: capture.selection)
            guard document.confirmRasterize(layer: capture.layer) else { return }
            try capture.requireCurrent(document)
            // #547 Indexed phase 3b: a cleared pixel with no transparent index takes the entry
            // nearest the background swatch.
            let command = try IndexedDocument.backgrounded(.erase(layer: capture.layer), backgroundColor,
                                                           in: capture.facade)
            let work = try PrecisionPixelEdit(capture.source, command: command, input: coverage)
            try capture.publish(work.run(), to: document, name: "Clear Selection", record: .clearSelection(note: "Edit ▸ Clear"))
        } catch { presentPrecisionError(error) }
    }

    func savePrecisionSelection() {
        guard let document else { return }
        do {
            let capture = try PrecisionEditorCapture(document)
            guard let selection = capture.selection else { throw DoctoredCoreError(message: "Make a selection first.") }
            let alert = NSAlert()
            alert.messageText = "Save Selection as 16-bit Channel"
            alert.informativeText = "The exact current coverage is saved separately from the image as a native 16-bit channel."
            let field = NSTextField(string: "Alpha \(capture.facade.channelCount + 1)")
            field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
            alert.accessoryView = field
            alert.addButton(withTitle: "Save Channel"); alert.addButton(withTitle: "Cancel")
            guard alert.runModal() == .alertFirstButtonReturn else { return }
            try capture.requireCurrent(document)
            let input = try PrecisionStrokeCoverage.selection(width: capture.facade.width,
                height: capture.facade.height, selection: selection)
            let name = field.stringValue
            let work = try PrecisionPixelEdit(capture.source, command: .setChannel(name: name), input: input)
            let step = ActionStep(tool: "save_selection", arguments: ["name": name, "mode": "replace"], note: "Save Selection")
            try capture.publish(work.run(), to: document, name: "Save Selection", record: [step])
        } catch { presentPrecisionError(error) }
    }
}
