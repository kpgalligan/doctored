import AppKit

// `EditingResourceReport` adds per-document history/snapshot unions and selection estimates; those
// overlapping reports must not be summed.

/// Main-thread companion to core's process-wide buffer inventory. These per-
/// document unions overlap with that inventory and with each other; do not sum
/// them. They add the selection estimates and explicit retention policies.
enum EditingResourceReport {
    static func collect() -> [[String: Any]] {
        assert(Thread.isMainThread)
        return NSDocumentController.shared.documents.compactMap { value in
            guard let document = value as? ImageDocument else { return nil }
            var result: [String: Any] = ["title": document.displayName ?? "Image",
                                         "history": document.history.report()]
            do { result["snapshots"] = try document.namedSnapshots.report() }
            catch { result["snapshot_accounting_error"] = error.localizedDescription }
            return result
        }
    }
}
