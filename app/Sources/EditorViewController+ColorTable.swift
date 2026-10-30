import AppKit

// Image ▸ Mode ▸ Color Table… (#1 Indexed phase 1c): the dialog (`ColorTableSheetController`,
// `.document`) over an Indexed Color document's table, then the edit as ONE undo step
// (`IndexedColorTableEdit`, recorded as `set_color_table`). Enabled only on an Indexed document,
// not during a live edit or other work. The MCP twins are `get_color_table` and
// `set_color_table` (`AgentServer+IndexedColorTable.swift`).

extension EditorViewController {
    @objc func showColorTable(_ sender: Any?) {
        guard let document, let typed = document.doc?.precision, document.doc?.isIndexed == true else {
            NSSound.beep()
            return
        }
        do {
            let table = try typed.indexedColorTable()
            presentAsSheet(ColorTableSheetController(
                table: table, purpose: .document,
                space: ColorTableSheetController.tableSpace(for: document.doc),
                refusal: { [weak self] in self?.colorTableRefusal($0) },
                commit: { [weak self] in self?.commitColorTable($0) }))
        } catch {
            NSSound.beep()
            document.presentError(error)
        }
    }

    /// The core's refusal of `table` on the current document (the dialog's OK asks it first).
    private func colorTableRefusal(_ table: IndexedColorTable) -> String? {
        guard let document else { return "The document closed." }
        do {
            return try IndexedColorTableEdit(document, table: table).refusal
        } catch {
            return error.localizedDescription
        }
    }

    /// One Color Table edit; an unchanged table does nothing.
    private func commitColorTable(_ table: IndexedColorTable) {
        guard let document else { return }
        do {
            let edit = try IndexedColorTableEdit(document, table: table)
            guard edit.changes else { return }
            let prepared = try edit.compute()
            try edit.publish(prepared, to: document, record: [edit.step()])
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
        }
    }

    /// Image ▸ Mode ▸ Color Table…: enabled only on an Indexed Color document, not during a live
    /// edit or other work. nil for every other item.
    func validateColorTableItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard item.action == #selector(showColorTable(_:)) else { return nil }
        guard let document, document.doc?.isIndexed == true else { return false }
        return !document.isLiveEditing && !BackgroundWork.occupied
    }
}
