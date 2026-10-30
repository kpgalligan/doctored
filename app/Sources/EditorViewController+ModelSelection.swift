import AppKit

/// Select ▸ Sky and Select ▸ Person Parts…: the commands and their commit as one
/// selection Undo, recorded with the model that made it. The runs are in `ModelSelection.swift`
/// and `PersonParts.swift`; `select_sky` and `select_person_parts` are the MCP mirrors. A missing
/// model offers Settings ▸ Models.
extension EditorViewController {
    @objc func selectSky(_ sender: Any?) {
        guard let document, !isTransforming else { NSSound.beep(); return }
        guard let model = ModelGate.ready("sky") else {
            ModelGate.offerSettings(ModelGate.notInstalled("sky"))
            return
        }
        let mode = canvas.selectionCombineBase
        do {
            let capture = try SelectionCapture(document: document)
            let found = try SkySelection.shape(capture, in: document)
            try capture.commit(found.shape, in: document, name: "Select Sky", mode: mode,
                               record: .step("select_sky", ["mode": mode.agentName],
                                             note: ModelGate.note(SkySelection.menuPath, model.entry)))
        } catch is JobCancelled {
        } catch { NSApp.presentError(error) }
        syncSelectionHistory()
    }

    @objc func showPersonParts(_ sender: Any?) {
        guard let document, !isTransforming else { NSSound.beep(); return }
        guard ModelGate.ready("person_parts") != nil else {
            ModelGate.offerSettings(ModelGate.notInstalled("person_parts"))
            return
        }
        do { presentAsSheet(try PersonPartsSheetController(editor: self, document: document)) }
        catch is JobCancelled {}
        catch { NSApp.presentError(error) }
    }

    /// The sheet's Apply: `parts` on `person` (1-based; nil = All People), committed as one
    /// "Person Parts" step recorded as `select_person_parts`.
    func applyPersonParts(_ capture: SelectionCapture, analysis: SubjectAnalysis?, parts: [PersonPart],
                          person: Int?, mode: SelectionCombineMode, feather: Double) {
        guard let document else { return }
        do {
            let people = person.map { [$0] }
            let found = try PersonPartsRun.shape(capture, in: document, parts: parts, people: people,
                                                 analysis: analysis)
            var arguments: [String: Any] = ["parts": parts.map(\.rawValue), "mode": mode.agentName,
                                            "feather": feather]
            if let people { arguments["people"] = people }
            try capture.commit(found.shape, in: document, name: "Person Parts", mode: mode, feather: feather,
                               record: .step("select_person_parts", arguments,
                                             note: ModelGate.note(PersonPartsRun.menuPath, found.entry)))
        } catch is JobCancelled {
        } catch { NSApp.presentError(error) }
        syncSelectionHistory()
    }
}
