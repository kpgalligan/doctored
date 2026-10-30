import AppKit

/// Select ▸ Face Parts…: the sheet, and its commit as one selection Undo.
extension EditorViewController {
    @objc func showFaceParts(_ sender: Any?) {
        guard let document, !isTransforming else { NSSound.beep(); return }
        do { presentAsSheet(try FacePartsSheetController(editor: self, document: document)) }
        catch { NSApp.presentError(error) }
    }

    func applyFaceParts(_ capture: SelectionCapture, analysis: FaceAnalysis, parts: [FacePart],
                        faces: [Int], mode: SelectionCombineMode, feather: Double) {
        guard let document else { return }
        do {
            let mask = try SubjectTargeting.faceMask(analysis, parts: parts, faces: faces,
                                                     capture: capture, in: document)
            var arguments: [String: Any] = ["parts": parts.map(\.rawValue), "mode": mode.agentName,
                                            "feather": feather]
            // Every face is the tool's default; a subset is named.
            if Set(faces) != Set(1...analysis.faces.count) { arguments["faces"] = faces.sorted() }
            try capture.commit(.mask16(mask), in: document, name: "Select Face Parts", mode: mode,
                               feather: feather,
                               record: .step("select_face_parts", arguments, note: "Select ▸ Face Parts…"))
        } catch { NSApp.presentError(error) }
    }
}
