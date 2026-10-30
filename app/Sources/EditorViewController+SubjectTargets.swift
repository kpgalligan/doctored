import AppKit

extension EditorViewController {
    @objc func showSubjectTargets(_ sender: Any?) {
        guard let document, !isTransforming else { NSSound.beep(); return }
        do { presentAsSheet(try SubjectTargetsSheetController(editor: self, document: document)) }
        catch { NSApp.presentError(error) }
    }

    /// Select ▸ Select People…: the picker over individual people.
    @objc func showPeopleTargets(_ sender: Any?) {
        guard let document, !isTransforming else { NSSound.beep(); return }
        do { presentAsSheet(try SubjectTargetsSheetController(editor: self, document: document, model: .people)) }
        catch { NSApp.presentError(error) }
    }

    @objc func selectPeople(_ sender: Any?) {
        guard let document, !isTransforming else { NSSound.beep(); return }
        do {
            let capture = try SelectionCapture(document: document)
            let mask = try SubjectTargeting.people(capture, in: document)
            try capture.commit(.mask(mask), in: document, name: "Select All People",
                record: .step("select_people", note: "Select ▸ All People"))
        } catch { NSApp.presentError(error) }
    }

    func applySubjectTargets(_ capture: SelectionCapture, analysis: SubjectAnalysis, instances: [Int],
                             mode: SelectionCombineMode, feather: Double) {
        guard let document else { return }
        do {
            let mask = try SubjectTargeting.mask(analysis, instances: instances, capture: capture, in: document)
            var arguments: [String: Any] = ["instances": instances, "mode": mode.agentName]
            // Named only when they differ from the objects default, so an
            // objects step records exactly what it always did.
            if analysis.model == .people { arguments["model"] = analysis.model.rawValue }
            if feather > 0 { arguments["feather"] = feather }
            try capture.commit(.mask(mask), in: document, name: analysis.model.title, mode: mode,
                               feather: feather,
                               record: .step("select_subjects", arguments, note: analysis.model.recordNote))
        } catch { NSApp.presentError(error) }
    }

    /// The people picker's All People: the analysis' aggregate coverage,
    /// committed and recorded as Select ▸ All People.
    func applyAllPeople(_ capture: SelectionCapture, analysis: SubjectAnalysis, mode: SelectionCombineMode,
                        feather: Double) {
        guard let document else { return }
        guard let mask = analysis.aggregateMask else { NSSound.beep(); return }
        do {
            var arguments: [String: Any] = ["mode": mode.agentName]
            if feather > 0 { arguments["feather"] = feather }
            try capture.commit(.mask(mask), in: document, name: "Select All People", mode: mode,
                               feather: feather,
                               record: .step("select_people", arguments, note: "Select ▸ All People"))
        } catch { NSApp.presentError(error) }
    }
}
