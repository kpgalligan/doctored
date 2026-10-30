import AppKit

extension EditorViewController {
    @objc func redevelopRaw(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        guard commitPendingSessions() else { return }
        do {
            let capture = try ImageSourceCapture(document)
            let layer = document.activeLayerIndex
            let infoPayload = try PrecisionEditPayload.admitted(bytes: 1024 * 1024)
            defer { withExtendedLifetime(infoPayload) {} }
            let source = capture.source
            let restored = try RawLocalWork.restoreForRedevelop(source: source, layer: layer)
            try capture.requireCurrent(document)
            guard let result = try RawDevelopWindowController.runRetained(restored.probe,
                descriptor: restored.descriptor,
                snapshot: restored.snapshot, source: source, layer: layer, owner: document,
                validate: { [weak document] in
                    guard let document else { throw DoctoredCoreError(message: "The RAW document has closed.") }
                    try capture.requireCurrent(document)
                }) else { return }
            defer { withExtendedLifetime(result) {} }
            try capture.requireCurrent(document)
            try RawLocalWork.publish(prepared: result.document, in: document, capture: capture,
                layer: layer, settings: result.settings, operations: result.operations)
        } catch { document.presentError(error) }
    }

    @objc func migrateRawDecoder(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        guard commitPendingSessions() else { return }
        do {
            let layer = document.activeLayerIndex
            let candidate: RawDecoderMigration
            do {
                candidate = try RawDecoderMigration.prepare(in: document, layer: layer)
            } catch {
                guard error.localizedDescription.contains("supply a reviewed inverse migration map"),
                      let source = document.doc else { throw error }
                let saved = try source.sourceMaster(layer)
                let frame = try RawDecoderMigration.proposedFrame(in: document, layer: layer)
                let proposed = try RawMigrationGeometry.dimensions(frame)
                guard let map = RawMigrationGeometrySheetController.run(
                    saved: (saved.width, saved.height), proposed: proposed) else { return }
                candidate = try RawDecoderMigration.prepare(in: document, layer: layer, geometry: map)
            }
            let before = try candidate.capture.source.sourceMaster(candidate.layer)
            guard let after = candidate.asset.precision?.layerSource(0) else {
                throw DoctoredCoreError(message: "Could not inspect the proposed RAW development.")
            }
            let hint = "Saved decoder: \(candidate.previous.decoder.version) · \(candidate.previous.decoder.implementation)\n"
                + "Current decoder: \(candidate.descriptor.decoder.version) · \(candidate.descriptor.decoder.implementation)\n"
                + "Saved \(before.width)×\(before.height); proposed \(after.width)×\(after.height). "
                + "Reviewed source map: \((try candidate.geometry.json())). "
                + candidate.changes.joined(separator: " ")
            guard RawDevelopComparisonController.run(before: before, after: after,
                title: "Review RAW Decoder Migration", hint: hint, applyTitle: "Migrate Decoder") else { return }
            try candidate.apply(in: document)
        } catch { document.presentError(error) }
    }

    @objc func showRawSource(_ sender: Any?) {
        guard let document, let source = document.doc else { NSSound.beep(); return }
        do {
            let info = try RawSourceWork.info(source: source, layer: document.activeLayerIndex)
            let raw = info["raw"] as? [String: Any] ?? [:]
            let decoder = raw["decoder"] as? [String: Any] ?? [:]
            let recipe = raw["recipe"] as? [String: Any] ?? [:]
            let outputSpace = (recipe["outputSpace"] as? String).flatMap(RawOutputSpace.init(policy:))?.displayName ?? "saved profile"
            let layerDepth = source.isFloat ? "32-bit float" : "\(source.isHighDepth ? 16 : 8)-bit"
            // The master is the 16-bit SDR development or the 32-bit
            // float (linear) one.
            let float = (recipe["outputSpace"] as? String).flatMap(RawOutputSpace.encoding(policy:)) == .linearFloat
            let master = float ? "32-bit float (linear)" : "16-bit"
            let alert = NSAlert()
            alert.messageText = "Retained Camera RAW"
            alert.informativeText = "Original: \(raw["original_filename"] as? String ?? "Camera RAW")\n"
                + "Developed source: \(master) \(outputSpace); layer: \(layerDepth).\n"
                + "Decoder: \(decoder["version"] as? String ?? "Unknown")\n\n"
                + (info["redevelopment_note"] as? String ?? "") + "\n\n"
                + "The original camera file, effective recipe and saved rendering are embedded in native saves. Redevelop RAW updates this layer beneath existing retouch layers."
            if let window = view.window { alert.beginSheetModal(for: window) }
        } catch { document.presentError(error) }
    }
}
