import AppKit
import UniformTypeIdentifiers

extension AppDelegate {
    /// File ▸ Load Files into Stack…: several image files become the
    /// layers of ONE new document, bottom-first in file-name order. It lives
    /// here, not on `EditorViewController`, for `newDocument`'s reason — it
    /// has to work at the Welcome window, before any document exists. The
    /// work is `ImageDocument.openStack` (LoadFilesIntoStack.swift); MCP twin
    /// `load_files_into_stack`, which keeps the caller's own order.
    @objc func loadFilesIntoStack(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.title = "Load Files into Stack"
        panel.message = "Choose two or more images. Each becomes a layer of one new document."
        panel.prompt = "Load"
        panel.allowsMultipleSelection = true
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = ImageDocument.readableTypes.compactMap(UTType.init)
        guard panel.runModal() == .OK else { return }
        loadFilesIntoStack(urls: panel.urls)
    }

    /// The action past its open panel: the files in natural name order become
    /// one new document, and every alert the action shows is raised here.
    func loadFilesIntoStack(urls: [URL]) {
        // Natural file-name order ("frame 2" before "frame 10"), as Finder lists them.
        let urls = urls.sorted {
            $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending
        }
        do {
            let outcome = try ImageDocument.openStack(urls, name: nil).outcome
            guard let notice = outcome.notice else { return }
            let alert = NSAlert()
            alert.alertStyle = .informational
            alert.messageText = "Load Files into Stack"
            alert.informativeText = notice
            alert.runModal()
        } catch is JobCancelled {
            return
        } catch let error as CocoaError where error.code == .userCancelled {
            return
        } catch {
            let alert = NSAlert()
            alert.messageText = "Could Not Load the Files into a Stack"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }
}
