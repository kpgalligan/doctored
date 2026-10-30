import AppKit

// Image ▸ Mode ▸ Color Table (#1 Indexed phase 1c): the colour table value, the Previous
// palette, and the table edit as ONE undo step (`LabDepthEdit`'s shape). The core decides
// everything (`indexed_table`): the new table replaces the palette and transparent index over the
// same index plane, so every pixel keeps its index and takes its entry's new colour; a table
// shorter than the largest index the image uses refuses, naming both. The dialog is
// `ColorTableSheetController`, the menu item `EditorViewController+ColorTable.swift`'s, the MCP
// twins `get_color_table` and `set_color_table` (`AgentServer+IndexedColorTable.swift`); the
// `.act` file is read and written by the core (`DoctoredCore+Indexed.swift`).

/// An Indexed Color table: 2…256 8-bit `[r, g, b]` entries and an optional transparent index.
struct IndexedColorTable: Equatable {
    var colors: [[UInt8]]
    var transparentIndex: Int?

    init(colors: [[UInt8]], transparentIndex: Int?) {
        self.colors = colors
        self.transparentIndex = transparentIndex
    }

    /// An integral code from JSON: a number with no fraction, never a JSON boolean (which bridges
    /// to NSNumber), as the core's `colors_from_json` reads it; nil otherwise.
    static func integralCode(_ value: Any) -> Int? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
              number.doubleValue.isFinite, number.doubleValue.rounded() == number.doubleValue else {
            return nil
        }
        return number.intValue
    }

    /// The core's JSON (`{"colors":[[r,g,b],…],"transparent_index":n|null}`); a malformed one
    /// throws.
    init(json: [String: Any]) throws {
        guard let list = json["colors"] as? [Any] else {
            throw DoctoredCoreError(message: "colors must be a list of [r, g, b] codes from 0 to 255.")
        }
        colors = try list.map { entry in
            guard let codes = entry as? [Any], codes.count == 3 else {
                throw DoctoredCoreError(message: "colors must be a list of [r, g, b] codes from 0 to 255.")
            }
            return try codes.map { code in
                guard let value = Self.integralCode(code), (0...255).contains(value) else {
                    throw DoctoredCoreError(message: "colors must be a list of [r, g, b] codes from 0 to 255.")
                }
                return UInt8(value)
            }
        }
        switch json["transparent_index"] {
        case nil, is NSNull:
            transparentIndex = nil
        case let number as NSNumber:
            transparentIndex = number.intValue
        default:
            throw DoctoredCoreError(message: "transparent_index must be an index or null.")
        }
    }

    /// The core's JSON.
    var json: [String: Any] {
        ["colors": colors.map { $0.map { Int($0) } },
         "transparent_index": transparentIndex.map { $0 as Any } ?? NSNull()]
    }

    /// The table without its transparent entry: the colours a Custom palette takes.
    var opaqueColors: [[UInt8]] {
        guard let transparentIndex, colors.indices.contains(transparentIndex) else { return colors }
        var out = colors
        out.remove(at: transparentIndex)
        return out
    }
}

/// The Indexed Color sheet's Previous palette (#1 Indexed phase 1c), app-wide:
/// the last Indexed Color conversion's palette with its transparent entry removed. A conversion
/// whose palette leaves fewer than 2 opaque entries clears it, so Previous never names an older
/// conversion.
enum IndexedPalettes {
    static private(set) var previous: IndexedColorTable?

    /// Notes `doc`'s palette as Previous (an Indexed Color conversion just published).
    static func notePrevious(_ doc: RasterDocument?) {
        guard let typed = doc?.precision, doc?.isIndexed == true,
              let table = try? typed.indexedColorTable() else { return }
        let colors = table.opaqueColors
        previous = colors.count >= 2 ? IndexedColorTable(colors: colors, transparentIndex: nil) : nil
    }
}

/// One captured Color Table edit: the source revision and the new table, planned by the core
/// before anything is computed.
struct IndexedColorTableEdit {
    let source: RasterDocument
    let revision: UInt64
    let editingState: EditingState
    let table: IndexedColorTable
    let activeLayer: Int
    let scratch: UInt64
    /// False when the table and transparent index equal the document's.
    let changes: Bool
    /// The core's refusal (a table shorter than the image's largest index needs), or nil.
    let refusal: String?

    init(_ document: ImageDocument, table: IndexedColorTable) throws {
        guard let source = document.doc, !document.isLiveEditing, !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Finish the current operation before editing the color table.")
        }
        guard let typed = source.precision, source.isIndexed else {
            throw DoctoredCoreError(message: "Color Table applies to an Indexed Color document.")
        }
        guard let state = document.captureEditingState() else {
            throw DoctoredCoreError(message: "Could not capture the document state.")
        }
        self.source = source
        revision = document.revision
        editingState = state
        self.table = table
        activeLayer = document.activeLayerIndex
        let plan = try typed.setColorTablePlan(table)
        refusal = plan["refusal"] as? String
        scratch = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
        changes = plan["changes"] as? Bool ?? false
    }

    /// The menu and undo title.
    var title: String { "Color Table" }

    /// The edit, on the bounded worker with its own reservation, then the display preparation.
    func compute() throws -> PreparedDocumentEdit {
        if let refusal { throw DoctoredCoreError(message: refusal) }
        guard let typed = source.precision else {
            throw DoctoredCoreError(message: "The document is not an Indexed Color document.")
        }
        let table = table, title = title
        let result: RasterDocument = try PrecisionCapturedWork.run(
            scratchEstimate: scratch, title: title) { control in
            let context = try PrecisionWorkContext(source: typed)
            guard let edited = try typed.setColorTable(table, workingLimit: context.limit,
                                                       otherRetained: context.otherRetained,
                                                       control: control) else {
                throw DoctoredCoreError(message: "\(title) changed nothing.")
            }
            return try RasterDocument(precision: edited)
        }
        let task = try WorkerTask { PreparedDocumentEdit(result) }
        let prepared = try BackgroundWork.wait(task, title: "Preparing Indexed Color Display")
        guard prepared.preview != nil, prepared.error == nil else {
            throw DoctoredCoreError(message: prepared.error ?? "Could not prepare the Indexed Color display.")
        }
        return prepared
    }

    /// ONE undo step.
    func publish(_ prepared: PreparedDocumentEdit, to document: ImageDocument, record: [ActionStep]) throws {
        guard document.doc === source, document.revision == revision, !document.isLiveEditing,
              document.isCurrentEditingState(editingState) else {
            throw DoctoredCoreError(message: "The document changed while \(title) was prepared. "
                + "Nothing was changed.")
        }
        document.preparedEdit = prepared
        defer { document.preparedEdit = nil }
        try document.preflightPublication(prepared.document, record: record)
        withUndoGroup(document.undoManager) {
            document.applyEdit(title, record: record) { _ in prepared.document }
        }
        if document.doc === prepared.document, prepared.document.isValidIndex(activeLayer) {
            document.selectLayer(activeLayer)
        }
    }

    /// The recorded step: `set_color_table` with the table's colours and transparent index.
    func step() -> ActionStep {
        ActionStep(tool: "set_color_table",
                   arguments: ["colors": table.colors.map { $0.map { Int($0) } },
                               "transparent_index": table.transparentIndex.map { $0 as Any } ?? NSNull()],
                   note: title)
    }
}
