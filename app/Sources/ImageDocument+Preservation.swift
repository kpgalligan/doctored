import AppKit

extension RasterDocument {
    /// An Indexed Color or Bitmap document's editable-state lines beside its combined line
    /// (`oneLayerModeCombinedLoss`), which already names the saved channels, paths, colour
    /// samplers, guides and ruler origin: the core's other lines (`flatStateLosses`), such as the
    /// one layer's name, visibility, lock and label. The core reads no pixel line for these
    /// modes: the file is the policy's plane.
    func oneLayerModeLosses() throws -> [String] {
        guard let typed = precision else { return [] }
        let combined: Set<String> = [
            "saved selection channels", "guides and ruler origin", "color samplers", "paths",
        ]
        return try typed.flatStateLosses(alphaKept: true, tiff: false).filter { !combined.contains($0) }
    }

    /// An Indexed Color or Bitmap document's combined line for a kept `format` copy, nil when
    /// none of its states applies: the saved channels, paths, colour samplers, guides and ruler
    /// origin a flattened copy drops. A TIFF keeps the paths when it writes every one (the core's
    /// `flatStateLosses` with `tiff`, `tiff_paths`), so its line leaves "paths" out and a path
    /// alone adds none; every other format's line is unchanged.
    func oneLayerModeCombinedLoss(format: ExportFormat) throws -> String? {
        var tiffKeepsPaths = false
        if format == .tiff, let typed = precision {
            tiffKeepsPaths = try !typed.flatStateLosses(alphaKept: true, tiff: true).contains("paths")
        }
        let origin = rulerOrigin
        let pathsLost = !paths.entries.isEmpty && !tiffKeepsPaths
        guard channelCount > 0 || guideCount > 0 || origin.x != 0 || origin.y != 0 || pathsLost
            || !colorSamplers.items.isEmpty else { return nil }
        let states = tiffKeepsPaths ? "saved channels" : "saved channels, paths"
        return "\(states), colour samplers, guides and the ruler origin (flattened copy)"
    }

    /// The capture packets a flat `format` file of this document drops: "capture metadata"
    /// when they are stripped or the format has no packet slot at all (GIF, BMP), else each
    /// packet the core's carry table (`RasterProfile.carried(by:)`) has no slot for, in the byte
    /// list's words (`DrDocument::flat_save_losses`): a PNG has none for IPTC.
    func flatPacketLosses(format: ExportFormat, stripMetadata: Bool) -> [String] {
        let packets: [(kind: DrMetadataKind, bit: UInt32, line: String)] = [
            (DR_METADATA_EXIF, DR_CARRIES_EXIF, "EXIF metadata"),
            (DR_METADATA_XMP, DR_CARRIES_XMP, "XMP metadata"),
            (DR_METADATA_IPTC, DR_CARRIES_IPTC, "IPTC metadata"),
        ].filter { metadata($0.kind) != nil }
        guard !packets.isEmpty else { return [] }
        let carried = RasterProfile.carried(by: format.drFormat)
        if stripMetadata || carried & (DR_CARRIES_EXIF | DR_CARRIES_XMP | DR_CARRIES_IPTC) == 0 {
            return ["capture metadata"]
        }
        return packets.filter { carried & $0.bit == 0 }.map(\.line)
    }
}

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
        // them as extra samples where the core writes them (1c-2a, `SpotChannels.flatTiffKeeps`),
        // and a Lab TIFF refuses them.
        if typeName != Self.nativeTypeIdentifier, let doc, doc.isHighDepth, doc.hasSpotChannels {
            let format = ExportFormat.from(fileType: typeName)
            if format == .tiff && doc.isLab {
                losses.append(LabDocument.tiffSpotLoss)
            } else if format != .tiff || !SpotChannels.flatTiffKeeps(doc,
                bitDepth: PrecisionExportCapture.saveBitDepth(self, format: .tiff)) {
                losses.append(SpotChannels.flatLoss)
            }
        }
        return losses
    }

    private func modeSaveLosses(for typeName: String) throws -> [String] {
        guard let doc = doc else { throw DoctoredCoreError(message: "No image loaded.") }
        if typeName == Self.nativeTypeIdentifier { return [] }
        // A CMYK document writes TIFF and JPEG as its composite on paper, with no alpha: it
        // loses whatever editable state the core finds and the file has no place for.
        if doc.isCmyk {
            return try CmykDocument.saveLosses(doc, format: ExportFormat.from(fileType: typeName),
                                               embedProfile: embedColorProfile,
                                               stripMetadata: stripMetadata)
        }
        // #1 Bitmap phase 1c: a Bitmap document saves as .doctored, a 1-bit PNG, TIFF or BMP
        // keeping its plane, or JPEG through Gray; `[]` only when the copy reopens as the same
        // document.
        if doc.isBitmap {
            return try BitmapDocument.saveLosses(doc, format: ExportFormat.from(fileType: typeName),
                                             embedProfile: embedColorProfile,
                                             stripMetadata: stripMetadata)
        }
        // #1 Indexed phase 1b: an Indexed Color document saves as .doctored, PNG, GIF or TIFF
        // keeping its palette; `[]` only when the copy reopens as the same document.
        if doc.isIndexed {
            return try IndexedDocument.saveLosses(doc, format: ExportFormat.from(fileType: typeName),
                                              embedProfile: embedColorProfile,
                                              stripMetadata: stripMetadata)
        }
        // #1 Lab phase 1b: a Lab Color document saves as .doctored or Lab TIFF
        // (`LabDocument.saveRefusal`); `[]` only when the TIFF reopens as the same document.
        if doc.isLab {
            return try LabDocument.saveLosses(doc, format: ExportFormat.from(fileType: typeName),
                                              stripMetadata: stripMetadata)
        }
        // #1 Duotone phase 1b: a Duotone document saves as .doctored, or as PSD or PSB through
        // File ▸ Export as PSD; every other format is an export through its inks.
        if doc.isDuotone {
            return ["editable Duotone document state; a Duotone document saves as .doctored, or as PSD or "
                + "PSB through File ▸ Export as PSD"]
        }
        // A Grayscale document writes PNG, TIFF and JPEG as one channel (its composite) at
        // `GrayDocument.defaultBitDepth` (a 32-bit document's TIFF at 32): it loses whatever
        // editable state the core finds and the file has no place for.
        if doc.isGray {
            let format = ExportFormat.from(fileType: typeName)
            return try GrayDocument.saveLosses(doc, format: format,
                bitDepth: format.map { GrayDocument.defaultBitDepth(doc, format: $0) } ?? 8,
                embedProfile: embedColorProfile, stripMetadata: stripMetadata)
        }
        guard let format = ExportFormat.from(fileType: typeName) else {
            throw DoctoredCoreError(message: "Cannot write files of type \(typeName).")
        }
        // A 16- or 32-bit RGB document writes its composite at Save's depth
        // (`PrecisionExportCapture.saveBitDepth`: a 32-bit TIFF at 32): it loses whatever
        // editable state the core finds and the file has no place for.
        if doc.precision != nil {
            return try PrecisionExportCapture.losses(doc, format: format,
                bitDepth: PrecisionExportCapture.listedBitDepth(self, format: format),
                embedProfile: embedColorProfile, stripMetadata: stripMetadata)
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
