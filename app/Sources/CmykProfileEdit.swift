import AppKit

// Assign Profile, Convert to Profile and 8/16 Bits/Channel on a CMYK document (#547 phase 5): the
// captured edit and its ONE undo step, `CmykModeConversion`'s twin. The core decides everything
// (`cmyk_profile`): Assign keeps every sample, Convert re-separates each raster through the
// destination's B2A (masks, records and alpha kept; Flatten Image first on request), and Depth
// rounds to the 8-bit grid or widens the declared depth. The sheet is `CmykProfileSheetController`,
// the menu items `EditorViewController+CmykProfile.swift`, the MCP routes
// `AgentServer+CmykColour.swift`; the recorded steps replay as `assign_profile`,
// `convert_profile` and `convert_precision`. The open policy (Edit ▸ Color Settings' CMYK policy)
// runs the same core edit inside the open.
//
// Convert drops a converted raster's type, shape or fill description in the core; the job re-renders
// each captured layer with its inks converted into the new profile (`reinkingArtwork`,
// CmykArtwork.swift, #547) before the display preparation. Convert with Flatten captures nothing;
// Assign and Depth keep the descriptions as they are.

/// One captured CMYK profile edit: the source revision, the edit, the destination profile and
/// the options, planned by the core before anything is computed.
struct CmykProfileEdit {
    enum Kind: Equatable {
        case assign, convert
        /// The target depth, 8 or 16.
        case depth(Int)
    }

    let source: RasterDocument
    let revision: UInt64
    let editingState: EditingState
    let kind: Kind
    /// The destination CMYK profile's bytes; nil is the working CMYK profile (and, for Depth, the
    /// document's own, which the core keeps).
    let profile: Data?
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
    /// The described layers Convert re-renders; empty for Assign, Depth and a flattening Convert.
    let captures: [ArtworkCapture]
    /// The source's colour end (its CMYK profile).
    let sourceEnd: ArtworkColorEnd

    init(_ document: ImageDocument, kind: Kind, profile: Data?, destination: [String: Any],
         options: CmykConversionOptions) throws {
        guard let source = document.doc, !document.isLiveEditing, !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Finish the current operation before changing the profile.")
        }
        guard let typed = source.precision, typed.isCmyk else {
            throw DoctoredCoreError(message: "\(Self.title(kind)) of a CMYK profile requires a CMYK document.")
        }
        guard let state = document.captureEditingState() else {
            throw DoctoredCoreError(message: "Could not capture the document state.")
        }
        self.source = source
        revision = document.revision
        editingState = state
        self.kind = kind
        self.profile = profile
        self.destination = destination
        self.options = options
        activeLayer = document.activeLayerIndex
        let plan = try typed.cmykProfilePlan(profile: profile, request: Self.request(kind, options))
        if let refusal = plan["refusal"] as? String { throw DoctoredCoreError(message: refusal) }
        scratch = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
        switch kind {
        case .depth(let bits):
            changes = (typed.cmykInfo()?["depth"] as? Int) != bits
        case .assign, .convert:
            changes = (profile ?? WorkingCMYK.installedData) != typed.cmykProfileData
        }
        captures = kind == .convert && !options.flatten && changes ? try source.artworkCaptures() : []
        sourceEnd = source.artworkColorEnd
    }

    /// The menu and undo title of an edit.
    static func title(_ kind: Kind) -> String {
        switch kind {
        case .assign: return "Assign Profile"
        case .convert: return "Convert to Profile"
        case .depth(let bits): return "\(bits) Bits/Channel"
        }
    }

    /// The core request (`dr_precision_doc_cmyk_profile_edit`).
    static func request(_ kind: Kind, _ options: CmykConversionOptions) -> [String: Any] {
        switch kind {
        case .assign:
            return ["op": "assign"]
        case .convert:
            return ["op": "convert", "intent": options.intent.rawValue,
                    "black_point_compensation": options.blackPointCompensation, "flatten": options.flatten]
        case .depth(let bits):
            return ["op": "depth", "depth": bits]
        }
    }

    /// The edit, on the bounded worker with its own reservation, then the display preparation.
    func compute() throws -> PreparedDocumentEdit {
        guard let typed = source.precision else {
            throw DoctoredCoreError(message: "The document is not a CMYK document.")
        }
        let kind = kind, profile = profile, request = Self.request(kind, options)
        let pixels: RasterDocument = try PrecisionCapturedWork.run(
            scratchEstimate: scratch, title: Self.title(kind)) { control in
            let context = try PrecisionWorkContext(source: typed)
            guard let result = try typed.cmykProfileEdit(profile: profile, request: request,
                                                         workingLimit: context.limit,
                                                         otherRetained: context.otherRetained,
                                                         control: control) else {
                throw DoctoredCoreError(message: "\(Self.title(kind)) changed nothing.")
            }
            return try RasterDocument(precision: result)
        }
        // The captured type, shape and fill layers, re-inked into the new profile (main thread).
        let edited = captures.isEmpty ? pixels
            : try pixels.reinkingArtwork(captures, from: sourceEnd, options: options)
        let task = try WorkerTask { PreparedDocumentEdit(edited) }
        let prepared = try BackgroundWork.wait(task, title: "Preparing CMYK Display")
        guard prepared.preview != nil, prepared.error == nil else {
            throw DoctoredCoreError(message: prepared.error ?? "Could not prepare the CMYK display.")
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
            var arguments: [String: Any] = ["profile": "working_cmyk"]
            arguments.merge(destination) { _, recorded in recorded }
            return ActionStep(tool: "assign_profile", arguments: arguments, note: "Assign Profile")
        case .convert:
            var arguments: [String: Any] = ["profile": "working_cmyk", "intent": options.intent.rawValue,
                                            "black_point_compensation": options.blackPointCompensation,
                                            "flatten": options.flatten]
            arguments.merge(destination) { _, recorded in recorded }
            return ActionStep(tool: "convert_profile", arguments: arguments, note: "Convert to Profile")
        case .depth(let bits):
            return ActionStep(tool: "convert_precision", arguments: ["mode": "cmyk\(bits)"],
                              note: "\(bits) Bits/Channel")
        }
    }

    /// Edit ▸ Color Settings' CMYK policy at open (#547 phase 5): Preserve Embedded Profiles keeps
    /// the file's profile (today), Off assigns the working CMYK and Convert to Working converts
    /// into it at Color Settings' intent and BPC — the core edit above, run synchronously inside
    /// the open's own job and admitted by the core against the open's limit. Captured on the main
    /// thread. It runs only on a CMYK TIFF, JPEG, PSD or PSB import, which carries pixels and never
    /// a type, shape or fill description; a CMYK `.rz` opens as it was saved, so Convert's dropped
    /// descriptions need no re-ink here.
    struct OpenPolicy {
        let policy: ColorPolicy
        let options: CmykConversionOptions

        static var current: OpenPolicy {
            OpenPolicy(policy: ColorSettings.cmykPolicy, options: CmykConversionOptions())
        }

        /// `document` under the policy; itself when the policy keeps it or the edit changes nothing.
        func applied(to document: RasterPrecisionDocument,
                     context: PrecisionWorkContext) throws -> RasterPrecisionDocument {
            guard document.isCmyk else { return document }
            let kind: Kind
            switch policy {
            case .preserve: return document
            case .off: kind = .assign
            case .convert: kind = .convert
            }
            // The core admits the edit's own plan against the open's limit before it allocates,
            // and refuses (by the plan's sentence) a working profile it cannot convert into.
            return try document.cmykProfileEdit(profile: nil, request: CmykProfileEdit.request(kind, options),
                                                workingLimit: context.limit,
                                                otherRetained: context.otherRetained, control: nil)
                ?? document
        }
    }
}
