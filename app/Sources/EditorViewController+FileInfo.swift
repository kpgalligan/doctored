import AppKit

/// File ▸ File Info… — the document's creator, copyright notice, description
/// and keywords (`FileInfo.swift`).
///
/// The sheet edits a copy; this file commits it, the
/// `EditorViewController+Evaluation` shape: the setter re-runs on the
/// document's CURRENT handle inside `applyEdit`, as one undo step, and the
/// core's refusal (a field past its limit) is presented as its own sentence.
extension EditorViewController {
    @objc func showFileInfo(_ sender: Any?) {
        guard let sheet = FileInfoSheetController(editor: self) else {
            NSSound.beep()
            return
        }
        presentAsSheet(sheet)
    }

    /// Commits the sheet's fields as ONE undo step. `shown` is what the sheet
    /// opened with: the recorded step carries only the fields the user
    /// changed, so replaying it leaves a target document's other fields
    /// alone. A form left as it opened makes no edit at all.
    func commitFileInfo(_ edited: FileInfo, shown: FileInfo) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        guard edited != shown else { return }
        // What Image Size passes for its resolution half: a change that moves
        // no pixel still goes through `applyEdit` with its defaults.
        document.applyEdit("File Info", record: .setFileInfo(from: shown, to: edited)) { source in
            // Only the changed fields are written over the handle's own, as
            // the MCP tool merges an omitted key.
            var wanted = source.fileInfo
            if edited.creator != shown.creator { wanted.creator = edited.creator }
            if edited.copyright != shown.copyright { wanted.copyright = edited.copyright }
            if edited.description != shown.description { wanted.description = edited.description }
            if edited.keywords != shown.keywords { wanted.keywords = edited.keywords }
            return try source.settingFileInfo(wanted)
        }
    }

    /// Enablement for the menu item: any document with an image.
    func validateFileInfoItem() -> Bool { document?.doc != nil }
}
