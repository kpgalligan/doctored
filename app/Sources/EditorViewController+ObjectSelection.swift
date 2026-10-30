import AppKit

/// The Object Selection tool's controller side: the tool action, the
/// drag's commit and the options bar's Mode. The model and the canvas gesture
/// are in `ObjectSelection.swift`; `object_select` is its MCP mirror.
extension EditorViewController {
    @objc func selectObjectSelectTool(_ sender: Any?) { selectTool(.objectSelect) }

    /// A finished drag: find the object inside it, refine its edge (by
    /// default not at 32-bit float, `ObjectSelection.refines`) and commit the selection as
    /// one step, recorded as `object_select`.
    func finishObjectSelection(_ session: ObjectSelectionSession) {
        guard let document else { return }
        if session.mode == .rectangle, ObjectSelectionSettings.shared.objectFinder {
            finishObjectFinder(session, in: document)
            return
        }
        do {
            guard let region = session.region else { return }
            let refine = ObjectSelection.refines(session.capture.doc)
            let found = try ObjectSelection.shape(session.capture, in: document,
                region: region.shape, bounds: region.bounds, refine: refine)
            var arguments: [String: Any] = ["mode": session.combine.agentName,
                                            "feather": session.feather, "refine": refine]
            switch session.mode {
            case .rectangle:
                arguments["region"] = [
                    "x": Int(region.bounds.minX), "y": Int(region.bounds.minY),
                    "width": Int(region.bounds.width), "height": Int(region.bounds.height),
                ]
            case .lasso:
                arguments["points"] = ActionArgs.points(session.points)
            }
            try session.capture.commit(found.shape, in: document, name: "Object Selection",
                mode: session.combine, feather: session.feather,
                record: .step("object_select", arguments, note: "Object Selection tool"))
        } catch { NSApp.presentError(error) }
        syncSelectionHistory()
    }

    /// Object Finder: a click is a point prompt, a rectangle a box prompt, and a
    /// Shift- or ⌥-click on a live prompt adds an include or exclude point to it
    /// (`ObjectFinder.select`), recorded as `object_finder_select` with the whole prompt so far.
    /// A missing model offers Settings ▸ Models.
    func finishObjectFinder(_ session: ObjectSelectionSession, in document: ImageDocument) {
        guard let model = ModelGate.ready("object_prompt") else {
            ModelGate.offerSettings(ModelGate.notInstalled("object_prompt"))
            return
        }
        do {
            let shift = session.modifiers.contains(.shift), option = session.modifiers.contains(.option)
            let current = session.isClick && shift != option ? ObjectFinder.currentPrompt(document) : nil
            var points: [ObjectFinderPoint] = []
            var box: CGRect?
            if session.isClick {
                guard let first = session.points.first else { return }
                let point = ObjectFinderPoint(x: Double(first.x), y: Double(first.y), include: current == nil || shift)
                points = (current?.points ?? []) + [point]
                box = current?.box
            } else {
                let dragged = session.rect
                guard dragged.width >= 1, dragged.height >= 1 else { return }
                // Whole pixels, so the run prompts the same box the recorded step replays.
                box = dragged.integral
            }
            var arguments: [String: Any] = [:]
            if !points.isEmpty {
                arguments["points"] = points.map {
                    ["x": ActionArgs.number($0.x), "y": ActionArgs.number($0.y), "include": $0.include]
                }
            }
            if let box {
                arguments["box"] = ["x": Int(box.minX), "y": Int(box.minY),
                                    "width": Int(box.width), "height": Int(box.height)]
            }
            if current != nil {
                arguments["continue"] = true
            } else {
                arguments["mode"] = session.combine.agentName
                arguments["feather"] = session.feather
            }
            _ = try ObjectFinder.select(session.capture, in: document, points: points, box: box,
                mode: session.combine, feather: session.feather, continuing: current != nil,
                record: .step("object_finder_select", arguments,
                              note: ModelGate.note(ObjectFinder.notePath, model.entry)))
        } catch is JobCancelled {
        } catch { NSApp.presentError(error) }
        syncSelectionHistory()
    }

    /// Mode: Rectangle | Lasso, and the Object Finder switch.
    func objectSelectionClusters() -> [OptionCluster] {
        [OptionCluster([
            OptionDescriptor(id: "object.mode", overflowLabel: "Mode",
                kind: .popup(width: 110, items: ObjectSelectionMode.allCases.map(\.title),
                    get: { ObjectSelectionMode.allCases.firstIndex(of: ObjectSelectionSettings.shared.mode) ?? 0 },
                    set: { index in
                        guard ObjectSelectionMode.allCases.indices.contains(index) else { return }
                        ObjectSelectionSettings.shared.mode = ObjectSelectionMode.allCases[index]
                    })),
        ]), OptionCluster([
            OptionDescriptor(id: "object.finder", overflowLabel: "Object Finder",
                kind: .checkbox(
                    label: "Object Finder",
                    get: { ObjectSelectionSettings.shared.objectFinder },
                    set: { ObjectSelectionSettings.shared.objectFinder = $0 })),
        ])]
    }
}
