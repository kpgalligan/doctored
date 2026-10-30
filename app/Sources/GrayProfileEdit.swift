import AppKit

// Assign Profile, Convert to Profile and 8/16/32 Bits/Channel on a Grayscale document (#547 phase
// 5a): the captured edit and its ONE undo step, `CmykProfileEdit`'s twin. The core decides
// everything (`gray_profile`): Assign keeps every sample, Convert is the RGB conversion between the
// two profiles' twins with R = G = B kept (on the 8-bit grid at 8 bits; at 32 bits linear gray is
// the same under every Gray profile), and Depth rounds to the 8-bit grid, widens the declared depth
// or promotes to 32 bits. 32 → 8/16 is HDR Toning's (`HdrToning.swift`). The
// sheet is `GrayProfileSheetController`, the menu items `EditorViewController+GrayMode.swift` and
// `EditorViewController+CmykProfile.swift`, the MCP routes `AgentServer+GrayColour.swift`; the
// recorded steps replay as `assign_profile`, `convert_profile` and `convert_precision`. Since phase
// 6a-i Assign and Convert re-render each type, shape and fill layer on the edited document from
// its authored colours (`artworkCaptures` → `reinkingArtwork`, `CmykProfileEdit`'s pattern; RGB's
// `artworkProfile` rule), and Bits/Channel keeps the descriptions with the layers.

/// One captured Gray profile edit: the source revision, the edit, the destination profile and the
/// options, planned by the core before anything is computed.
struct GrayProfileEdit {
    enum Kind: Equatable {
        case assign, convert
        /// The target depth, 8, 16 or 32.
        case depth(Int)
    }

    let source: RasterDocument
    let revision: UInt64
    let editingState: EditingState
    let kind: Kind
    /// The destination Gray ICC's bytes (an installed or file profile); nil takes `space`.
    let profile: Data?
    /// The destination Gray working space's wire name; nil with `profile`, and for Depth.
    let space: String?
    /// The destination as the MCP tool names it (`profile`/`name`/`path`), merged into the
    /// recorded step so it replays to the profile that was applied.
    let destination: [String: Any]
    let options: CmykConversionOptions
    let activeLayer: Int
    /// The core's plan for the edit.
    let scratch: UInt64
    /// False when the core would change nothing: the profile the document already holds, or the
    /// depth it already declares.
    let changes: Bool
    /// The type, shape and fill layers an Assign or Convert re-renders (phase 6a-i).
    let captures: [ArtworkCapture]

    init(_ document: ImageDocument, kind: Kind, profile: Data?, space: String?, destination: [String: Any],
         options: CmykConversionOptions) throws {
        guard let source = document.doc, !document.isLiveEditing, !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Finish the current operation before changing the profile.")
        }
        guard let typed = source.precision, source.isGray else {
            throw DoctoredCoreError(message: "\(Self.title(kind)) of a Gray profile requires a Grayscale document.")
        }
        guard let state = document.captureEditingState() else {
            throw DoctoredCoreError(message: "Could not capture the document state.")
        }
        self.source = source
        revision = document.revision
        editingState = state
        self.kind = kind
        self.profile = profile
        self.space = space
        self.destination = destination
        self.options = options
        activeLayer = document.activeLayerIndex
        let plan = try typed.grayProfilePlan(profile: profile,
                                             request: Self.request(kind, space: space, options: options))
        if let refusal = plan["refusal"] as? String { throw DoctoredCoreError(message: refusal) }
        scratch = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
        changes = plan["changes"] as? Bool ?? false
        captures = (kind == .assign || kind == .convert) && changes ? try source.artworkCaptures() : []
    }

    /// The menu and undo title of an edit.
    static func title(_ kind: Kind) -> String {
        switch kind {
        case .assign: return "Assign Profile"
        case .convert: return "Convert to Profile"
        case .depth(let bits): return "\(bits) Bits/Channel"
        }
    }

    /// The core request (`dr_precision_doc_gray_profile_edit`); Assign and Convert with neither a
    /// profile nor a space take Color Settings' working Gray.
    static func request(_ kind: Kind, space: String?, options: CmykConversionOptions) -> [String: Any] {
        switch kind {
        case .assign:
            return ["op": "assign", "space": space ?? ColorSettings.workingGray.rawValue]
        case .convert:
            return ["op": "convert", "space": space ?? ColorSettings.workingGray.rawValue,
                    "intent": options.intent.rawValue,
                    "black_point_compensation": options.blackPointCompensation]
        case .depth(let bits):
            return ["op": "depth", "depth": bits]
        }
    }

    /// The edit, on the bounded worker with its own reservation, then the display preparation.
    func compute() throws -> PreparedDocumentEdit {
        guard let typed = source.precision else {
            throw DoctoredCoreError(message: "The document is not a Grayscale document.")
        }
        let kind = kind, profile = profile, request = Self.request(kind, space: space, options: options)
        let pixels: RasterDocument = try PrecisionCapturedWork.run(
            scratchEstimate: scratch, title: Self.title(kind)) { control in
            let context = try PrecisionWorkContext(source: typed)
            guard let result = try typed.grayProfileEdit(profile: profile, request: request,
                                                         workingLimit: context.limit,
                                                         otherRetained: context.otherRetained,
                                                         control: control) else {
                throw DoctoredCoreError(message: "\(Self.title(kind)) changed nothing.")
            }
            return try RasterDocument(precision: result)
        }
        // The captured type, shape and fill layers, re-rendered on the edited document (main
        // thread).
        let edited = captures.isEmpty ? pixels
            : try pixels.reinkingArtwork(captures, from: .srgb, options: options)
        let task = try WorkerTask { PreparedDocumentEdit(edited) }
        let prepared = try BackgroundWork.wait(task, title: "Preparing Grayscale Display")
        guard prepared.preview != nil, prepared.error == nil else {
            throw DoctoredCoreError(message: prepared.error ?? "Could not prepare the Grayscale display.")
        }
        return prepared
    }

    /// ONE undo step.
    func publish(_ prepared: PreparedDocumentEdit, to document: ImageDocument, record: [ActionStep]) throws {
        guard document.doc === source, document.revision == revision, !document.isLiveEditing,
              document.isCurrentEditingState(editingState) else {
            throw DoctoredCoreError(message: "The document changed while \(Self.title(kind)) was prepared. "
                + "Nothing was changed.")
        }
        document.preparedEdit = prepared
        defer { document.preparedEdit = nil }
        try document.preflightPublication(prepared.document, record: record)
        withUndoGroup(document.undoManager) {
            document.applyEdit(Self.title(kind), record: record) { _ in prepared.document }
        }
        if document.doc === prepared.document, prepared.document.isValidIndex(activeLayer) {
            document.selectLayer(activeLayer)
        }
    }

    /// The recorded step: `assign_profile`, `convert_profile` or `convert_precision`.
    func step() -> ActionStep {
        switch kind {
        case .assign:
            var arguments: [String: Any] = ["profile": "working_gray"]
            arguments.merge(destination) { _, recorded in recorded }
            return ActionStep(tool: "assign_profile", arguments: arguments, note: "Assign Profile")
        case .convert:
            var arguments: [String: Any] = ["profile": "working_gray", "intent": options.intent.rawValue,
                                            "black_point_compensation": options.blackPointCompensation]
            arguments.merge(destination) { _, recorded in recorded }
            return ActionStep(tool: "convert_profile", arguments: arguments, note: "Convert to Profile")
        case .depth(let bits):
            return ActionStep(tool: "convert_precision", arguments: ["mode": "gray\(bits)"],
                              note: "\(bits) Bits/Channel")
        }
    }
}
