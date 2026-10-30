import AppKit

// The six `ImageDocument` entry points are UI-ONLY: an MCP handler must go through `editStructure`,
// because `performGroupedEdit`'s `withUndoGroup` is what keeps agent edits separate undo steps
// (`AgentServer+LayerLabels.swift`'s precedent).

/// The six UI entry points for LAYER COMPS — the Layer Comps window's
/// buttons, its drag-reorder and the Window menu's item.
///
/// Every one of them is one `applyEdit` through
/// `RasterDocument.editingLayerComps(_:)`, the single shared writer, so a
/// comp mutation is one undo step and one recorded Action step exactly like
/// every other document edit. These are UI-ONLY: an MCP handler must never
/// call them, because an agent edit has to open its own undo group
/// (`AgentServer.performGroupedEdit`) and goes through `editStructure`
/// instead.
///
/// A refusal is the core's own sentence, presented rather than beeped: a
/// duplicate comp name and the 64-comp limit are both worth reading.
extension ImageDocument {
    /// Saves the current per-layer state as a new comp.
    func saveLayerComp(name: String, comment: String, captures: LayerCompCaptures) throws {
        var request: [String: Any] = ["action": "save", "name": name, "comment": comment]
        request.merge(captures.arguments) { _, new in new }
        try commitLayerComps("Save Layer Comp", .saveLayerComp(name, comment, captures), request)
    }

    /// Applies the comp called `name`, reporting what it could not honour.
    /// The report is built from the comp read BEFORE the edit, so the names
    /// it carries are the names the list showed.
    @discardableResult
    func applyLayerComp(named name: String) throws -> LayerCompApplyReport {
        let comp = try requireLayerComp(named: name)
        var changed = false
        try commitLayerComps(
            "Apply Layer Comp", .applyLayerComp(name),
            ["action": "apply", "comp": comp.index], noOpIsFine: true, changed: &changed)
        // The document now holds this comp's state whether or not an undo
        // step was opened, which is exactly what the window's ◀ ▶ step from.
        lastAppliedLayerComp = name
        return LayerCompApplyReport.applyReport(for: comp, changed: changed)
    }

    /// Renames, re-comments, re-flags and optionally re-records a comp.
    func updateLayerComp(
        named name: String, newName: String?, comment: String?,
        captures: LayerCompCaptures?, recapture: Bool
    ) throws {
        let comp = try requireLayerComp(named: name)
        var request: [String: Any] = [
            "action": "update", "comp": comp.index, "recapture": recapture,
        ]
        if let newName { request["name"] = newName }
        if let comment { request["comment"] = comment }
        if let captures { request.merge(captures.arguments) { _, new in new } }
        var changed = false
        try commitLayerComps(
            "Update Layer Comp",
            .updateLayerComp(name, newName: newName, comment: comment, captures: captures,
                             recapture: recapture),
            request, noOpIsFine: true, changed: &changed)
    }

    /// Deletes the comp called `name`.
    func deleteLayerComp(named name: String) throws {
        let comp = try requireLayerComp(named: name)
        try commitLayerComps(
            "Delete Layer Comp", .deleteLayerComp(name),
            ["action": "delete", "comp": comp.index])
    }

    /// Moves the comp called `name` to FINAL list index `to` (counted with it
    /// taken out, as `reorder_layer` counts) — the window's drag-reorder.
    /// Returns whether an undo step opened; the same index is the core's
    /// exact no-op and returns false.
    @discardableResult
    func reorderLayerComp(named name: String, to: Int) throws -> Bool {
        let comp = try requireLayerComp(named: name)
        var changed = false
        try commitLayerComps(
            "Reorder Layer Comp", .reorderLayerComp(name, to: to),
            ["action": "reorder", "comp": comp.index, "to": to], noOpIsFine: true,
            changed: &changed)
        return changed
    }

    /// Applies the comp one place before (`-1`) or after (`+1`) the one whose
    /// entries the document currently matches — the window's ◀ ▶ buttons.
    /// With nothing applied yet it starts at either end of the list.
    @discardableResult
    func applyAdjacentLayerComp(step: Int) throws -> LayerCompApplyReport {
        let comps = try requireLayerComps()
        guard let current = lastAppliedLayerComp,
              let at = comps.firstIndex(where: { $0.name == current })
        else {
            guard let first = step < 0 ? comps.last : comps.first else {
                throw DoctoredCoreError(message: "This document has no layer comps.")
            }
            return try applyLayerComp(named: first.name)
        }
        let next = at + step
        guard comps.indices.contains(next) else {
            throw DoctoredCoreError(
                message: step < 0
                    ? "That is the first layer comp." : "That is the last layer comp.")
        }
        return try applyLayerComp(named: comps[next].name)
    }

    /// Every comp of the current document.
    func requireLayerComps() throws -> [LayerComp] {
        guard let doc else { throw DoctoredCoreError(message: "This document has no image.") }
        return try doc.layerComps()
    }

    /// The comp called `name`, or a refusal naming it.
    func requireLayerComp(named name: String) throws -> LayerComp {
        guard let comp = try requireLayerComps().first(where: { $0.name == name }) else {
            throw DoctoredCoreError(message: "There is no layer comp called \"\(name)\".")
        }
        return comp
    }

    private func commitLayerComps(
        _ actionName: String, _ record: [ActionStep], _ request: [String: Any]
    ) throws {
        var changed = false
        try commitLayerComps(actionName, record, request, noOpIsFine: false, changed: &changed)
    }

    /// The one commit. `applyEdit` beeps on nil, so the core's refusal is
    /// pulled out FIRST — `editingLayerComps` is run once here, and the
    /// result handed to the transform — and presented as a sentence.
    private func commitLayerComps(
        _ actionName: String, _ record: [ActionStep], _ request: [String: Any],
        noOpIsFine: Bool, changed: inout Bool
    ) throws {
        guard let current = doc else {
            throw DoctoredCoreError(message: "This document has no image.")
        }
        let updated = try current.editingLayerComps(request)
        guard let updated else {
            guard noOpIsFine else {
                throw DoctoredCoreError(message: "\(actionName) changed nothing.")
            }
            changed = false
            return
        }
        applyEdit(actionName, record: record) { _ in updated }
        // `applyEdit` beeps and leaves the handle alone when the document
        // moved under it, so identity is the honest "did an undo step open".
        changed = doc === updated
    }
}
