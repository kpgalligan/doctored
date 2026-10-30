import AppKit

// Image ▸ Mode ▸ Duotone… (#1 Duotone phase 1a): the options value (inks, curves, overprints —
// the core's one JSON schema, `duotone_ink`) and the edit as ONE undo step (`IndexedColorTableEdit`'s
// shape). The core decides everything: the description is set, replaced or (nil options, Image ▸
// Mode ▸ Grayscale) dropped over the same gray samples. The dialog is `DuotoneSheetController`,
// the menu items `EditorViewController+DuotoneMode.swift`'s, the MCP twins `convert_mode` with
// `mode: "duotone"` and `get_duotone`/`set_duotone` (`AgentServer+Duotone.swift`).

/// One ink: a name, a D50 Lab colour and 13 curve slots (output percent per input of
/// `DuotoneOptions.curveInputs`; nil is blank).
struct DuotoneInk: Equatable {
    var name: String
    var lab: [Double]
    var curve: [Double?]

    /// A straight curve from 0 % to 100 %, the eleven slots between blank.
    static let straightCurve: [Double?] = [0] + Array(repeating: nil, count: 11) + [100]
}

/// The Duotone description: 1…4 inks and the authored overprint colours by key (`"1+2"`, …).
struct DuotoneOptions: Equatable {
    var inks: [DuotoneInk]
    /// Authored overprints only; a missing key is the default.
    var overprints: [String: [Double]]

    /// The Duotone Curve dialog's 13 inputs, in percent.
    static let curveInputs = [0, 5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 95, 100]

    /// The Type popup, by ink count.
    static let typeTitles = ["Monotone", "Duotone", "Tritone", "Quadtone"]

    /// First use: Monotone, Ink 1 "Black", Lab (0, 0, 0), curve 0 → 0, 100 → 100.
    static let monotoneBlack = DuotoneOptions(
        inks: [DuotoneInk(name: "Black", lab: [0, 0, 0], curve: DuotoneInk.straightCurve)],
        overprints: [:])

    init(inks: [DuotoneInk], overprints: [String: [Double]]) {
        self.inks = inks
        self.overprints = overprints
    }

    /// Every combination of two or more of `count` inks, by size then lexicographic (the core's
    /// `overprint_subsets`): `1+2`; `1+2, 1+3, 2+3, 1+2+3`; …
    static func overprintKeys(count: Int) -> [String] {
        guard count >= 2 else { return [] }
        var out: [String] = []
        func combinations(_ start: Int, _ size: Int, _ picked: [Int]) {
            if picked.count == size {
                out.append(picked.map { String($0 + 1) }.joined(separator: "+"))
                return
            }
            guard start < count else { return }
            for index in start..<count { combinations(index + 1, size, picked + [index]) }
        }
        for size in 2...count { combinations(0, size, []) }
        return out
    }

    private static func number(_ value: Any?) -> Double? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else { return nil }
        return number.doubleValue
    }

    private static func triple(_ value: Any?) -> [Double]? {
        guard let items = value as? [Any], items.count == 3 else { return nil }
        let values = items.compactMap(number)
        return values.count == 3 ? values : nil
    }

    /// The core's JSON (`{"inks":[{name, lab, curve}], "overprints":{key: [L,a,b] | null}}`; extra
    /// keys such as the normalized reply's `type` are ignored here, the core checks the rest).
    init(json: [String: Any]) throws {
        guard let list = json["inks"] as? [Any] else {
            throw DoctoredCoreError(message: "inks must be a list of 1 to 4 inks, each {name, lab, curve}.")
        }
        inks = try list.enumerated().map { index, entry in
            guard let ink = entry as? [String: Any], let name = ink["name"] as? String,
                  let lab = Self.triple(ink["lab"]), let slots = ink["curve"] as? [Any],
                  slots.count == Self.curveInputs.count else {
                throw DoctoredCoreError(message: "ink \(index + 1) must be {name, lab: [L, a, b], curve: "
                    + "13 numbers or null}.")
            }
            let curve: [Double?] = try slots.map { slot in
                if slot is NSNull { return nil }
                guard let value = Self.number(slot) else {
                    throw DoctoredCoreError(message: "ink \(index + 1)'s curve must be 13 numbers or null.")
                }
                return value
            }
            return DuotoneInk(name: name, lab: lab, curve: curve)
        }
        var authored: [String: [Double]] = [:]
        switch json["overprints"] {
        case nil, is NSNull:
            break
        case let entries as [String: Any]:
            for (key, value) in entries where !(value is NSNull) {
                guard let lab = Self.triple(value) else {
                    throw DoctoredCoreError(message: "overprint \(key) must be [L, a, b] or null.")
                }
                authored[key] = lab
            }
        default:
            throw DoctoredCoreError(message: "overprints must be an object keyed \"1+2\", ….")
        }
        overprints = authored
    }

    /// The core's JSON: the inks and the authored overprints.
    var json: [String: Any] {
        ["inks": inks.map { ink -> [String: Any] in
            ["name": ink.name, "lab": ink.lab,
             "curve": ink.curve.map { $0.map { $0 as Any } ?? NSNull() }]
         },
         "overprints": overprints]
    }

    /// The Type name for the ink count.
    var typeTitle: String {
        Self.typeTitles.indices.contains(inks.count - 1) ? Self.typeTitles[inks.count - 1] : "Duotone"
    }
}

/// One captured Duotone edit: the source revision and the new description, planned by the core
/// before anything is computed. Duotone → Grayscale is `CmykModeConversion`'s (`duotoneToGray`).
struct DuotoneEdit {
    let source: RasterDocument
    let revision: UInt64
    let editingState: EditingState
    let options: DuotoneOptions
    let activeLayer: Int
    let scratch: UInt64
    /// False when the description equals the document's.
    let changes: Bool
    /// The core's refusal (a source that is not an 8- or 16-bit Grayscale document), or nil.
    let refusal: String?

    init(_ document: ImageDocument, options: DuotoneOptions) throws {
        guard let source = document.doc, !document.isLiveEditing, !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Finish the current operation before converting the mode.")
        }
        if let reason = DuotoneDocument.sourceRefusal(source) {
            throw DoctoredCoreError(message: reason)
        }
        guard let typed = source.precision, source.isGray else {
            throw DoctoredCoreError(message: DuotoneDocument.sourceRefusal(source)
                ?? "The document is not a Grayscale document.")
        }
        guard let state = document.captureEditingState() else {
            throw DoctoredCoreError(message: "Could not capture the document state.")
        }
        self.source = source
        revision = document.revision
        editingState = state
        self.options = options
        activeLayer = document.activeLayerIndex
        let plan = try typed.setDuotonePlan(options)
        refusal = plan["refusal"] as? String
        scratch = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
        changes = plan["changes"] as? Bool ?? false
    }

    /// The menu and undo title.
    var title: String { "Duotone" }

    /// The edit on the bounded worker with its own reservation, then the display preparation.
    func compute() throws -> PreparedDocumentEdit {
        if let refusal { throw DoctoredCoreError(message: refusal) }
        guard let typed = source.precision else {
            throw DoctoredCoreError(message: "The document is not a Grayscale document.")
        }
        let options = options, title = title
        let result: RasterDocument = try PrecisionCapturedWork.run(
            scratchEstimate: scratch, title: title) { _ in
            guard let edited = try typed.settingDuotone(options) else {
                throw DoctoredCoreError(message: "\(title) changed nothing.")
            }
            return try RasterDocument(precision: edited)
        }
        let task = try WorkerTask { PreparedDocumentEdit(result) }
        let prepared = try BackgroundWork.wait(task, title: "Preparing Duotone Display")
        guard prepared.preview != nil, prepared.error == nil else {
            throw DoctoredCoreError(message: prepared.error ?? "Could not prepare the Duotone display.")
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

    /// The recorded step: `convert_mode` with `mode: "duotone"` and the options.
    func step() -> ActionStep {
        let json = options.json
        return ActionStep(tool: "convert_mode",
                          arguments: ["mode": "duotone", "inks": json["inks"] ?? [],
                                      "overprints": json["overprints"] ?? [:]],
                          note: "Convert to Duotone")
    }
}
