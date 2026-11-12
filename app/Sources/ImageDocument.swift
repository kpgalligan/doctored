// The app module is named "Doctored", so the integrator must set
// NSDocumentClass = "Doctored.ImageDocument" on every CFBundleDocumentTypes
// entry in Info.plist.

import AppKit
import UniformTypeIdentifiers

// `isPublishingEditingState` protects read/history restoration from reentrant mutation; deferred
// display waits for that interval and the existing worker to finish.
//
// UI call sites of the throwing facades use `try` inside the `applyEdit` / `applyToSelectedLayers`
// closure and `applyEdit` presents the thrown sentence instead of beeping, as the panel's reorder
// drop does (a nil still beeps).
//
// `ImageDocument.setGroupExpanded` re-points the retained projection at its new document, as
// `applyGuideEdit` does, so opening or closing a group while proofing keeps the proof on the
// retained projection; MCP `set_layer_properties open` goes through its throwing body
// `applyGroupExpanded` at both depths, so it adds no undo step either. The `carriedProjection`
// carry is at 8 bits too: `carriedProjection` has a byte branch
// (`PreparedDocumentEdit(carrying:projection:preview:)`), `applyEdit` asks it at both depths, so
// every existing `renderNeutral: true` call site and its Undo/Redo keep the byte projection instead
// of re-rendering on main, and the 8-bit `set_layer_properties` passes `renderNeutral` when only
// `name` was given.

extension Notification.Name {
    /// Posted by ImageDocument (object: the document) whenever `doc` is
    /// replaced by an edit, live edit, undo, or redo.
    static let imageDocumentImageDidChange = Notification.Name("ImageDocumentImageDidChange")
    static let imageDocumentDidClose = Notification.Name("ImageDocumentDidClose")
}

final class ImageDocument: NSDocument {
    /// The current layered document. Set in read(from:ofType:); documents are
    /// never created blank.
    var doc: RasterDocument! {
        didSet {
            if oldValue !== doc {
                revision &+= 1
                // The samplers are document state; the live list
                // follows every handle change (edit, undo, redo, open).
                syncColorSamplersFromDocument()
                // Edit ▸ Fade's candidate pins two states outside the history
                // budget; any handle change that isn't its result drops it.
                if let candidate = fadeCandidate, candidate.after !== doc { fadeCandidate = nil }
                // The Move float commits on any handle that isn't one of
                // its steps; undo and redo keep it (MoveFloat.swift).
                if let float = moveFloat, !history.navigating, !float.holds(doc) { moveFloat = nil }
            }
        }
    }
    private(set) var revision: UInt64 = 0
    private(set) var hasClosed = false
    /// Counted in `RasterResources`' open-document count.
    private var countsAsOpen = false
    var lastWrittenDocument: RasterDocument?
    var savedDocument: RasterDocument?
    lazy var history = DocumentHistory(document: self)
    lazy var namedSnapshots = NamedSnapshots(document: self)
    var historyWindowController: HistoryWindowController?
    var snapshotsWindowController: SnapshotsWindowController?
    var layerCompsWindowController: LayerCompsWindowController?
    /// The comp the document was last put into, so the Layer Comps window's
    /// ◀ ▶ know where they are stepping from. VIEW bookkeeping: it is not
    /// part of the editing state, never persisted and never undone — a comp
    /// records no pointer back to itself, and the document's per-layer state
    /// can be edited away from a comp without the comp knowing.
    var lastAppliedLayerComp: String?

    /// Window ▸ Sync Pan & Zoom (Phase 12D-2). VIEW bookkeeping on
    /// `lastAppliedLayerComp`'s precedent: it lives here because it is
    /// DOCUMENT-wide rather than per window, but it is not editing state,
    /// never persisted in a `.rz`, never undone and never an Action step.
    var syncPanZoom = false
    /// The last committed Free Transform as this document may repeat it with
    /// Layer ▸ Transform Again (`TransformAgain.swift`): the ONE
    /// application-wide `TransformAgainMemory` when this canvas is the size it
    /// was committed on, else nil. Assigning stores the application's
    /// memory stamped with this canvas's size; nil clears it. Never persisted
    /// and never undone.
    var lastFreeTransform: RepeatableTransform? {
        get {
            guard let doc else { return nil }
            return TransformAgainMemory.repeatable(on: doc.canvasSize)
        }
        set {
            guard let newValue else {
                TransformAgainMemory.last = nil
                return
            }
            guard let doc else { return }
            TransformAgainMemory.last = TransformAgainMemory(
                transform: newValue, canvasSize: doc.canvasSize)
        }
    }
    /// The next view ordinal to mint (`ImageDocument+Views.swift`). Never
    /// re-used inside a session, so a closed view's id does not come back on
    /// a different window.
    var nextViewOrdinal = 1
    /// Set while one view is pushing its zoom and centre at the others. The
    /// push moves their clip views, which post their own bounds changes and
    /// would push straight back.
    var isPropagatingViewSync = false

    override var isDocumentEdited: Bool {
        guard let doc else { return false }
        return doc !== savedDocument
    }

    /// Flattened projection of `doc`, refreshed on every doc change: what the
    /// canvas draws, Copy copies, and flat-format writes encode.
    private(set) var projection: RasterImage?
    /// Authoritative projection in the document's native 16-bit precision.
    /// The display conversion is held only by previewImage.
    private(set) var precisionProjection: RasterPrecisionImage?
    /// Display only: keeps the last successful preview in its original color space.
    /// Failed revisions clear `projection`, so no pixel consumer receives stale data.
    private(set) var previewImage: CGImage?
    private(set) var projectionError: String?
    private weak var projectionDocument: RasterDocument?
    var preparedEdit: PreparedDocumentEdit?
    var precisionProjectionTimer: Timer?
    /// The last undo, redo or History navigation's over-limit warning,
    /// read through `historyMemoryWarning` while its entry is current.
    var historyMemoryWarningStorage: HistoryMemoryWarning?
    /// History/state publication is atomic even if display preparation needs
    /// to service the main run loop. This is the existing Undo system's guard.
    var isPublishingEditingState = false

    /// The panel selection: a PRIMARY entry plus the others a set operation
    /// acts on. Deliberately NOT part of undo in itself — changing it
    /// neither dirties the document nor registers an undo step — but it IS
    /// captured by every undo registration, because undoing a Group Layers
    /// has to put the selection back on the layers that were grouped.
    /// Re-clamped whenever `doc` changes.
    ///
    /// Written through `setLayerSelection` / `selectLayer`
    /// (LayerSelection.swift) or through `activeLayerIndex` below.
    ///
    /// The `didSet` is Phase 12D-2's cross-view mirror, and it hangs on the
    /// PROPERTY rather than on `setLayerSelection` so the `activeLayerIndex`
    /// writes are covered too. Guarded on the value actually changing, so a
    /// re-assignment of the same selection posts nothing.
    private(set) var layerSelection = LayerSelection.single(0) {
        didSet {
            if oldValue != layerSelection {
                // Switching layers commits the Move float; undo and redo keep it.
                if moveFloat != nil, !history.navigating { moveFloat = nil }
                notifyViewTargetsChanged()
            }
        }
    }

    /// The layer that single-layer tools target — the selection's PRIMARY.
    ///
    /// It keeps its name and its exact old meaning so the ~120 places that
    /// ask "which layer am I working on?" need no change at all. Assigning
    /// it COLLAPSES the selection onto that one entry, which is what every
    /// existing write already meant (a click on a row, a new layer becoming
    /// active, `set_active_layer`); a genuine multi-selection is written
    /// with `setLayerSelection`.
    var activeLayerIndex: Int {
        get { layerSelection.primary }
        set { layerSelection = .single(newValue); history.refreshCurrentViewState() }
    }

    /// The editor's paint target, mirrored down so the ONE active-layer edit
    /// path can route a filter or adjustment onto pixels, a mask, a color
    /// plane or a saved channel (OperationScope.swift). UI state: never
    /// persisted, never undoable, written only by
    /// EditorViewController.setPaintTarget.
    var planeEditTarget: PaintTarget = .layer {
        didSet {
            history.refreshCurrentViewState()
            if oldValue != planeEditTarget { notifyViewTargetsChanged() }
        }
    }
    /// The path the Pen edits and the Paths panel shows selected, or
    /// nil for none. UI state like the paint target above: never persisted,
    /// never undoable. A target whose path is gone resolves to nothing
    /// (`resolvedPath`), so an undo that removes the path needs no reset here.
    var currentPath: PathTarget?
    var channelVisibility = ChannelVisibility() {
        didSet {
            history.refreshCurrentViewState()
            if oldValue != channelVisibility { notifyViewTargetsChanged() }
        }
    }
    /// The Color Samplers, the LIVE list the canvas, the Info panel and MCP
    /// read: it mirrors `doc.colorSamplers` (ColorSamplers.swift) and
    /// differs only while a marker drag is in flight. Written by
    /// `commitColorSamplers` and `syncColorSamplersFromDocument`, and by a
    /// drag tick.
    var colorSamplers = ColorSamplerSet() {
        didSet {
            if oldValue != colorSamplers {
                NotificationCenter.default.post(name: .imageDocumentColorSamplersDidChange, object: self)
            }
        }
    }

    /// Snapshot taken by beginLiveEdit, consumed by endLiveEdit.
    private var liveEditBase: RasterDocument?
    /// What the edit about to commit tells Edit ▸ Fade when its record cannot:
    /// an agent call (`AgentServer.execute`), a job (`DocumentJobs.finish`)
    /// or a stroke (set before `endLiveEdit`). Consumed and cleared by the
    /// next `applyEdit` or `endLiveEdit` (ImageDocument+Fade.swift).
    var fadeHint: FadeHint?
    /// The last edit, when Edit ▸ Fade can blend it back; whether it still
    /// can is `currentFadeCandidate`.
    var fadeCandidate: FadeCandidate?
    /// The Move tool's persistent float, while consecutive Moves can
    /// continue it (MoveFloat.swift). Session state: never persisted or undone.
    var moveFloat: MoveFloat?
    private var liveEditingState: EditingState?
    var selectionHistory = SelectionHistoryState()

    /// Quality used for JPEG writes (Save/Save As and the last export choice).
    var jpegExportQuality: Int = 90

    /// Embed the document's ICC profile on export — Save/Save As, Export
    /// and the agent's save_copy alike. Per-document and not persisted, the
    /// same shape as `jpegExportQuality`; a format that cannot carry a
    /// profile ignores it (the save reports what was actually written).
    var embedColorProfile = true

    /// Drop EXIF / XMP / IPTC on export. Off — the default — re-splices the
    /// packets the file arrived with, EXIF orientation reset to 1: the
    /// camera rotation was baked into the pixels at open, so a preserved 6
    /// would double-rotate the picture in every viewer.
    var stripMetadata = false
    var exportBitDepth = 16
    var exportDither = "none"

    /// What the OPEN did to this document's colour: nothing, a conversion
    /// into the working space, or "kept, but not convertible from". It
    /// describes THE OPEN and is deliberately never updated by a later
    /// Assign or Convert — the document's profile name is the truth about
    /// the document, this is the truth about how it got here. A `.rz` never
    /// adopts, so it always reads `.unchanged`.
    private(set) var profileAdoption: RasterAdoptOutcome = .unchanged

    /// Why the OPEN converted a platform-decoded or pasted image to sRGB as
    /// it was read — its embedded profile is one a document cannot carry
    /// (`Bitmap.ingestRefusal`), or it is a Gray, CMYK or Lab image the
    /// platform converted (`Bitmap.platformConversion`, worded by the core's
    /// `dr_platform_conversion_sentence`) — or, on the
    /// core's flat open, converted from or ignored such a profile (the import report's
    /// `profile_refusal`), or, on the typed 16-bit open, converted a
    /// Gray-profiled file (the same sentence, found in its
    /// `import_warnings`) — or nil.
    /// Like `profileAdoption` it
    /// describes the open and is never updated by a later Assign or Convert;
    /// the same sentence is in `metadataImportReport["warnings"]`.
    private(set) var openProfileRefusal: String?

    /// Source container was not inspected for capture metadata. Partial
    /// capture has a separate report so it cannot be confused with absence.
    /// The pre-edit PSD compatibility report and the user's explicit choice.
    private(set) var psdImport: PSDImportState?

    private(set) var metadataNotCaptured = false
    private(set) var metadataImportReport: [String: Any]?

    /// The develop settings this document's camera RAW was opened with, or
    /// nil when its source was not a RAW.
    ///
    /// It exists for Revert, which re-enters `read(from:ofType:)`: Revert
    /// means "the file as I had it", so a RAW re-develops identically and
    /// shows no dialog. Sparse (`RawDevelopSettings`), so what is remembered
    /// is "exposure +1", never one decoder version's idea of neutral.
    private(set) var rawDevelop: RawDevelopSettings?

    /// The rendition an HDR photo was opened with (v1.x): `.hdr` for
    /// a float open, `.sdr` for a gain map's SDR base; nil for every other
    /// file. Revert reopens the same rendition whatever the preference says
    /// now. Never saved.
    private(set) var hdrRendition: HDRPhotoRendition?

    /// The native layered format (registered in Info.plist as .doctored).
    static let nativeTypeIdentifier = "com.kgalligan.doctored-document"

    /// The native format's type before the rename (`.rz`, an imported
    /// type): read and edited, never written. Save on one runs Save As,
    /// because `writableTypes(for:)` never lists it.
    static let legacyNativeTypeIdentifier = "com.kgalligan.rasterize-document"

    /// The extension every native save writes.
    static let nativeFileExtension = "doctored"

    /// True for the native type under either identifier: the read side
    /// (opening, adopting, ordering Save types). Writing compares exactly.
    static func isNativeDocumentType(_ typeName: String?) -> Bool {
        typeName == nativeTypeIdentifier || typeName == legacyNativeTypeIdentifier
    }

    /// The MCP `format` token a native save answers with.
    static let nativeFormatName = "doctored"

    /// True for `doctored` and its legacy alias `rz`, in any case.
    static func isNativeFormatName(_ name: String) -> Bool {
        let lowered = name.lowercased()
        return lowered == nativeFormatName || lowered == "rz"
    }

    private static let readableTypeIdentifiers: [String] = [
        nativeTypeIdentifier,
        legacyNativeTypeIdentifier,
        "public.png",
        "public.jpeg",
        "com.adobe.photoshop-image",
        "com.adobe.photoshop-large-image",
        "public.tiff",
        "com.microsoft.bmp",
        "com.compuserve.gif",
        "org.webmproject.webp",
        // Decoded by the PLATFORM, not the core: HEIC/HEIF stills (the half
        // of a Live Photo the camera writes), and the QuickTime clip that is
        // its other half — opening either one opens the Live Photo.
        "public.heic",
        "public.heif",
        "public.jpeg-xl",
        "com.apple.quicktime-movie",
        "public.mpeg-4",
        // Camera RAW, also platform-decoded (Core Image's CIRAWFilter), and
        // ONE umbrella type: CR3, CR2, NEF, ARW, DNG (Apple ProRAW
        // included), ORF, RW2, RAF, SRW and PEF all conform to
        // public.camera-raw-image, while public.tiff does not — so a plain
        // TIFF keeps its own core decode path. This array also drives
        // FileDropView by conformance, so drag-and-drop needs no edit.
        "public.camera-raw-image",
        // Phase 11D: two more platform-decoded still containers through the
        // 8F host-source seam, and the two VECTOR containers, which are
        // rasterized on the way in (`VectorImport`).
        "public.avif",
        "public.jpeg-2000",
        "com.adobe.pdf",
        "public.svg-image",
        // The two float interchange containers, decoded by the core
        // into a 32-bit float document (`PrecisionImportRequest.floatContainer`).
        // Never a Save type: the export workspace writes them.
        "com.ilm.openexr-image",
        "public.radiance",
    ]

    private static let writableTypeIdentifiers: [String] = [
        "public.png",
        "public.jpeg",
        "public.tiff",
        "com.microsoft.bmp",
        "com.compuserve.gif",
        "org.webmproject.webp",
    ]

    /// The flat types a plain Save (`.saveOperation`) offers in place: every
    /// writable type but GIF, which only Save As and Save a Copy write.
    /// The one home of that rule (`writableTypes(for:)`).
    static let inPlaceSaveTypeIdentifiers: [String] =
        writableTypeIdentifiers.filter { $0 != "com.compuserve.gif" }

    override init() {
        super.init()
        let manager = HistoryUndoManager()
        manager.document = self
        manager.levelsOfUndo = 0 // Memory retention, not a fixed revision count.
        undoManager = manager
    }

    override class var autosavesInPlace: Bool { false }
    override class var autosavesDrafts: Bool { false }

    /// Creates an untitled, dirty document around in-memory pixels
    /// (File > New from Clipboard). Saving prompts for a location.
    ///
    /// `profile` is the ICC bytes those pixel NUMBERS belong to, and this
    /// path follows the same colour rule as every other document-creating
    /// one (see `openDocument`): assign the profile the numbers came in,
    /// then adopt the working space exactly once. Without it a pasted
    /// Display P3 screenshot — the default on every current Mac — became a
    /// document holding P3 numbers labelled sRGB: over-saturated on screen,
    /// and every export propagated the wrong tag.
    ///
    /// `profileRefusal` is the ingest's sentence when the pixels' own
    /// profile could not be carried and they were read into sRGB instead,
    /// or the platform's conversion of a Gray, CMYK or Lab paste
    /// (`RasterImage.fromPasteboard`); it becomes `openProfileRefusal` and
    /// the document's import-report warning. nil when the core refuses
    /// `profile` — its numbers are never kept under another label.
    static func makeUntitled(
        with image: RasterImage, profile: Data? = nil, profileRefusal: String? = nil
    ) -> ImageDocument? {
        guard var doc = RasterDocument.from(image: image) else { return nil }
        // A refusal fails the document rather than leaving `profile`'s
        // numbers labelled sRGB; nil means the profile is already the one.
        if let profile = profile {
            do { doc = try doc.assigningProfile(profile) ?? doc } catch { return nil }
        }
        let document = ImageDocument()
        document.doc = doc
        document.openProfileRefusal = profileRefusal
        document.metadataImportReport = profileRefusal.map { ["warnings": [$0]] }
        document.adoptWorkingSpace()
        document.activeLayerIndex = 0
        document.refreshProjection()
        document.fileType = "public.png"
        document.countEditChange(.changeDone)
        return document
    }

    override class var readableTypes: [String] { readableTypeIdentifiers }

    override func writableTypes(for saveOperation: NSDocument.SaveOperationType) -> [String] {
        if saveOperation == .saveToOperation {
            // #1 phase 5: a CMYK document's Save a Copy writes what a CMYK
            // save can (`CapturedSave`): .doctored, JPEG and TIFF.
            if doc?.isCmyk == true {
                return [Self.nativeTypeIdentifier, UTType.jpeg.identifier, UTType.tiff.identifier]
            }
            // #1 Lab phase 1b: a Lab Color document's Save a Copy writes .doctored or a Lab TIFF.
            if doc?.isLab == true {
                return [Self.nativeTypeIdentifier, UTType.tiff.identifier]
            }
            // #1 Bitmap phase 1c: a Bitmap document's Save a Copy keeps its 1-bit plane in PNG, TIFF
            // and BMP (`BitmapDocument.keptFormats`) and writes JPEG through its Gray render.
            if doc?.isBitmap == true {
                return [Self.nativeTypeIdentifier]
                    + (BitmapDocument.keptFormats + [.jpeg]).map(\.utType.identifier)
            }
            // #1 Indexed phase 1b: an Indexed Color document's Save a Copy keeps its palette in
            // PNG, GIF and TIFF (`IndexedDocument.keptFormats`).
            if doc?.isIndexed == true {
                return [Self.nativeTypeIdentifier] + IndexedDocument.keptFormats.map(\.utType.identifier)
            }
            return [Self.nativeTypeIdentifier] + Self.writableTypeIdentifiers
        }
        let preserving = Self.writableTypeIdentifiers.filter { canPreserveState(in: $0) }
        let flatTypes = saveOperation == .saveOperation
            ? preserving.filter { Self.inPlaceSaveTypeIdentifiers.contains($0) } : preserving
        // Native leads when the current format loses state, or when native
        // is already the master. Save As cannot establish a lossy saved state.
        if let fileType = fileType,
           Self.isNativeDocumentType(fileType) || !canPreserveState(in: fileType) {
            return [Self.nativeTypeIdentifier] + flatTypes
        }
        return flatTypes + [Self.nativeTypeIdentifier]
    }

    override func defaultDraftName() -> String { "Image" }

    override func fileNameExtension(
        forType typeName: String, saveOperation: NSDocument.SaveOperationType
    ) -> String? {
        if typeName == Self.nativeTypeIdentifier {
            return Self.nativeFileExtension
        }
        if let format = ExportFormat.from(fileType: typeName) {
            return format.fileExtension
        }
        return super.fileNameExtension(forType: typeName, saveOperation: saveOperation)
    }

    // MARK: - Reading and writing

    override func read(from url: URL, ofType typeName: String) throws {
        let wasPublishing = isPublishingEditingState
        isPublishingEditingState = true
        defer { isPublishingEditingState = wasPublishing }
        let opened = try Self.openDocument(at: url)
        // Native and explicit PSD imports retain their own profile and
        // source codes, including a PSD's single-layer/composite choice.
        let adopts = opened.psdImport == nil && !Self.isNativeDocumentType(typeName)
            && !Self.hasNativeSignature(at: url) && opened.doc.precision == nil
        // An open whose own charge omitted the first projection admits it
        // here, before anything is installed.
        if opened.projectionUncharged && opened.doc.precision == nil {
            _ = try RasterResources.shared.admit(
                scratch: FlatOpenAdmission.firstProjectionBytes(opened.doc, adopts: adopts),
                own: opened.doc.ownRetainedBytes())
        }
        // A native file's smart-object, placed and Live Photo layers open as
        // ordinary rasters (`RetainedImageWork.plainRasters`); only a camera
        // RAW layer keeps its source. Part of the open: no undo step.
        let native = Self.isNativeDocumentType(typeName) || Self.hasNativeSignature(at: url)
        doc = native ? RetainedImageWork.plainRasters(opened.doc) : opened.doc
        selectionHistory = SelectionHistoryState()
        channelVisibility = ChannelVisibility()
        metadataNotCaptured = opened.metadataNotCaptured
        metadataImportReport = opened.metadataReport
        rawDevelop = opened.rawDevelop
        hdrRendition = opened.hdrRendition
        psdImport = opened.psdImport
        profileAdoption = .unchanged
        openProfileRefusal = opened.profileRefusal
        if adopts {
            adoptWorkingSpace()
        }
        savedDocument = doc
        lastWrittenDocument = nil
        activeLayerIndex = max(doc.layerCount - 1, 0) // topmost
        planeEditTarget = .layer
        refreshProjection()
        undoManager?.removeAllActions()
        history.reset(captureEditingState())
    }

    /// Brings a freshly opened flat document into the colour working space,
    /// ONCE — the single conversion any opened file gets, whichever decoder
    /// produced its pixels. It runs before the document is shown, so it is
    /// part of the open and never an undo step.
    ///
    /// Internal, not private: `makeUntitled` is the fourth document-creating
    /// path and takes the same one conversion.
    ///
    /// Edit ▸ Color Settings' RGB policy decides it (#1 phase 5): Convert to Working (the
    /// default) is the conversion above, Preserve Embedded Profiles keeps the file's profile, and
    /// Off assigns the working space to the numbers as they are.
    func adoptWorkingSpace() {
        guard let current = doc, current.precision == nil else { return }
        switch ColorSettings.rgbPolicy {
        case .convert:
            let (adopted, outcome) = current.adoptingWorkingSpace(
                ColorSettings.workingSpace.profileData)
            profileAdoption = outcome
            if let adopted = adopted { doc = adopted }
        case .preserve:
            profileAdoption = .unchanged
        case .off:
            profileAdoption = .unchanged
            if let assigned = try? current.assigningProfile(ColorSettings.workingSpace.profileData) {
                doc = assigned
            }
        }
    }

    /// The app's open path, most specific first:
    ///
    /// 1. A host still or movie the platform decodes at full precision (HEIC,
    ///    JPEG XL, AVIF, JPEG 2000, H.264, SDR HEVC, ProRes 4444, either half
    ///    of a Live Photo pair) becomes one raster layer of its decoded samples
    ///    (`openHostSource`); an HLG or PQ HEVC movie's frame a 32-bit float
    ///    document (`openHDRMovieFrame`).
    /// 2. A CAMERA RAW — tested by TYPE, never by a failed decode — is
    ///    developed by Core Image, with the Develop dialog choosing the
    ///    settings first unless `RawImportRequest.mode` says otherwise
    ///    (`RawImage.isRawFile` says why the type is the test and the
    ///    failure is not).
    /// 3. The core's own `dr_doc_open`, which transparently handles `.rz`,
    ///    layered PSD, and the flat formats (single "Background" layer; a
    ///    multipage TIFF one hidden layer per later page).
    /// 4. Formats the core has no decoder for (HEIC, HEIF) fall back to the
    ///    platform's, as one flat layer.
    /// 5. COLOUR, on every one of those paths: the document is assigned the
    ///    profile its pixel NUMBERS actually belong to — the file's embedded
    ///    ICC bytes, the space the platform decoded into, or the built-in
    ///    sRGB for an untagged file — and nothing is converted here. The
    ///    exception is a platform decode whose embedded profile a document
    ///    cannot carry: it is read into sRGB, labelled sRGB and reported
    ///    (`Opened.profileRefusal`), so the label still matches the numbers.
    ///    The core's flat open does the same for a Gray-profiled grayscale
    ///    file and a CMYK-profiled CMYK TIFF or JPEG: converted
    ///    to sRGB as they are read, and every profile it does not keep is
    ///    named by the import report's `profile_refusal`, which becomes the
    ///    refusal. An UNTAGGED CMYK TIFF or JPEG is converted the same way
    ///    from the working CMYK profile (`WorkingCMYK`) and reported
    ///    naming it. A CMYK file the core's gate refuses (planar, extra or
    ///    alpha samples, past its size limits) falls through to the
    ///    platform decode, which converts it to sRGB and reports that — as
    ///    every platform decode of a Gray, CMYK or Lab image does — with the
    ///    core's own converted sentence when the core would have converted
    ///    from that profile too, else a plain one (`decoded.refusal`).
    ///    The typed 16-bit open's Gray conversion reports the same way.
    ///    `read(from:ofType:)` then adopts the working space for ordinary
    ///    byte images. Native and explicit PSD imports retain their profile.
    ///    The CLIPBOARD is the fourth document-creating path and follows the
    ///    same two steps in `makeUntitled(with:profile:)`; pasting into an
    ///    open document instead converts the pixels into that document's
    ///    space, since a layer has no profile of its own.
    ///
    /// 6. METADATA uses the core's JPEG/PNG/TIFF scanner or ImageIO's
    ///    standard capture fields for RAW/HEIC. The import report distinguishes
    ///    supported packets, general limits and verified omissions. Live Photo
    ///    video frames and other unwalked containers retain an explicit notice.
    ///
    /// A file that none of them can read reports the CORE's error, which
    /// names the file and the reason — except a RAW that Core Image accepted
    /// and then failed to develop, which reports its own reason and never
    /// falls back to a decode that would import a CFA mosaic.
    static func openDocument(at url: URL) throws -> Opened {
        // Genuine camera RAW precedes ordinary high-depth TIFF intake. A
        // mis-extensioned PNG/TIFF that has no RAW decoder still receives the
        // strict preserving path; a filename alone never authorizes byte loss.
        if !hasNativeSignature(at: url), RawImage.isRawFile(url), let probe = RawImage.inspect(url: url) {
            return try openRaw(probe)
        }
        // v1.x: an HDR photo opens as 32-bit float unless the SDR
        // rendition is asked for (`HDRPhotoRequest`). A gain map's SDR base
        // is today's host open below, disclosed; a PQ/HLG still has no SDR
        // rendition and refuses by name. Either half of a Live Photo pair
        // keeps the pair's own open (`HostSourceInput`).
        if !hasNativeSignature(at: url), !routingSDRBase, LivePhoto.locate(url) == nil,
           let kind = HDRStillDecoder.classify(url) {
            if HDRPhotoRequest.effective == .hdr, PrecisionImportRequest.mode != .rgba8 {
                return try openHDRStill(url, kind: kind)
            }
            guard kind == .gainMap else {
                throw HostSourceError.failure(HDRPhotoRequest.noSDRRendition(kind))
            }
            // The rest of this function, exactly as before v1.x.
            routingSDRBase = true
            defer { routingSDRBase = false }
            var opened = try openDocument(at: url)
            opened.hdrRendition = .sdr
            var report = opened.metadataReport ?? [:]
            report["hdr_open"] = HDRPhotoRequest.openRecord(kind, rendition: .sdr)
            report["warnings"] = ((report["warnings"] as? [String]) ?? [])
                + [HDRPhotoRequest.sdrBaseDisclosure]
            opened.metadataReport = report
            return opened
        }
        if !hasNativeSignature(at: url), HostSourceInput.claims(url) {
            return try openHostSource(url)
        }
        if !hasNativeSignature(at: url), VectorImport.claims(url) { return try openVector(url) }
        // A TIFF carrying Photoshop's layer block opens as its layers ONLY
        // when Import Layers is chosen (`openLayeredTIFF` answers nil for
        // every other route, which falls through to the unchanged code
        // below). It is asked here, before the high-depth intake, because
        // that intake otherwise takes every 16-bit TIFF.
        if isLayeredTIFF(at: url), let imported = try openLayeredTIFF(at: url) {
            return Opened(doc: imported.doc, metadataNotCaptured: true, rawDevelop: nil,
                          psdImport: imported.state,
                          projectionUncharged: imported.doc.precision == nil)
        }
        if let preserving = try openPrecisionDocumentIfRequested(at: url) {
            return Opened(doc: preserving, metadataNotCaptured: false, rawDevelop: nil,
                          metadataReport: preserving.precisionInfo.map { ["warnings": $0["import_warnings"] ?? []] },
                          profileRefusal: preserving.precisionProfileRefusal(openedFrom: url),
                          projectionUncharged: preserving.precision == nil)
        }
        // PSD never reaches the generic ImageIO fallback: unsupported color
        // or depth and incompatible layers require an explicit import choice.
        if isPSD(at: url) {
            let imported = try openPSD(at: url)
            return Opened(doc: imported.doc, metadataNotCaptured: true, rawDevelop: nil,
                          psdImport: imported.state,
                          projectionUncharged: imported.doc.precision == nil)
        }
        // The core's flat JPEG/PNG open is admitted whole before it decodes.
        let flatCharge = try FlatOpenAdmission.charge(for: url)
        if let charge = flatCharge {
            _ = try RasterResources.shared.admit(scratch: charge)
        }
        // Ordinary image imports retain the established auxiliary-matte pass.
        do {
            let opened = try RasterDocument.open(url: url)
            let report = RasterDocument.metadataImportReport(at: url)
            return Opened(
                doc: AuxiliaryMattes.attaching(to: opened, from: url) ?? opened,
                metadataNotCaptured: !RasterDocument.metadataWalked(at: url),
                rawDevelop: nil,
                metadataReport: report,
                profileRefusal: report?["profile_refusal"] as? String,
                projectionUncharged: flatCharge == nil)
        } catch {
            // A typed/unsupported native file must keep the core's refusal;
            // ImageIO's byte fallback is only for non-native image containers.
            if Self.hasNativeSignature(at: url) { throw error }
            if Self.hostFallbackWouldLosePrecision(url) {
                throw HostSourceError.failure("This platform image format has no qualified preserving import path. An 8-bit image was not substituted.")
            }
            guard let decoded = RasterImage.decoded(from: url),
                  let built = RasterDocument.from(image: decoded.image)
            else { throw error }
            var doc = built
            // The platform decoded into the source's OWN space, so these are
            // that profile's numbers; label them and let the adoption convert.
            // `ingestSpace` already read a profile no document can carry into
            // sRGB (`decoded.refusal`), so a refusal here fails the open with
            // the core's sentence rather than mislabelling the numbers.
            if let profile = decoded.profile, let tagged = try doc.assigningProfile(profile) {
                doc = tagged
            }
            if let dpi = decoded.dpi, let sized = try doc.settingResolution(x: dpi.0, y: dpi.1) {
                doc = sized
            }
            let metadata = ImageIOMetadata.read(url)
            let captured = metadata.attaching(to: doc)
            doc = captured.document
            var report = captured.report
            if let refusal = decoded.refusal {
                report["warnings"] = ((report["warnings"] as? [String]) ?? []) + [refusal]
            }
            // The platform decodes one image: a multipage TIFF the core could
            // not open says which pages are missing and why (`tiff_pages`).
            if let pages = RasterDocument.metadataImportReport(at: url)?["tiff_pages"] as? Int,
               pages > 1 {
                report["warnings"] = ((report["warnings"] as? [String]) ?? [])
                    + ["Only page 1 of the file's \(pages) pages was opened, by the system "
                        + "decoder, because the file's own open failed: "
                        + error.localizedDescription]
            }
            return Opened(
                doc: AuxiliaryMattes.attaching(to: doc, from: url) ?? doc,
                metadataNotCaptured: !metadata.inspected, rawDevelop: nil,
                metadataReport: report, profileRefusal: decoded.refusal,
                projectionUncharged: true)
        }
    }

    /// True while a gain-map photo's SDR base re-enters `openDocument(at:)`
    /// to take the path every such photo took before v1.x.
    private static var routingSDRBase = false

    /// Capture original camera bytes and dense decoder state, develop directly
    /// to u16 and attach a retained source in the chosen document depth.
    private static func openRaw(_ probe: RawProbe) throws -> Opened {
        try openRetainedRaw(probe)
    }

    /// Open-time diagnostics remain separate from persisted packet bytes.
    struct Opened {
        let doc: RasterDocument
        let metadataNotCaptured: Bool
        /// The settings a camera RAW was developed with, so Revert can
        /// reproduce them; nil on every other path.
        let rawDevelop: RawDevelopSettings?
        var metadataReport: [String: Any]? = nil
        var psdImport: PSDImportState? = nil
        /// `ImageDocument.openProfileRefusal`: set by the ImageIO fallback, by
        /// the core open's import report (`profile_refusal`) and by the typed
        /// open's Gray conversion (`precisionProfileRefusal(openedFrom:)`).
        var profileRefusal: String? = nil
        /// The open charged no first projection: `read(from:ofType:)` admits it before installing anything.
        var projectionUncharged: Bool = false
        /// `.hdr` from `openHDRStill`, `.sdr` from a gain map's SDR-base open;
        /// nil for every other file (`ImageDocument.hdrRendition`).
        var hdrRendition: HDRPhotoRendition? = nil
    }

    override func revert(toContentsOf url: URL, ofType typeName: String) throws {
        // The file as it was opened: a PSD (or a layered TIFF) with the
        // choice it was opened with, and anything else with the flat choice,
        // so a layered TIFF that was opened flat reverts flat with no dialog.
        let previousPSD = PSDImportRequest.mode
        PSDImportRequest.mode = .explicit(psdImport?.choice ?? .composite, fingerprint: nil)
        defer { PSDImportRequest.mode = previousPSD }
        // Revert means "the file as I had it": a RAW re-develops with the
        // exact settings this document was opened with, and shows no dialog.
        // Restored rather than cleared afterwards, because an open can nest
        // (a Live Photo pair, a drop of several files).
        let previous = RawImportRequest.mode
        if let settings = rawDevelop { RawImportRequest.mode = .headless(settings) }
        defer { RawImportRequest.mode = previous }
        // …and an HDR photo reopens in the rendition it was opened with.
        let previousHDR = HDRPhotoRequest.mode
        if let hdrRendition { HDRPhotoRequest.mode = hdrRendition }
        defer { HDRPhotoRequest.mode = previousHDR }
        try super.revert(toContentsOf: url, ofType: typeName)
        // Revert replaces `doc` via read(from:ofType:) without going through
        // applyEdit, so tell the editor UI explicitly.
        NotificationCenter.default.post(
            name: .imageDocumentImageDidChange, object: self, userInfo: ["isLive": false])
    }

    override func write(to url: URL, ofType typeName: String) throws {
        guard let doc = doc else {
            throw DoctoredCoreError(message: "No image loaded.")
        }
        if doc.precision != nil {
            try CapturedSave(self, type: typeName).write(to: url)
            return
        }
        if typeName == Self.nativeTypeIdentifier {
            try doc.saveNative(to: url)
            return
        }
        guard let format = ExportFormat.from(fileType: typeName) else {
            throw DoctoredCoreError(message: "Cannot write files of type \(typeName).")
        }
        // #1: a TIFF keeps the spot channels (1c-2a); any other flat file composites them.
        let flattened = try flatSaveProjection(format: format)
        // The document-level save so the ICC profile, the EXIF/XMP/IPTC
        // packets and the print resolution ride along; the warm projection
        // goes in as the composite so ⌘S never re-flattens the layer stack.
        // The safe-writing entry point has already verified preservation;
        // explicit Export may instead choose to drop state.
        _ = try doc.saveImage(
            flattened, to: url, format: format.drFormat, jpegQuality: jpegExportQuality,
            embedProfile: embedColorProfile, stripMetadata: stripMetadata)
    }

    // MARK: - Window controllers

    override func makeWindowControllers() {
        addWindowController(makeViewWindowController())
        if !countsAsOpen {
            countsAsOpen = true
            RasterResources.shared.noteDocumentOpened()
        }
    }

    /// With two views open, a sheet belongs on the one the user is in.
    /// NSDocument's default answers the FIRST window controller's window,
    /// which would drop an alert on the overview while the work is happening
    /// in the detail view.
    override var windowForSheet: NSWindow? {
        if let key = windowControllers.compactMap(\.window).first(where: { $0.isKeyWindow }) {
            return key
        }
        return super.windowForSheet
    }

    /// A closed view leaves the survivors' titles to fix: “<name>:1” has to
    /// become “<name>” again the moment there is only one of them.
    override func removeWindowController(_ windowController: NSWindowController) {
        super.removeWindowController(windowController)
        for controller in windowControllers {
            controller.synchronizeWindowTitleWithDocumentName()
        }
    }

    // MARK: - Change counting

    /// NSDocument's automatic undo-based change counting is unreliable
    /// here: whether the DidCloseUndoGroup/DidUndo/DidRedo counting fires
    /// depends on how the run loop is being driven (real UI events versus
    /// the agent server's dispatched-to-main tool calls), which double- or
    /// zero-counts edits. So this class counts every edit explicitly at
    /// the applyEdit/restoreDoc/endLiveEdit sites, and this override drops
    /// the automatic done/undone/redone calls. Clearing (save) and other
    /// change types pass through untouched.
    private var allowEditChangeCount = false

    /// Set for the length of `withReplayUndo`'s body: an Action replay is ONE
    /// change for the change count exactly as it is one entry for undo, so
    /// the replayed steps' own `countEditChange(.changeDone)` calls are
    /// dropped here and the run counts once, beside its single registration.
    ///
    /// Suppressed rather than compensated for afterwards: a step that reads
    /// the count while the run is in flight (a `save_copy` step reporting the
    /// document, a window-title update from a step's notification) would
    /// otherwise see a transient value that no undo ever walks back.
    private var suppressEditChangeCount = false

    override func updateChangeCount(_ change: NSDocument.ChangeType) {
        if change == .changeCleared {
            savedDocument = lastWrittenDocument ?? doc
            lastWrittenDocument = nil
            // Save commits the Move float.
            moveFloat = nil
        }
        switch change {
        case .changeDone, .changeUndone, .changeRedone:
            guard allowEditChangeCount, !suppressEditChangeCount else { return }
        default:
            break
        }
        super.updateChangeCount(change)
        // AppKit clears its internal counter at save completion, even when
        // the worker wrote an older revision. Keep its close/save machinery
        // dirty too; saved-handle identity remains authoritative across undo.
        if isDocumentEdited != super.isDocumentEdited {
            super.updateChangeCount(isDocumentEdited ? .changeDone : .changeCleared)
        }
        refreshWindowDirtyIndicators()
        if change == .changeCleared {
            if history.entries.isEmpty { history.reset(captureEditingState()) }
            else {
                history.measure()
                if undoManager?.groupingLevel == 0 { history.enforceBudget() }
                history.notify()
            }
            RecoveryCoordinator.shared.documentDidSave(self, captured: savedDocument)
        }
    }

    private func refreshWindowDirtyIndicators() {
        for controller in windowControllers { controller.window?.isDocumentEdited = isDocumentEdited }
    }

    /// The explicit counting entry used by this class's edit paths.
    func countEditChange(_ change: NSDocument.ChangeType) {
        allowEditChangeCount = true
        updateChangeCount(change)
        allowEditChangeCount = false
    }

    // MARK: - Editing

    /// `applyEdit`'s publication refusals, as one ordered list: the first
    /// sentence that refuses publishing `updated` over the current `doc`, or
    /// nil. In order: a `record` step's retained source (RetainedImagePolicy);
    /// then, only when `updated` is given, the evaluation policy's
    /// linear-light refusal (DocumentEvaluation.swift — an edit that would put
    /// an unmigrated operator into a linear-light document is refused BY NAME,
    /// so the user meets the sentence on the edit rather than on the next
    /// render; nil, without any walk, for every encoded document); then a
    /// persisted warp the edit would drop, unless `allowWarpChange` or
    /// `intentionalDepthConversion`. `applyEdit` asks for the record part
    /// before its transform runs (`updated: nil`) and the rest after;
    /// `preflightPublication` asks for all of it before a caller's undo group opens.
    func publicationRefusal(record: [ActionStep], to updated: RasterDocument?,
                            allowWarpChange: Bool = false,
                            intentionalDepthConversion: Bool = false) -> String? {
        for step in record {
            if let reason = RetainedImagePolicy.refusal(tool: step.tool, arguments: step.arguments, document: self) {
                return reason
            }
        }
        guard let updated else { return nil }
        if let reason = updated.linearLightRefusal() { return reason }
        if !allowWarpChange && !intentionalDepthConversion, let current = doc {
            return PersistedWarpPolicy.publicationRefusal(from: current, to: updated)
        }
        return nil
    }

    /// Applies `transform` to the current document. A nil result beeps and
    /// leaves the document untouched. A THROWN error is the core's refusal
    /// sentence: it is presented (`presentError`, like the retained-image,
    /// linear-light and warp refusals of `publicationRefusal`), nothing is published or
    /// recorded, and there is no beep. Undo restores the exact prior
    /// handle (whole-document snapshots stay cheap: handles are copy-on-write).
    ///
    /// After the transform, the remaining refusals, the depth guard and the
    /// 16-bit display preparation are `admitsPublication`
    /// (ImageDocument+EditPreflight.swift). A caller that wraps this call in
    /// `withUndoGroup` asks `preflightPublication` (or `admitsPublication`)
    /// first, so a refusal never leaves an empty undo step.
    ///
    /// `record` is the ACTION STEP (or steps) this edit is, for the Actions
    /// recorder — `.notACommand` for an internal or agent-originated path,
    /// `.unrecorded("…")` for a user command with no MCP twin. It has no
    /// default value on purpose: a new edit path must state one before it
    /// compiles, which is the enforcement a runtime assert could not give.
    /// It is recorded on the SUCCESS path only, after the handle has been
    /// replaced, so a beeped core refusal records nothing for free.
    func applyEdit(
        _ actionName: String, record: [ActionStep], changesCanvasGeometry: Bool = false,
        intentionalDepthConversion: Bool = false,
        allowWarpChange: Bool = false, renderNeutral: Bool = false,
        resultingSelection: SelectionHistoryState? = nil,
        _ transform: (RasterDocument) throws -> RasterDocument?
    ) {
        defer { fadeHint = nil }
        guard !isLiveEditing, !isPublishingEditingState, let current = doc else {
            SessionCommitRefusal.beep("Finish the current gesture or edit first. Nothing changed.")
            return
        }
        let capturedRevision = revision
        let capturedState = captureEditingState()
        if let reason = publicationRefusal(record: record, to: nil) {
            presentError(DoctoredCoreError(message: reason))
            return
        }
        let transformed: RasterDocument?
        do { transformed = try transform(current) } catch { presentError(error); return }
        guard let updated = transformed, doc === current, revision == capturedRevision,
              !isLiveEditing,
              isCurrentEditingState(capturedState) else {
            SessionCommitRefusal.beep(transformed == nil
                ? "\(actionName) was refused and nothing changed."
                : "The document changed while \(actionName) was running. Nothing changed.")
            return
        }
        guard admitsPublication(updated, record: [], allowWarpChange: allowWarpChange,
                                intentionalDepthConversion: intentionalDepthConversion,
                                renderNeutral: renderNeutral) else { return }
        isPublishingEditingState = true
        defer { isPublishingEditingState = false }
        let before = captureEditingState()
        doc = updated
        if intentionalDepthConversion {
            layerSelection = .single(0)
            planeEditTarget = .layer
        }
        let geometryChanged = changesCanvasGeometry || current.width != updated.width
            || current.height != updated.height
        if geometryChanged {
            clearSelectionForGeometry()
        }
        // Move Selected Pixels: the moved selection lands in the SAME
        // undo step as the pixels.
        if let resultingSelection {
            selectionHistory = resultingSelection
            notifySelectionChanged()
        }
        countEditChange(.changeDone)
        // `current`, not `doc`: the canvas a step's coordinates were written
        // against is the one it STARTED on, and a crop, a rotate or a resize
        // has already replaced the handle by here (ActionRecorder.noteCanvas).
        ActionRecorder.shared.record(
            record, on: self, canvas: (width: current.width, height: current.height))
        docDidChange()
        if let before {
            registerHistoryChange(from: before, actionName: actionName, renderNeutral: renderNeutral,
                                  changesGeometry: geometryChanged)
        }
        noteFadeCandidate(name: actionName, before: current, record: record)
    }

    /// Runs `body` — a one-entry Action replay (`run_action`, Batch), or one
    /// ⌃F repeat — with the steps' OWN undo registrations suppressed, and
    /// registers exactly one entry afterwards, restoring the document to where
    /// it started. The interactive Play is per-step and does not come here
    /// (`ActionRunHistory`).
    ///
    /// **One undo step for the whole action, and none at all when nothing
    /// changed.** The player used to open one explicit group around the run
    /// and name it whatever the steps did, which left a phantom "Undo
    /// <action>" behind every run whose steps all failed or were all
    /// disabled: AppKit pushes an EMPTY group on `endUndoGrouping` (measured)
    /// and there is no API to take one back, so it also consumed one of the
    /// 24 undo levels and pushed a real step off the bottom. Suppressing the
    /// registrations instead means the decision is made after the fact, on
    /// the one question that answers it — did the document handle change? —
    /// and the entry is a snapshot restore, which is byte-for-byte what
    /// undoing every step of the run one at a time would have produced
    /// (`applyEdit`'s own undo restores exactly this pair).
    ///
    /// While registration is disabled every nested `beginUndoGrouping` /
    /// `endUndoGrouping` — `AgentServer.performGroupedEdit`'s, one per step —
    /// is a no-op, so no empty subgroup lands either. The single registration
    /// then goes inside `withUndoGroup`, because off the event path (an MCP
    /// `run_action`) an ungrouped registration would open NSUndoManager's
    /// implicit event group and never close it.
    ///
    /// **The change count is bracketed exactly like the undo registration,
    /// and for the same reason.** Each replayed step reaches `applyEdit`,
    /// which counts a change; the one entry registered here undoes them all
    /// but decrements once, so an N-step run left the document N−1 changes
    /// "edited" with an empty undo stack — permanently prompting to save a
    /// document byte-identical to the file. So the steps' counting is
    /// suppressed for the length of `body()` and the run counts ONE change,
    /// in the same `snapshot !== doc` branch that registers the one undo:
    /// `restoreDoc`'s single `.changeUndone` then balances it exactly, and a
    /// run that changed nothing counts nothing.
    @discardableResult
    func withReplayUndo<T>(_ actionName: String, _ body: () -> T) -> T {
        let manager = undoManager
        let snapshot = captureEditingState()
        history.suspended += 1
        manager?.disableUndoRegistration()
        // Saved and restored rather than set to false, so a replay nested
        // inside another one (none today) counts against the outer run.
        let wasSuppressed = suppressEditChangeCount
        suppressEditChangeCount = true
        let value = body()
        suppressEditChangeCount = wasSuppressed
        manager?.enableUndoRegistration()
        history.suspended -= 1
        guard let snapshot else { return value }
        let pixelsChanged = snapshot.document !== doc
        registerHistoryChange(from: snapshot, actionName: actionName)
        if pixelsChanged { countEditChange(.changeDone) }
        return value
    }

    /// `applyEdit` for an edit that changes the document's GUIDES or its
    /// RULER ORIGIN — and the ONE path for both the UI and the agent.
    ///
    /// It is `applyEdit`'s body with `docDidChange()` replaced by the
    /// notification alone: undo, dirty and notify, WITHOUT `refreshProjection()`.
    /// A guide changes no pixel, so re-flattening a 100 MP document once per
    /// guide edit is pure waste — and a guide drag would pay it on the one
    /// edit it makes, on the main thread, while the pointer is still down.
    /// It still COUNTS a change, for the reason `setGroupExpanded` gives
    /// just below: a field that saves without dirtying loses itself.
    ///
    /// For the same reason the retained projection is RE-POINTED at the
    /// guide-only document (its projection, typed projection and preview are
    /// exactly `current`'s), and the history transition is recorded
    /// `renderNeutral`, so undo and redo carry that display across instead of
    /// preparing a new one behind the modal panel.
    ///
    /// The agent's wrapper (`AgentServer+Guides.editGuides`) adds only the
    /// explicit undo grouping `performGroupedEdit` uses, and must never
    /// route a guide edit through `applyEdit`: that is the reprojecting path
    /// this method exists to avoid, and using it would make a second entry
    /// point for the same edit.
    func applyGuideEdit(
        _ actionName: String, record: [ActionStep],
        _ transform: (RasterDocument) throws -> RasterDocument?
    ) {
        guard !isLiveEditing, let current = doc else {
            NSSound.beep()
            return
        }
        // A named refusal is presented and changes nothing; a plain
        // nil still beeps.
        let result: RasterDocument?
        do {
            result = try transform(current)
        } catch {
            presentError(error)
            return
        }
        guard let updated = result else {
            NSSound.beep()
            return
        }
        let before = captureEditingState()
        doc = updated
        if projectionDocument === current { projectionDocument = updated }
        countEditChange(.changeDone)
        // Same hook, same place, same rule as applyEdit's (see there).
        ActionRecorder.shared.record(
            record, on: self, canvas: (width: current.width, height: current.height))
        NotificationCenter.default.post(
            name: .imageDocumentImageDidChange, object: self, userInfo: ["isLive": false])
        if let before {
            registerHistoryChange(from: before, actionName: actionName, renderNeutral: true)
        }
    }

    /// Replaces the whole selection, clamped to the document. Like the
    /// single-layer `activeLayerIndex` write it retargets future edits only:
    /// no undo step, no dirty flag. The ONE setter, so the "always in range,
    /// always holds its primary" invariant lives in one place.
    func setLayerSelection(_ selection: LayerSelection) {
        layerSelection = selection.clamped(to: doc?.layerCount ?? 0)
        history.refreshCurrentViewState()
    }

    /// Chains one pure per-entry op across the WHOLE selection as a single
    /// undo step — the panel header's blend mode and opacity, the lock
    /// items, visibility.
    ///
    /// For PROPERTY writes only: every index is resolved against the same
    /// stack, so an op that renumbers (group, delete, merge, reorder) must
    /// be ONE core call over the whole set instead — that is why the core
    /// exports `dr_doc_*_layers` rather than leaving the host to loop.
    ///
    /// A nil from `op` means "this entry did not change" — the core's purity
    /// rule, which a multi-selection hits constantly (three layers set to
    /// Multiply when one already is). The edit therefore carries on with the
    /// entries that DID change and fails only when none of them did, which
    /// is exactly when `applyEdit`'s beep is the right answer. A THROWN
    /// refusal from any entry aborts the whole edit, and `applyEdit` presents
    /// its sentence.
    func applyToSelectedLayers(
        _ actionName: String, record: [ActionStep], renderNeutral: Bool = false,
        _ op: @escaping (RasterDocument, Int) throws -> RasterDocument?
    ) {
        let indices = layerSelection.all
        applyEdit(actionName, record: record, renderNeutral: renderNeutral) { doc in
            var updated = doc
            var changed = false
            for idx in indices {
                if let next = try op(updated, idx) {
                    updated = next
                    changed = true
                }
            }
            return changed ? updated : nil
        }
    }

    /// Opens or closes a GROUP's disclosure in the panel.
    ///
    /// A real document mutation, deliberately: `open` is persisted in the
    /// `.rz` layer record (at 16/32 bits the typed `LOPN` table), and a field that saves without dirtying loses
    /// itself — open a document, toggle a disclosure, close it, and there is
    /// no save prompt and the state is gone, while any later edit would
    /// silently save it. So this counts a change like every other edit path.
    ///
    /// It registers NO undo: a disclosure triangle is not part of the
    /// picture, and an undo step for it would be noise the user never asked
    /// for. And it skips `refreshProjection()` — the picture did not change,
    /// only the row set — while still posting the change notification, which
    /// is what reloads the panel's rows.
    ///
    /// CLOSING a group re-points the selection at the rows that are still
    /// visible, through the same `visibleRow(for:)` the panel draws with. What
    /// the panel highlights and what a command acts on must be the same entry:
    /// a selected layer inside a group that then closes has no row of its own,
    /// the panel draws its GROUP's row selected instead, and leaving the
    /// selection on the hidden child meant Delete Layer, Merge Down and the
    /// footer buttons all acted on something other than the one row the panel
    /// said was selected. Remapping removes the mismatch rather than hiding
    /// it. Opening a group needs no remap: nothing stops being visible.
    ///
    /// `applyGroupExpanded` is the throwing body MCP `set_layer_properties
    /// open` shares, so the agent's disclosure is the triangle's: saved,
    /// dirtying, render-neutral and no undo step. It refuses a live gesture or
    /// a publication in flight with `SilentEditRefusal` and rethrows the typed
    /// edit's named refusal; false when the disclosure would change nothing.
    /// Only this UI wrapper presents (a `SilentEditRefusal` beeps).
    func setGroupExpanded(_ idx: Int, _ open: Bool, record: [ActionStep]) {
        guard !isLiveEditing else { return }
        do { try applyGroupExpanded(idx, open, record: record) } catch { presentError(error) }
    }

    @discardableResult
    func applyGroupExpanded(_ idx: Int, _ open: Bool, record: [ActionStep]) throws -> Bool {
        guard !isLiveEditing, !isPublishingEditingState, let current = doc else {
            throw SilentEditRefusal(message: "Finish the current gesture or edit first. Nothing changed.")
        }
        // A named refusal (the typed edit while background work holds the
        // document) is thrown and changes nothing; the UI wrapper presents it,
        // `applyGuideEdit`'s rule.
        guard let updated = try current.withLayerOpen(idx, open) else { return false }
        doc = updated
        // The picture did not change: the retained projection stays the
        // current document's own, so a proof keeps using it.
        if projectionDocument === current { projectionDocument = updated }
        countEditChange(.changeDone)
        // After the mutation, like every other hook: the guard above returns
        // before it, so a disclosure that would change nothing records nothing.
        ActionRecorder.shared.record(
            record, on: self, canvas: (width: current.width, height: current.height))
        setLayerSelection(open ? layerSelection : layerSelection.mappedToVisibleRows(in: updated))
        history.replaceUntrackedCurrentState()
        NotificationCenter.default.post(
            name: .imageDocumentImageDidChange, object: self, userInfo: ["isLive": false])
        return true
    }

    /// Commits the colour samplers as an UNTRACKED document mutation —
    /// `applyGroupExpanded`'s precedent: the list is saved in `.rz` and PSD,
    /// so the edit dirties the document, but it adds no undo step, records no
    /// Action step and does not reproject (the retained projection is
    /// re-pointed: no pixel changed). The UI's add, ⌥-click remove, Clear and
    /// drag end and MCP `set_color_samplers` all land here.
    ///
    /// Refused — `SilentEditRefusal`, nothing changed — during a live
    /// gesture, a publication in flight, or while a job runs on this
    /// document (`DocumentJobs.refusesGesture`), because the new handle
    /// would make that job's result stale. False when the core answers nil:
    /// the list the document already holds (callers validate first, so an
    /// invalid list never reaches here). Either way the live list is resynced
    /// from the handle, which ends a drag that changed nothing.
    @discardableResult
    func commitColorSamplers(_ next: ColorSamplerSet) throws -> Bool {
        guard !isLiveEditing, !isPublishingEditingState, let current = doc,
              !DocumentJobs.refusesGesture(on: self, workerBound: false) else {
            syncColorSamplersFromDocument()
            throw SilentEditRefusal(
                message: "Finish the current gesture, edit or job first. The color samplers did not change.")
        }
        guard let updated = try current.withColorSamplers(next) else {
            syncColorSamplersFromDocument()
            return false
        }
        doc = updated
        if projectionDocument === current { projectionDocument = updated }
        countEditChange(.changeDone)
        history.replaceUntrackedCurrentState()
        return true
    }

    /// Resets the live list to the committed one on `doc` (a no-op when they
    /// already agree), so a refused or unchanged drag snaps back.
    func syncColorSamplersFromDocument() {
        let committed = doc?.colorSamplers ?? ColorSamplerSet()
        if committed != colorSamplers { colorSamplers = committed }
    }

    /// Applies a flat-image operation to the ACTIVE layer's pixels as one
    /// undo step (filters and adjustments). Rewriting the pixels of a text
    /// layer invalidates its description, so this goes through
    /// applyRasterizingEdit.
    ///
    /// Deliberately SINGLE-layer even when several are selected: a filter
    /// fanned out across a selection would be one undo step holding
    /// several pictures the user never previewed. The set-aware entry
    /// point is `applyToSelectedLayers`, and it is for property writes.
    func applyToActiveLayer(
        _ actionName: String, record: [ActionStep], _ op: (RasterImage) -> RasterImage?
    ) {
        applyCapturedOperation(captureOperation(), actionName, record: record, op)
    }

    /// applyEdit for an edit that rewrites a layer's pixels DESTRUCTIVELY —
    /// painting, filling, filtering, adjusting. A plain raster layer goes
    /// straight through. On a DESCRIBED layer — text, a shape —
    /// the raster would stop being the rendering of its description, so the
    /// user is asked first: Cancel abandons the edit entirely, Rasterize
    /// drops the description inside the SAME edit, keeping it one undo step.
    ///
    /// Whole-document geometry (rotate, flip, image/canvas size) and a plain
    /// affine Free Transform deliberately do NOT come through here: they
    /// COMPOSE into a described layer's description (DescribedLayer.swift)
    /// and the layer re-renders through it, so the description survives.
    /// The perspective path and a description that cannot render right now
    /// still do.
    ///
    /// USER-initiated edits only — it can put up a modal alert, so anything
    /// running on the agent's dispatched-to-main path must use applyEdit and
    /// decide about the metadata itself.
    func applyRasterizingEdit(
        _ actionName: String, layer idx: Int, record: [ActionStep],
        _ transform: (RasterDocument) throws -> RasterDocument?
    ) {
        guard doc != nil else {
            NSSound.beep()
            return
        }
        let described = layerDescribesSource(idx)
        // Both early returns are BEFORE the forward, so a beeped missing
        // document and a cancelled rasterize prompt carry the step away with
        // them: nothing is recorded and no stale step is left behind.
        guard confirmRasterize(layer: idx) else { return }
        applyEdit(actionName, record: record) { doc in
            guard let updated = try transform(doc) else { return nil }
            guard described else { return updated }
            return updated.withLayerMeta(idx, nil) ?? updated
        }
    }

    /// True when layer `idx`'s pixels are the RENDERING of a description —
    /// a text layer's string, a shape's geometry
    /// — which is exactly what a destructive edit would contradict. Judged
    /// by the kind the meta CLAIMS (`LayerDescription.claimedKind`), not by
    /// whether this build can decode it: a description at a payload version
    /// this build does not know still describes the pixels, and leaving it
    /// in place through a paint would hand the next build that understands
    /// it a description of pixels that no longer exist. (An adjustment
    /// layer has no pixels of its own to contradict, so it is deliberately
    /// not one of these.) The paths that cannot use applyRasterizingEdit ask
    /// this: the live-edit brush then clears the metadata itself, and the
    /// Free Transform commit composes into the description or prompts.
    /// `source` (nil: the document's own handle) is a handle the document
    /// does not hold yet — a ⌥⌘T copy's.
    func layerDescribesSource(_ idx: Int, in source: RasterDocument? = nil) -> Bool {
        guard let doc = source ?? doc else { return false }
        return doc.hasArtworkDescription(idx) || LayerDescription.claimedKind(of: doc.layerMeta(idx)) != nil
    }

    /// Layer `idx`'s description (text or shape), for the
    /// feature extensions; nil for a plain raster layer.
    func layerDescription(_ idx: Int) -> LayerDescription? {
        doc?.layerDescription(idx)
    }

    /// Asks — once, app-modally — whether a destructive edit may drop layer
    /// `idx`'s description, in the words of whichever kind it is. True means
    /// the edit may proceed: either the layer carries no description, or the
    /// user confirmed. `reason` adds a line saying why THIS edit has to
    /// rasterize when that is not obvious (`unrenderableReason`); `source`
    /// is `layerDescribesSource`'s.
    func confirmRasterize(
        layer idx: Int, reason: String? = nil, in source: RasterDocument? = nil
    ) -> Bool {
        guard let doc = source ?? doc else { return true }
        if doc.hasRetainedSource(idx) {
            presentError(DoctoredCoreError(message: RetainedImagePolicy.rasterizeFirst))
            return false
        }
        let name = doc.layerInfo(idx)?.name ?? "this layer"
        // By the claimed kind, like layerDescribesSource: a description this
        // build cannot decode is still dropped, and still asked about.
        let metadata = doc.layerMeta(idx)
        if doc.hasArtworkDescription(idx), metadata == nil {
            presentError(DoctoredCoreError(message: "The artwork description could not be read. No destructive edit was applied."))
            return false
        }
        let confirmed: Bool
        switch LayerDescription.claimedKind(of: metadata) {
        case .text?:
            confirmed = TextLayer.confirmRasterize(layerName: name, reason: reason)
        case .shape?:
            confirmed = ShapeLayer.confirmRasterize(layerName: name, reason: reason)
        case .fill?:
            confirmed = FillLayer.confirmRasterize(layerName: name, reason: reason)
        case nil:
            return true
        }
        if !confirmed {
            SessionCommitRefusal.note("The prompt to rasterize “\(name)” was cancelled in the document window.")
        }
        return confirmed
    }

    /// Why a Free Transform on layer `idx` has to rasterize instead of
    /// composing into its description — because the description cannot
    /// render right now (a text family not installed here) or because the
    /// pixels are not its rendering any more (`RasterDocument.describedRasterIsStale`); nil
    /// for a plain layer, a shape that could render, or a description that
    /// could render over its own raster. `source` is `layerDescribesSource`'s.
    func unrenderableReason(layer idx: Int, in source: RasterDocument? = nil) -> String? {
        guard let doc = source ?? doc, let description = doc.layerDescription(idx) else {
            return nil
        }
        guard !description.isRenderable else {
            guard doc.describedRasterIsStale(idx) else { return nil }
            return "It has to rasterize because its pixels are no longer the rendering of its "
                + "description — an earlier version's Image Size resampled them while the "
                + "description kept its original size — so composing the transform into the "
                + "description would re-render the layer at a size you never saw."
        }
        switch description {
        case let .text(payload):
            return "It has to rasterize because its font family “\(payload.font)” is not "
                + "installed here: re-rendering it through the transform would substitute a "
                + "face."
        case .shape, .fill:
            return nil
        }
    }

    /// Publish an editable document replacement and refresh its projection.
    func docDidChange(isLive: Bool = false) {
        clampLayerSelection()
        refreshProjection()
        NotificationCenter.default.post(
            name: .imageDocumentImageDidChange, object: self, userInfo: ["isLive": isLive])
    }

    /// Prune-and-clamp, after every document change: an edit that removed
    /// entries (delete, merge, ungroup) leaves indices in the set that name
    /// nothing, or name something else. Pruning is what stops a stale index
    /// from silently retargeting the next set op.
    private func clampLayerSelection() {
        guard let doc = doc else { return }
        layerSelection = layerSelection.clamped(to: doc.layerCount)
    }

    /// A render-neutral edit's prepared edit, at either depth: the current
    /// projection and preview, carried over when they are the current
    /// document's own (8-bit renames and their Undo/Redo too).
    func carriedProjection(from current: RasterDocument,
                           to updated: RasterDocument) -> PreparedDocumentEdit? {
        if current.precision == nil, updated.precision == nil {
            guard projectionDocument === current, projectionError == nil,
                  let projection, let previewImage,
                  updated.width == current.width, updated.height == current.height
            else { return nil }
            return PreparedDocumentEdit(carrying: updated, projection: projection,
                                        preview: previewImage)
        }
        guard current.precision != nil, updated.precision != nil,
              projectionDocument === current, projectionError == nil,
              let precisionProjection, let previewImage,
              updated.width == current.width, updated.height == current.height else { return nil }
        return PreparedDocumentEdit(carrying: updated, precisionProjection: precisionProjection,
                                    preview: previewImage)
    }

    func refreshProjection() {
        defer { RasterResources.shared.trimCachesIfNeeded() }
        guard let doc else {
            projection = nil
            precisionProjection = nil
            previewImage = nil
            projectionDocument = nil
            projectionError = nil
            return
        }
        if let prepared = preparedEdit, prepared.document === doc {
            preparedEdit = nil
            projection = prepared.projection
            precisionProjection = prepared.precisionProjection
            projectionDocument = prepared.error == nil ? doc : nil
            if let preview = prepared.preview { previewImage = preview }
            projectionError = prepared.error
            return
        }
        preparedEdit = nil
        if doc.precision != nil {
            if BackgroundWork.occupied {
                projection = nil
                precisionProjection = nil
                projectionDocument = nil
                projectionError = "The 16-bit display will refresh when the current background operation finishes."
                schedulePrecisionProjectionRefresh()
                return
            }
            let prepared: PreparedDocumentEdit
            do {
                prepared = try BackgroundWork.wait(try WorkerTask { PreparedDocumentEdit(doc) },
                                                  title: "Preparing 16-bit Display")
            } catch {
                guard self.doc === doc else { return }
                projection = nil
                precisionProjection = nil
                projectionDocument = nil
                projectionError = error.localizedDescription
                return
            }
            guard self.doc === doc else { return }
            projection = nil
            precisionProjection = prepared.precisionProjection
            projectionDocument = prepared.error == nil ? doc : nil
            if let preview = prepared.preview { previewImage = preview }
            projectionError = prepared.error
            return
        }
        precisionProjection = nil
        do {
            let rendered = try doc.render()
            projection = rendered
            projectionDocument = doc
            previewImage = rendered.makeCGImage(in: doc.colorSpace)
            projectionError = nil
        } catch {
            projection = nil
            projectionDocument = nil
            projectionError = error.localizedDescription
        }
    }

    /// Only pixels representing the current immutable document may be copied or
    /// exported. A failed preview must not silently become the next saved image.
    func currentProjection() throws -> RasterImage {
        if let projectionError { throw DoctoredCoreError(message: projectionError) }
        guard let doc else { throw DoctoredCoreError(message: "No image loaded.") }
        guard doc.precision == nil else {
            throw DoctoredCoreError(message: "This operation requires an explicit 8-bit conversion. The document remains 16-bit.")
        }
        if projectionDocument === doc, let projection { return projection }
        return try doc.render()
    }

    /// The retained typed projection of the CURRENT document, or nil. Never
    /// renders: the canvas proof reads it and waits for the next
    /// refresh rather than compositing on the main thread.
    var retainedPrecisionProjection: RasterPrecisionImage? {
        guard projectionError == nil, let doc, projectionDocument === doc else { return nil }
        return precisionProjection
    }

    /// The retained projection of the CURRENT 8-bit document, or nil. Never
    /// renders: the 8-bit canvas proof and `render_proof` read it
    /// rather than compositing the document again.
    var retainedByteProjection: RasterImage? {
        guard projectionError == nil, let doc, !doc.isHighDepth,
              projectionDocument === doc else { return nil }
        return projection
    }

    func currentPrecisionProjection() throws -> RasterPrecisionImage {
        if let projectionError { throw DoctoredCoreError(message: projectionError) }
        guard let doc, let source = doc.precision else {
            throw DoctoredCoreError(message: "No 16-bit or 32-bit float document is loaded.")
        }
        if projectionDocument === doc, let precisionProjection { return precisionProjection }
        let context = try PrecisionWorkContext(source: source)
        return try source.render(workingLimit: context.limit, otherRetained: context.otherRetained)
    }

    /// Deliberate display/interchange conversion, never an editing source.
    /// `view` is a float document's view exposure and candidate tone map
    /// (`ViewExposure.swift`); it changes nothing at another depth, and the
    /// default is the document's own display.
    func currentDisplayProjection(view: FloatDisplayView = .document) throws -> RasterImage {
        guard let doc, doc.precision != nil else { return try currentProjection() }
        let image = try currentPrecisionProjection()
        let context = try PrecisionWorkContext(image: image)
        // A float projection has no direct RGBA8 form: it goes through the
        // document's own tone map (`FloatDocument.swift`).
        return try doc.display(of: image, context: context, view: view)
    }

    // MARK: - Live edits (Move drags, opacity slider scrubs)

    /// True while a live-edit gesture is in progress (between beginLiveEdit
    /// and endLiveEdit). The layers panel uses this — together with the
    /// "isLive" userInfo on imageDocumentImageDidChange — to defer expensive
    /// refreshes until the gesture ends.
    var isLiveEditing: Bool { liveEditBase != nil }

    /// Snapshots the current doc handle. The following updateLiveEdit calls
    /// swap the document without touching the undo stack; endLiveEdit turns
    /// the whole gesture into a single undo step.
    func beginLiveEdit() {
        guard liveEditBase == nil, !isPublishingEditingState else { return }
        liveEditBase = doc
        liveEditingState = captureEditingState()
    }

    /// Swaps in `new` and refreshes the UI with NO undo registration. The
    /// change notification carries ["isLive": true].
    func updateLiveEdit(_ new: RasterDocument) {
        guard let base = liveEditBase, (base.precision == nil) == (new.precision == nil) else {
            NSSound.beep()
            return
        }
        if let reason = PersistedWarpPolicy.publicationRefusal(from: base, to: new) {
            presentError(DoctoredCoreError(message: reason))
            return
        }
        doc = new
        docDidChange(isLive: true)
    }

    /// Drop a captured gesture's live session without publishing history or
    /// changing the current document. The caller restores its own preview
    /// first, only when that preview still owns `doc`.
    func abandonLiveEdit(ownedBy base: RasterDocument) {
        guard liveEditBase === base else { return }
        liveEditBase = nil
        liveEditingState = nil
        NotificationCenter.default.post(
            name: .imageDocumentImageDidChange, object: self, userInfo: ["isLive": false])
    }

    /// Ends a live edit by putting the pre-gesture document back: no undo
    /// step and no Action step, whatever the gesture's ticks did. For a
    /// gesture that keeps no base of its own — a Move drag that turned into
    /// a layer drag to another document.
    func cancelLiveEdit() {
        guard let base = liveEditBase else { return }
        if doc !== base {
            doc = base
            docDidChange(isLive: true)
        }
        endLiveEdit("", record: .notACommand)
    }

    /// Registers one undo step from the beginLiveEdit snapshot to the current
    /// doc (none if the doc never changed — same handle), then posts one
    /// final ["isLive": false] change notification so listeners that skip
    /// live updates refresh exactly once per gesture.
    ///
    /// `record` is an `@autoclosure` here and nowhere else: a gesture's step
    /// is the one recorded payload big enough to be felt — a 10,000-point
    /// stroke's arguments cost 16 ms to build at mouse-up, on every stroke,
    /// whether or not anyone was recording — while every other commit's step
    /// is a handful of numbers. `ActionRecorder.recordGesture` states the
    /// rule that makes skipping it safe.
    func endLiveEdit(_ actionName: String, record: @autoclosure () -> [ActionStep]) {
        defer { fadeHint = nil }
        guard let base = liveEditBase else { return }
        liveEditBase = nil
        let before = liveEditingState
        liveEditingState = nil
        if base !== doc {
            countEditChange(.changeDone)
            // Inside the `base !== doc` branch, deliberately: a Move drag
            // that ended where it started, and a stroke that missed the
            // layer, register no undo step and must record no action step.
            // `base` is the pre-gesture handle, so it carries the canvas the
            // gesture's points were sampled on.
            ActionRecorder.shared.recordGesture(
                record(), on: self, canvas: (width: base.width, height: base.height))
        }
        NotificationCenter.default.post(
            name: .imageDocumentImageDidChange, object: self, userInfo: ["isLive": false])
        if base !== doc, let before { registerHistoryChange(from: before, actionName: actionName) }
        // `record: []`: a stroke's hint is `fadeHint`, so the lazy record is
        // never built for it.
        if base !== doc { noteFadeCandidate(name: actionName, before: base, record: []) }
    }

    // MARK: - Canvas-session safety

    /// Commits any in-progress canvas session — text entry, a Free Transform
    /// — so save/close/export paths never silently drop what the user can see
    /// on the canvas. The commits run through applyEdit, which also dirties
    /// the document, so close paths then show the standard unsaved-changes
    /// prompt. False at the first editor whose refused commit left a session
    /// open — the refusal was already presented, so the caller just stops.
    /// Internal, not private: the print operation (ImageDocument+Print.swift)
    /// is the same kind of read of the composite and does the same first.
    func commitPendingCanvasSessions() -> Bool {
        for controller in windowControllers {
            guard let editor = controller.contentViewController as? EditorViewController
            else { continue }
            if !editor.commitPendingSessions() { return false }
        }
        return true
    }

    /// Answers an AppKit `document:shouldClose:contextInfo:` or
    /// `document:didSave:contextInfo:` callback with NO, synchronously — the
    /// stop a refused session commit makes on a close or close-time save.
    /// The IMP cast is `AutomationAlerts.installDocumentCloseReview`'s.
    /// `answer` is the reply (NO unless a caller says otherwise): a contents
    /// window's close-time Save answers whether its commit landed.
    func answerDocumentCallback(
        _ delegate: Any?, _ selector: Selector?, contextInfo: UnsafeMutableRawPointer?,
        answer: Bool = false
    ) {
        typealias Answer = @convention(c) (AnyObject, Selector, NSDocument, ObjCBool,
                                           UnsafeMutableRawPointer?) -> Void
        guard let delegate = delegate as AnyObject?, let selector,
              let cls = object_getClass(delegate),
              let imp = class_getMethodImplementation(cls, selector) else { return }
        unsafeBitCast(imp, to: Answer.self)(delegate, selector, self, ObjCBool(answer), contextInfo)
    }

    override func save(_ sender: Any?) {
        guard commitPendingCanvasSessions() else { return }
        super.save(sender)
    }

    override func save(
        to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType,
        completionHandler: @escaping (Error?) -> Void
    ) {
        guard commitPendingCanvasSessions() else {
            completionHandler(CocoaError(.userCancelled))
            return
        }
        if saveOperation != .saveToOperation {
            do {
                try requirePreservingSave(typeName)
            } catch {
                completionHandler(error)
                return
            }
        }
        super.save(
            to: url, ofType: typeName, for: saveOperation,
            completionHandler: { [weak self] error in
                self?.refreshWindowDirtyIndicators()
                completionHandler(error)
            })
    }

    override func saveAs(_ sender: Any?) {
        guard commitPendingCanvasSessions() else { return }
        super.saveAs(sender)
    }

    override func close() {
        hasClosed = true
        if countsAsOpen {
            countsAsOpen = false
            RasterResources.shared.noteDocumentClosed()
        }
        NotificationCenter.default.post(name: .imageDocumentDidClose, object: self)
        RawMigrationReview.discard(for: self)
        precisionProjectionTimer?.invalidate()
        precisionProjectionTimer = nil
        DocumentJobs.cancel(for: self)
        RecoveryCoordinator.shared.documentDidClose(self)
        historyWindowController?.close()
        historyWindowController = nil
        snapshotsWindowController?.close()
        snapshotsWindowController = nil
        layerCompsWindowController?.close()
        layerCompsWindowController = nil
        super.close()
    }

    override func canClose(
        withDelegate delegate: Any, shouldClose shouldCloseSelector: Selector?,
        contextInfo: UnsafeMutableRawPointer?
    ) {
        guard commitPendingCanvasSessions() else {
            answerDocumentCallback(delegate, shouldCloseSelector, contextInfo: contextInfo)
            return
        }
        super.canClose(
            withDelegate: delegate, shouldClose: shouldCloseSelector, contextInfo: contextInfo)
    }

    // MARK: - Export

    /// The informational half of an export: what the chosen format could
    /// not carry. Never a failure — the file was written.
    func presentExportNotice(_ message: String) {
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = "Exported"
        alert.informativeText = message
        if let window = windowForSheet {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }

    // MARK: - Printing

    /// NSDocument's own print selectors are delivered to the DOCUMENT, not
    /// to the editor, so they never reach `EditorViewController`'s
    /// validation switch: File > Print and Page Setup would stay enabled on
    /// a document with no image without this.
    override func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        switch item.action {
        case #selector(printDocument(_:)), #selector(runPageLayout(_:)):
            return doc != nil
        // Both depths: the file follows the document's own depth. A float
        // document stays enabled ON PURPOSE (reversing 10B's grey-out):
        // both core reports are unavailable, so `exportPSD` shows the core's
        // refusal, which names the routes that work, before any panel opens.
        case #selector(exportPSD(_:)):
            return doc != nil
        default:
            return super.validateUserInterfaceItem(item)
        }
    }
}
