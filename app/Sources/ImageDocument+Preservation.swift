import AppKit

/// Ordinary Save preserves editable state. Explicit File > Export and MCP
/// save_copy may flatten without retargeting the document or clearing dirty.
extension ImageDocument {
    /// NSDocument's close-time Save enters here directly. Keeping routing
    /// here makes the menu and the unsaved-changes dialog choose the same
    /// preserving format, while forwarding AppKit's completion callback.
    override func save(
        withDelegate delegate: Any?, didSave didSaveSelector: Selector?,
        contextInfo: UnsafeMutableRawPointer?
    ) {
        guard commitPendingCanvasSessions() else {
            answerDocumentCallback(delegate, didSaveSelector, contextInfo: contextInfo)
            return
        }
        if fileURL != nil, let fileType = fileType,
           !writableTypes(for: .saveOperation).contains(fileType) {
            runModalSavePanel(
                for: .saveAsOperation, delegate: delegate, didSave: didSaveSelector,
                contextInfo: contextInfo)
            return
        }
        super.save(withDelegate: delegate, didSave: didSaveSelector, contextInfo: contextInfo)
    }

    func saveLosses(for typeName: String) throws -> [String] {
        var losses = try modeSaveLosses(for: typeName)
        // #1 1c-1: a typed document's flattened file carries its spot channels' overprint, not
        // the channels (the byte list's `flatSaveLosses` names them in the core); a TIFF keeps
        // them as extra samples (1c-2a).
        if typeName != Self.nativeTypeIdentifier, ExportFormat.from(fileType: typeName) != .tiff,
           let doc, doc.isHighDepth, doc.hasSpotChannels {
            losses.append(SpotChannels.flatLoss)
        }
        return losses
    }

    private func modeSaveLosses(for typeName: String) throws -> [String] {
        guard let doc = doc else { throw DoctoredCoreError(message: "No image loaded.") }
        if typeName == Self.nativeTypeIdentifier { return [] }
        // #1: a CMYK document writes TIFF and JPEG as its composite on
        // paper, so a one-layer document loses nothing and a layered one its
        // layers; no other flat format holds CMYK.
        if doc.isCmyk {
            guard let format = ExportFormat.from(fileType: typeName), format == .tiff || format == .jpeg
            else { return ["editable CMYK document state; a CMYK document saves as .doctored, TIFF or JPEG"] }
            return doc.layerCount > 1
                ? ["editable CMYK layers (\(format.displayName) holds the composite flattened onto paper)"] : []
        }
        // #1 Bitmap phase 1c: a Bitmap document saves as .doctored, a 1-bit PNG, TIFF or BMP
        // keeping its plane, or JPEG through Gray; `[]` only when the copy reopens as the same
        // document.
        if doc.isBitmap {
            return BitmapDocument.saveLosses(doc, format: ExportFormat.from(fileType: typeName),
                                             embedProfile: embedColorProfile,
                                             stripMetadata: stripMetadata)
        }
        // #1 Indexed phase 1b: an Indexed Color document saves as .doctored, PNG, GIF or TIFF
        // keeping its palette; `[]` only when the copy reopens as the same document.
        if doc.isIndexed {
            return IndexedDocument.saveLosses(doc, format: ExportFormat.from(fileType: typeName),
                                              embedProfile: embedColorProfile,
                                              stripMetadata: stripMetadata)
        }
        // #1 Lab phase 1b: a Lab Color document saves as .doctored or Lab TIFF
        // (`LabDocument.saveRefusal`); a TIFF holds the flattened composite and is never
        // empty-lossed, so Save As keeps `.doctored` and Save a Copy writes the TIFF.
        if doc.isLab {
            if ExportFormat.from(fileType: typeName) == .tiff {
                return ["editable Lab Color document state: TIFF holds the flattened composite, and "
                    + "Doctored opens a Lab TIFF from #1 phase 3"]
            }
            return ["editable Lab Color document state; a Lab Color document saves as .doctored or Lab TIFF"]
        }
        // #1 Duotone phase 1b: a Duotone document saves as .doctored, or as PSD or PSB through
        // File ▸ Export as PSD; every other format is an export through its inks.
        if doc.isDuotone {
            return ["editable Duotone document state; a Duotone document saves as .doctored, or as PSD or "
                + "PSB through File ▸ Export as PSD"]
        }
        // #1 phase 1b: a Grayscale document writes PNG, TIFF and JPEG as one channel (its
        // composite), so only one visible Normal raster layer at full opacity with no mask or
        // style, at a depth the format holds, loses nothing.
        if doc.isGray {
            guard let format = ExportFormat.from(fileType: typeName), [.png, .tiff, .jpeg].contains(format)
            else { return ["editable Grayscale document state; a Grayscale document saves as .doctored, PNG, TIFF or JPEG"] }
            if doc.isFloat {
                return ["editable 32-bit Grayscale document state; use Export for an explicitly flattened copy"]
            }
            if doc.layerCount > 1 {
                return ["editable Grayscale layers (\(format.displayName) holds the composite)"]
            }
            if format == .jpeg, doc.grayDepth == 16 {
                return ["16-bit Grayscale precision (JPEG holds 8 bits)"]
            }
            guard let layer = doc.layerInfo(0), layer.kind == DR_LAYER_RASTER, layer.visible,
                  layer.opacity == 1, layer.blendMode == DR_BLEND_NORMAL,
                  !doc.layerHasMask(0), !doc.layerHasStyle(0) else {
                return ["editable Grayscale layer state: its mask, style, opacity, blend mode or "
                    + "visibility (\(format.displayName) holds the composite)"]
            }
            return []
        }
        if doc.isFloat {
            return ["editable 32-bit float document state; use Export for an explicitly flattened copy"]
        }
        if doc.precision != nil {
            return ["editable 16-bit document state; use Export for an explicitly flattened copy"]
        }
        guard let format = ExportFormat.from(fileType: typeName) else {
            throw DoctoredCoreError(message: "Cannot write files of type \(typeName).")
        }
        return try doc.flatSaveLosses(
            format: format.drFormat, embedProfile: embedColorProfile, stripMetadata: stripMetadata)
    }

    func canPreserveState(in typeName: String) -> Bool {
        (try? saveLosses(for: typeName).isEmpty) == true
    }

    /// Preflight before AppKit starts its destination replacement protocol.
    /// Checking only the lower write override is too late for direct callers
    /// of writeSafely, including inherited autosave paths.
    override func writeSafely(
        to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType
    ) throws {
        if saveOperation != .saveToOperation {
            try requirePreservingSave(typeName)
        }
        let captured = try CapturedSave(self, type: typeName)
        _ = try RasterResources.shared.admit(scratch: captured.scratchEstimate,
            own: captured.ownRetainedBytes, remedy: captured.refusalRemedy)
        let task = try WorkerTask(scratchEstimate: captured.scratchEstimate) { try captured.writeSafely(to: url) }
        try BackgroundWork.wait(task, title: "Saving “\(url.lastPathComponent)”")
        if saveOperation != .saveToOperation { lastWrittenDocument = captured.source }

    }

    /// NSDocument.h requires custom safe writers that do not move its backup
    /// to return nil, so any version preservation occurs before this write.
    override var backupFileURL: URL? { nil }

    /// Also protects direct callers of the lower-level writing entry point.
    override func write(
        to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType,
        originalContentsURL absoluteOriginalContentsURL: URL?
    ) throws {
        if saveOperation != .saveToOperation {
            try requirePreservingSave(typeName)
        }
        try super.write(
            to: url, ofType: typeName, for: saveOperation,
            originalContentsURL: absoluteOriginalContentsURL)
    }

    func requirePreservingSave(_ typeName: String) throws {
        let losses = try saveLosses(for: typeName)
        guard losses.isEmpty else {
            let name = ExportFormat.from(fileType: typeName)?.displayName ?? typeName
            throw DoctoredCoreError(message:
                "\(name) cannot preserve \(losses.joined(separator: ", ")). "
                + "Save as a Doctored Document (.doctored) to keep this work, "
                + "or use File > Export to create a flat copy.")
        }
    }
}
