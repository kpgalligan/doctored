import AppKit

// The MCP `document_id` map. `AgentServer.target` used to rebuild a set of the open
// documents and filter the whole map for every candidate it compared, so one call was O(n²) in
// open documents; a lookup here is O(1), plus one `===` scan of the open list to refuse a closed
// document.

/// The stable tool-facing id of each document an agent has seen. Ids are
/// assigned on first sight, counted up from 1 and never reused. Main thread.
final class AgentDocumentIDs {
    private final class WeakBox {
        weak var document: ImageDocument?
        init(_ document: ImageDocument) { self.document = document }
    }

    private var byObject: [ObjectIdentifier: Int] = [:]
    private var byID: [Int: WeakBox] = [:]
    private var next = 1

    /// The document's id, assigning the next one on first sight.
    func id(for document: ImageDocument) -> Int {
        let key = ObjectIdentifier(document)
        // The key is the object's address, so a closed document's entry would
        // be inherited by whatever ImageDocument the allocator later put in
        // the same place — after which document_id resolved to the wrong
        // document. A batch of 200 files makes that certain rather than
        // unlikely. An entry counts only while its weak reference is still
        // this document.
        if let existing = byObject[key], byID[existing]?.document === document { return existing }
        let id = next
        next += 1
        byID = byID.filter { $0.value.document != nil }
        byObject = byObject.filter { byID[$0.value] != nil }
        byObject[key] = id
        byID[id] = WeakBox(document)
        return id
    }

    /// The document with `id`, or nil unless it is among `open`.
    func document(withID id: Int, among open: [ImageDocument]) -> ImageDocument? {
        guard let document = byID[id]?.document, open.contains(where: { $0 === document }) else {
            return nil
        }
        return document
    }
}
