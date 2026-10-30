import AppKit

// `PastePlan.resolve` answers `.typed` (any typed payload, every depth), `.external` (another app's
// image, `ExternalClipboardImage`, every depth), `.editable` or `.refused` (no image). **A
// paste converts** — never refused for depth or profile: the core converts a typed payload
// (promoting a 16-bit one into float), an external image is decoded into the document's drawing
// space (exact ×257 `bytePayload` at depth, promoted into float by the core), an UNTAGGED external
// image keeps its numbers in the destination's profile (measured against Photoshop,
// `paste_depth.py` case f), and Paste in Place of an external image lands like Paste
// (`PasteWork.landing`, `context.landing`). `PasteProfile` (`.convert` / `.preserve`, `differ` is the ONE
// mismatch test: bytes and name) is the Paste Profile Mismatch choice: `pastePlanned` asks it
// (`askPasteProfile`: Convert / Don't Convert / Cancel) only when `ColorSettings.askWhenPasting`
// (Image ▸ Mode ▸ Working Space ▸ Ask When Pasting, app-wide, no MCP setter) is on and
// `PastePlan.profileMismatch` answers; MCP `paste` takes it as `profile` (a `convert` boolean is
// accepted and ignored; `external: "refuse"` refuses an external image at depth).
// `PasteWork.byteTransform` is the 8-bit edit (published through `applyEdit` /
// `performGroupedEdit`), which uses a payload's own `rgba8()` when it is u16 in the document's
// exact profile and otherwise the core's `rgba8(in:profile:)` into the document's profile
// (`converted: true`), so a pasteboard payload and a `payload_path` file take one path;
// `PasteWork.typedDocument` is the typed one; `PasteOutcome.profile` is "same" / "converted" /
// "preserved". `PasteOutcome` carries the reply, the PPI disclosure ("Paste Layer (300 → 72 ppi)")
// and the landed RZCB, which `ClipboardPayloadStore.pasteStep` writes as `<uuid>.drcb` under
// `DR_CLIPBOARD_PAYLOADS_DIR` ONLY while recording a SAVED-PAYLOAD step (MCP `paste` with
// `payload_path` or `record_payload`), so that `paste {payload_path}` packages as a
// `$asset`. Every other recorded paste — the Edit menu's, MCP's default, pixels and layers — is the
// LIVE step `ActionSteps.paste(payloadPath: nil, …)`: the requested placement, no pixels and no
// position, so a replay pastes whatever the clipboard holds (`ActionCatalogFacts.pasteSource` names
// the kind on the Actions row). `NewFromClipboard` builds a document at the payload's depth (no
// sheet; a held layer clipboard goes to `LayerClipboard.Entry.newDocument` instead:
// `layersTrimToContent`, `layersAttributes`), trims a typed payload to its content box first
// (`TypedClipboardPayload.trimmed`)
// and rebuilds the document around an editable section
// (`RasterDocument.restoringEditableDocument` in `ClipboardEditable.swift`; never throws, a fallback
// is disclosed in `Made.editable`).
//
// `PasteWork` is the ONE name rule (`name` nil → the record's name, else "Pasted Layer";
// `PasteOutcome.name`), and `pasteStep` records `outcome.name`.

/// How pixels in another profile land — the Paste Profile
/// Mismatch choice. `.convert` (the default, and the only behaviour with
/// "Ask When Pasting" off) converts them into the destination's profile,
/// keeping their appearance; `.preserve` ("Don't Convert") keeps their
/// numbers and relabels them. Depth always converts.
enum PasteProfile: String {
    case convert
    case preserve

    /// The ONE mismatch test, for typed payloads, external images and
    /// layers alike: the bytes differ AND the profiles' names differ, so two
    /// encodings of one space (the same description) never prompt.
    static func differ(_ a: Data, _ b: Data) -> Bool {
        a != b && RasterProfile.inspect(a).name != RasterProfile.inspect(b).name
    }

    /// The outcome's `profile`: "same" when `source` does not `differ` from
    /// `destination`, else "converted" or "preserved".
    func outcome(source: Data, destination: Data) -> String {
        guard Self.differ(source, destination) else { return "same" }
        return self == .preserve ? "preserved" : "converted"
    }
}

/// An image another app put on the pasteboard, decoded by the
/// platform (`Bitmap.pasteboardImage`) and not yet converted: the image, the
/// ICC bytes its numbers are in (`sourceProfile`) and that profile's name —
/// what the Paste Profile Mismatch alert names. An UNTAGGED image
/// (`untagged`, `ExternalClipboardImage.read` in `Bitmap.swift`) has no
/// profile of its own: its numbers are taken to be the DESTINATION's
/// (measured against Photoshop, `paste_depth.py` case f), so it never
/// converts and never asks.
struct ExternalClipboardImage {
    let image: CGImage
    let sourceProfile: Data
    let sourceName: String
    let untagged: Bool
    /// #1 phase 4c: the pasteboard's own TIFF as a CMYK payload when it is
    /// a CMYK TIFF the core reads; it pastes as that payload at every
    /// destination (`ExternalClipboardImage.cmykPayload`).
    let cmyk: TypedClipboardPayload?
    /// The colour model CoreGraphics converted from while decoding a Gray,
    /// Lab or CMYK image ("Gray", "Lab", "CMYK"); nil for an RGB image.
    let platformModel: String?
    /// #1 phase 4: the pasteboard's own TIFF as a Gray payload when it is a
    /// gray TIFF the core reads; it pastes as that payload at every
    /// destination (`ExternalClipboardImage.grayPayload`), set by
    /// `PastePlan.resolve`.
    var gray: TypedClipboardPayload?

    var size: (width: Int, height: Int) { (image.width, image.height) }

    /// The outcome's `profile` for this image under `profile`: "same" for an
    /// untagged image, otherwise `PasteProfile.outcome`.
    func outcome(_ profile: PasteProfile, destination: Data) -> String {
        untagged ? "same" : profile.outcome(source: sourceProfile, destination: destination)
    }
}

/// Phase 12G-2: every clipboard BOUNDARY is decided before the edit;
/// none of them is refused for depth or profile.
///
/// `PastePlan` decides what a Paste will do BEFORE any edit — one
/// implementation for the Edit menu, the MCP `paste` tool and the UI probe:
///
/// - Any typed payload is `.typed` at every depth: the core converts it into
///   the document's depth and profile (`dr_precision_doc_clipboard_paste`,
///   which promotes a 16-bit payload into 32-bit float; at 8 bits
///   `RasterClipboardPayload.rgba8(in:profile:)`), so a pasteboard payload
///   and a `payload_path` file take one path.
/// - Another app's image (the held token of another instance included) is
///   `.external` at every depth: CoreGraphics decodes it into the
///   document's drawing space (or, under `.preserve`, its own space), and at
///   16 bits and float its exact ×257 byte payload enters the core.
/// - With "Ask When Pasting" on (`ColorSettings.askWhenPasting`) a `.typed`
///   or `.external` plan whose profile differs (`profileMismatch`) asks
///   Convert / Don't Convert / Cancel first.
///
/// Phase 12G-3: a payload carrying an editable record this document can
/// restore resolves to `.editable` BEFORE every depth or profile rule — no
/// pixels cross and nothing is asked; the record is restored through the
/// tools' own paths (`ClipboardEditable.swift`).
///
/// `PasteWork` runs a plan (the 8-bit transform or the typed document),
/// `ClipboardPayloadStore` makes a recorded Paste a replayable `$asset`
/// step, and `NewFromClipboard` builds a document at the payload's depth.
enum PastePlan {
    case typed(TypedClipboardPayload)
    case external(ExternalClipboardImage)
    case editable(TypedClipboardPayload, EditableClipboardRecord)
    case refused(String)

    static let noImage = "The clipboard holds no image."

    /// What a Paste into `document` will do. `payloadFile` (MCP
    /// `payload_path`, a replayed step's `$asset`) is decoded INSTEAD of the
    /// pasteboard's typed payload. `profile` reaches an editable record's
    /// disposition (Don't Convert keeps a CMYK record's ink, #1); the UI
    /// never asks for an editable plan, so it passes `.convert`.
    static func resolve(for document: ImageDocument, pasteboard: NSPasteboard,
                        payloadFile: URL?, profile: PasteProfile = .convert) throws -> PastePlan {
        guard let doc = document.doc else { return .refused(noImage) }
        let typed: TypedClipboardPayload?
        if let payloadFile {
            typed = try TypedClipboardPayload.read(file: payloadFile)
        } else if !doc.isHighDepth {
            // An unreadable typed payload (another build's RZCB) beside a
            // public image pastes that image at 8 bits, as it would alone.
            do { typed = try TypedClipboardPayload.read(pasteboard) } catch {
                guard let image = ExternalClipboardImage.read(pasteboard, cmykFallback: nil) else {
                    throw error
                }
                return .external(withGray(image, pasteboard, doc))
            }
        } else {
            typed = try TypedClipboardPayload.read(pasteboard)
        }
        if let typed, let record = doc.editableDisposition(of: typed, profile: profile)?.record {
            return .editable(typed, record)
        }
        if let typed { return .typed(typed) }
        // #1 phase 4c: an untagged CMYK TIFF pastes in a CMYK destination's
        // own profile (the working CMYK elsewhere).
        let fallback = doc.isCmyk ? ColorConversion.profileData(of: doc) : nil
        if payloadFile == nil, let image = ExternalClipboardImage.read(pasteboard,
                                                                       cmykFallback: fallback) {
            return .external(withGray(image, pasteboard, doc))
        }
        return .refused(noImage)
    }

    /// #1 phase 4: `image` with the pasteboard's gray TIFF as its Gray payload (an untagged one
    /// in a Gray destination's own profile, the working Gray elsewhere); a CMYK image keeps its
    /// CMYK payload alone.
    private static func withGray(_ image: ExternalClipboardImage, _ pasteboard: NSPasteboard,
                                 _ doc: RasterDocument) -> ExternalClipboardImage {
        guard image.cmyk == nil else { return image }
        var image = image
        image.gray = ExternalClipboardImage.grayPayload(
            pasteboard, fallback: doc.isGray ? ColorConversion.profileData(of: doc) : nil)
        return image
    }

    /// The Paste Profile Mismatch alert's two names — the plan's profile and
    /// the document's — for a `.typed` or `.external` plan whose profile
    /// `PasteProfile.differ`s from the document's (an untagged document
    /// counts as sRGB); nil otherwise, `.editable` and an untagged external
    /// image (whose numbers are the document's) included.
    ///
    /// #1 phase 4c: pixels that change colour mode always convert, so a
    /// paste across modes never asks; between CMYK documents the CMYK
    /// profiles are compared (`cmykProfileData`). Lab phase 4c: nor does a
    /// paste into or out of Lab Color (one twin, so Lab → Lab never asks).
    static func profileMismatch(_ plan: PastePlan,
                                in doc: RasterDocument) -> (source: String, destination: String)? {
        let source: (bytes: Data, name: String)
        switch plan {
        case .typed(let typed):
            guard typed.isCmyk == doc.isCmyk, typed.isGray == doc.isGray,
                  typed.isLab == doc.isLab else { return nil }
            source = (typed.profileData, typed.profileDescription)
        case .external(let image):
            if let cmyk = image.cmyk {
                guard doc.isCmyk else { return nil }
                source = (cmyk.profileData, cmyk.profileDescription)
            } else if let gray = image.gray {
                // #1 phase 4: a gray TIFF asks only between Gray profiles.
                guard doc.isGray else { return nil }
                source = (gray.profileData, gray.profileDescription)
            } else {
                guard !image.untagged, !doc.isCmyk, !doc.isGray, !doc.isLab else { return nil }
                source = (image.sourceProfile, image.sourceName)
            }
        case .editable, .refused: return nil
        }
        let destination = ColorConversion.profileData(of: doc)
        guard PasteProfile.differ(source.bytes, destination) else { return nil }
        return (source.name, RasterProfile.inspect(destination).name)
    }
}

/// What a Paste did: where it landed and every boundary it crossed.
struct PasteOutcome {
    let landing: Int
    /// The LANDED placement: never `.auto`.
    let placement: PastePlacement
    let offset: (x: Int, y: Int)
    let depth: Int
    let typed: Bool
    let originDepth: Int
    let converted: Bool
    let external: Bool
    let narrowed: Bool
    /// "same", "converted" or "preserved" (`PasteProfile.outcome`);
    /// nil for an editable restore, where no pixels cross.
    let profile: String?
    let ppi: (payload: (Double, Double)?, document: (Double, Double))
    /// The RZCB a SAVED-PAYLOAD step replays (the typed payload itself — a
    /// Don't Convert choice is recorded as the step's `profile` — the
    /// promoted payload, or at 8 bits `bytePayload` of the pasted image).
    /// `ClipboardPayloadStore.pasteStep` is its one reader; a live step
    /// stores none of it.
    let landedBytes: Data
    /// The resolved layer name (`PasteWork`'s one name rule).
    let name: String
    /// The payload's editable record: restored, or pasted as pixels for
    /// `reason`; nil when the payload carries none.
    let editable: (type: String, restored: Bool, reason: String?)?
    /// Paste Into / Paste Outside: the mask the pasted layer got; nil for
    /// every other placement.
    let mask: PasteMask?
    /// #1 phase 4c: the colour model or CMYK profile the pixels crossed
    /// (the reply's `color_conversion`); nil when they kept their model.
    var colorConversion: ColorConversion?
    /// #1 Indexed phase 2d: an Indexed paste's admitted working memory (its core plan's
    /// `scratch_bytes`), the reply's `working_memory_plan_bytes`; nil for every other paste.
    var indexedPlanBytes: UInt64? = nil

    var ppiMatches: Bool {
        guard let payload = ppi.payload else { return true }
        return payload.0 == ppi.document.0 && payload.1 == ppi.document.1
    }

    /// "Paste Layer" (or "Paste Into" / "Paste Outside"), with
    /// " (CMYK → RGB)" when the pixels changed colour model (#1 phase 4c)
    /// and " (300 → 72 ppi)" when the payload's PPI differs from the
    /// document's — both disclosed in the history row itself.
    var historyName: String {
        let base = (mask?.historyName ?? "Paste Layer") + (colorConversion?.historySuffix ?? "")
        guard !ppiMatches, let payload = ppi.payload else { return base }
        func text(_ v: Double) -> String { String(format: "%g", v) }
        return "\(base) (\(text(payload.0)) → \(text(ppi.document.0)) ppi)"
    }

    /// An editable record's disposition as MCP JSON: `null`, or `{type,
    /// restored, reason?}` — Paste's reply and New from Clipboard's alike.
    static func editableJSON(_ editable: (type: String, restored: Bool, reason: String?)?) -> Any {
        guard let editable else { return NSNull() }
        var object: [String: Any] = ["type": editable.type, "restored": editable.restored]
        if let reason = editable.reason { object["reason"] = reason }
        return object
    }

    var json: [String: Any] {
        var payloadPPI: Any = NSNull()
        if let p = ppi.payload { payloadPPI = [p.0, p.1] }
        var object: [String: Any] = [
            "ok": true, "layer": landing, "placement": placement.rawValue,
            "offset": [offset.x, offset.y], "depth": depth,
            "typed": typed, "origin_depth": originDepth, "converted": converted,
            "external": external, "narrowed": narrowed, "name": name,
            "profile": profile ?? NSNull(),
            "editable": Self.editableJSON(editable),
            "mask": mask?.json ?? NSNull(),
            "color_conversion": ColorConversion.json(colorConversion),
            "ppi": ["payload": payloadPPI, "document": [ppi.document.0, ppi.document.1],
                    "match": ppiMatches] as [String: Any],
        ]
        if let indexedPlanBytes { object["working_memory_plan_bytes"] = indexedPlanBytes }
        return object
    }
}

enum PasteWork {
    static let defaultName = "Pasted Layer"
    static let maskRefusal = "The pasted layer's mask could not be set."

    /// A masked placement requires its `PasteMask`, and only it may carry one.
    private static func requireMask(_ placement: PastePlacement, _ mask: PasteMask?) throws {
        guard placement.masksSelection == (mask != nil) else {
            throw DoctoredCoreError(message: "Paste Into and Paste Outside require a selection "
                + "mask, and no other paste takes one.")
        }
    }

    private static func depth(of doc: RasterDocument) -> Int {
        doc.isFloat ? 32 : doc.isHighDepth ? 16 : 8
    }

    /// Where a paste of `plan` lands, decided before any edit. Masked →
    /// (request, the mask's frame); `.at` → (.at, (x, y, payload size));
    /// `.auto` → `context.landing` (the selection, the view or
    /// the centre — never the source position); `.centered` → (request,
    /// nil); `.inPlace` → `context.inPlace` over the payload's source offset
    /// and size (`.inPlace`, or `.relativeInPlace` across canvas sizes),
    /// except that an external image, which has no source position, lands
    /// like Paste. `.refused` throws its reason.
    static func landing(_ plan: PastePlan, request: PastePlacement, at: (x: Int, y: Int)?,
                        mask: PasteMask?, canvas: (width: Int, height: Int),
                        context: PasteContext) throws -> PasteLanding {
        try requireMask(request, mask)
        func payload() throws -> (source: (x: Int, y: Int)?, size: (width: Int, height: Int)) {
            switch plan {
            case .typed(let t), .editable(let t, _):
                return (t.offset, t.size)
            case .external(let image):
                return (nil, image.size)
            case .refused(let reason):
                throw DoctoredCoreError(message: reason)
            }
        }
        switch request {
        case .into, .outside:
            guard let mask else { throw DoctoredCoreError(message: maskRefusal) }
            return PasteLanding(placement: request, frame: mask.frame)
        case .at:
            guard let at else {
                throw DoctoredCoreError(message: "placement at needs both x and y.")
            }
            let size = try payload().size
            return PasteLanding(placement: .at, frame: CGRect(x: at.x, y: at.y,
                                                               width: size.width,
                                                               height: size.height))
        case .auto:
            _ = try payload()
            return context.landing
        case .inPlace:
            let (source, size) = try payload()
            guard let source else { return context.landing }
            return context.inPlace(source: source, size: size, canvas: canvas)
        case .centered:
            return PasteLanding(placement: request, frame: nil)
        case .selection, .view, .relativeInPlace:
            throw DoctoredCoreError(message: "placement \(request.rawValue) is a landed placement, "
                + "not a request.")
        }
    }

    /// An editable restore's outcome: the payload window's placement, and
    /// the shift that moves the record's anchor with it. No pixels cross.
    private static func editableOutcome(
        _ payload: TypedClipboardPayload, _ record: EditableClipboardRecord,
        facade: RasterDocument, landing: Int, spot: PasteLanding, name: String,
        mask: PasteMask?
    ) -> (outcome: PasteOutcome, shift: (x: Int, y: Int)) {
        let offset = spot.placement.offset(payload: payload.offset, size: payload.size,
                                           canvas: (facade.width, facade.height),
                                           frame: spot.frame)
        let outcome = PasteOutcome(
            landing: landing, placement: spot.placement, offset: offset,
            depth: depth(of: facade), typed: true,
            originDepth: payload.originDepth, converted: false, external: false, narrowed: false,
            profile: nil,
            ppi: (payload: (payload.ppi.x, payload.ppi.y),
                  document: (facade.resolution.x, facade.resolution.y)),
            landedBytes: payload.handle.bytes, name: name,
            editable: (type: record.type, restored: true, reason: nil), mask: mask)
        return (outcome, (offset.x - payload.offset.x, offset.y - payload.offset.y))
    }

    /// A payload's section that pastes as pixels, for the outcome.
    private static func rasterEditable(_ payload: TypedClipboardPayload,
                                       in doc: RasterDocument)
        -> (type: String, restored: Bool, reason: String?)? {
        doc.editableDisposition(of: payload).map { ($0.type, false, $0.reason) }
    }

    /// 8 bits: the transform the caller publishes (the UI's `applyEdit`,
    /// MCP's `performGroupedEdit`) — `addingImageLayer(above:_:name:offset:)`
    /// at the landed placement's offset (`landing(…)`; a paste is MADE
    /// there, so its reference point stays (0, 0)), or an editable record's
    /// restore, then (Paste Into / Outside) the canvas-sized unlinked `mask`
    /// — and its outcome. `name` nil is the record's name for an editable
    /// paste and "Pasted Layer" otherwise. A typed payload u16 in this
    /// document's exact profile pastes its own `rgba8()`; any other is
    /// converted by the core into this document's profile under `profile`
    /// (`rgba8(in:profile:)`), so a pasteboard payload and a payload
    /// file take one path. An external image is decoded under `profile`
    /// (`ExternalClipboardImage.pixels`).
    static func byteTransform(_ plan: PastePlan, document: ImageDocument, anchor: Int,
                              landing spot: PasteLanding, name: String?, mask: PasteMask?,
                              profile: PasteProfile) throws
        -> (transform: (RasterDocument) throws -> RasterDocument?, outcome: PasteOutcome) {
        try requireMask(spot.placement, mask)
        guard let doc = document.doc, !doc.isHighDepth else {
            throw DoctoredCoreError(message: "The 8-bit paste requires an 8-bit document.")
        }
        let coverage = mask?.bytes()
        let outsideCoverage = mask?.outsideCoverage ?? 0
        /// The masked paste's last step: nil document stays nil, a refused
        /// mask throws.
        func masked(_ pasted: RasterDocument?, at landing: Int) throws -> RasterDocument? {
            guard let pasted, let coverage else { return pasted }
            guard let result = pasted.settingLayerMask(
                landing, coverage: coverage, width: pasted.width, height: pasted.height,
                x: 0, y: 0, linked: false, outsideCoverage: outsideCoverage) else {
                throw DoctoredCoreError(message: maskRefusal)
            }
            return result
        }
        let image: RasterImage
        let origin: (x: Int, y: Int)
        var converted = false, typed = false, narrowed = false, originDepth = 8
        var payloadPPI: (Double, Double)?
        var editable: (type: String, restored: Bool, reason: String?)?
        var conversion: ColorConversion?
        let destination = doc.iccProfile ?? RasterProfile.builtin(.sRGB)
        let profileOutcome: String
        let landing = LayerInsertPosition.above.landingIndex(in: doc, anchor: anchor)
        switch plan {
        case let .editable(payload, record):
            let name = name ?? record.name
            let (outcome, shift) = editableOutcome(payload, record, facade: doc, landing: landing,
                                                   spot: spot, name: name, mask: mask)
            let transform: (RasterDocument) throws -> RasterDocument? = { current in
                try masked(try? current.restoringEditable(record, anchor: anchor, position: .above,
                                                          shift: shift, name: name), at: landing)
            }
            return (transform, outcome)
        case .typed(let payload):
            typed = true
            editable = rasterEditable(payload, in: doc)
            originDepth = payload.originDepth
            narrowed = !payload.byteOrigin
            payloadPPI = (payload.ppi.x, payload.ppi.y)
            origin = payload.offset
            // #1 phase 4c: a CMYK payload converts into this profile (phase 4: a Gray one too;
            // Lab phase 4c: a Lab one too).
            profileOutcome = payload.isCmyk || payload.isGray || payload.isLab ? "converted"
                : profile.outcome(source: payload.profileData, destination: destination)
            conversion = ColorConversion.paste(payload, into: doc, mode: profile)
            let budget = try PrecisionWorkContext()
            if !payload.isCmyk, payload.storageDepth == 16, payload.profileData == doc.iccProfile {
                image = try payload.handle.rgba8(workingLimit: budget.limit,
                                                 otherRetained: budget.otherRetained)
            } else {
                image = try payload.handle.rgba8(in: destination, profile: profile,
                                                 workingLimit: budget.limit,
                                                 otherRetained: budget.otherRetained)
                converted = true
            }
        case .external(let pasted):
            conversion = ColorConversion.external(pasted, into: doc, mode: profile)
            if let cmyk = pasted.cmyk ?? pasted.gray {
                // #1 phase 4c: a CMYK TIFF converts through the core (phase 4: a gray one too).
                let budget = try PrecisionWorkContext()
                image = try cmyk.handle.rgba8(in: destination, profile: profile,
                                              workingLimit: budget.limit,
                                              otherRetained: budget.otherRetained)
                converted = true
                profileOutcome = "converted"
            } else {
                guard let pixels = pasted.pixels(into: doc, profile: profile) else {
                    throw DoctoredCoreError(message: "The clipboard's image could not be decoded.")
                }
                image = pixels
                profileOutcome = pasted.outcome(profile, destination: destination)
            }
            origin = (0, 0)
        case .refused(let reason):
            throw DoctoredCoreError(message: reason)
        }
        let name = name ?? defaultName
        let offset = spot.placement.offset(payload: origin, size: (image.width, image.height),
                                           canvas: (doc.width, doc.height), frame: spot.frame)
        let landed: Data
        do { landed = try doc.bytePayload(image, offset: offset).bytes } catch { landed = Data() }
        let outcome = PasteOutcome(
            landing: landing, placement: spot.placement, offset: offset, depth: 8, typed: typed,
            originDepth: originDepth,
            converted: converted, external: !typed, narrowed: narrowed, profile: profileOutcome,
            ppi: (payload: payloadPPI, document: (doc.resolution.x, doc.resolution.y)),
            landedBytes: landed, name: name, editable: editable, mask: mask,
            colorConversion: conversion)
        let transform: (RasterDocument) throws -> RasterDocument? = { current in
            guard let added = current.addingImageLayer(above: anchor, image, name: name,
                                                       offset: offset) else {
                return nil
            }
            return try masked(added, at: landing)
        }
        return (transform, outcome)
    }

    /// 16 bits and float: the typed document to publish through
    /// `PrecisionEditorCapture.publish`. A typed payload of another depth or
    /// profile is converted by the core under `profile` (a 16-bit payload
    /// promoted into float); `.external` pastes its exact ×257
    /// `bytePayload` of the image decoded under `profile`, which the core
    /// promotes into a float document; `.editable` restores its record.
    /// Paste Into / Outside then set the canvas-sized unlinked `mask`
    /// through the typed `set_mask` edit (exact u16 coverage). `name`
    /// follows `byteTransform`'s rule; `landing` is `landing(…)`'s answer.
    static func typedDocument(_ plan: PastePlan, capture: PrecisionEditorCapture, anchor: Int,
                              position: LayerInsertPosition, landing spot: PasteLanding,
                              name: String?, profile: PasteProfile,
                              mask: PasteMask?) throws
        -> (RasterPrecisionDocument, PasteOutcome) {
        try requireMask(spot.placement, mask)
        let facade = capture.facade
        // #1 Indexed phase 2d: the one layer holds no editable record, so one pastes as its
        // pixels.
        if facade.isIndexed, case let .editable(t, _) = plan {
            return try typedDocument(.typed(t), capture: capture, anchor: anchor,
                                     position: position, landing: spot, name: name,
                                     profile: profile, mask: mask)
        }
        /// The masked paste's last step over the pasted typed document.
        func masked(_ pasted: RasterPrecisionDocument, at landing: Int) throws
            -> RasterPrecisionDocument {
            guard let mask else { return pasted }
            let width = facade.width, height = facade.height
            let edit = try PrecisionPixelEdit(
                pasted,
                command: .setMask(layer: landing, width: width, height: height, linked: false,
                                  outsideCoverage: Int(mask.outsideCoverage)),
                input: mask.samples(width: width, height: height))
            guard let result = try edit.run(title: mask.historyName) else {
                throw DoctoredCoreError(message: maskRefusal)
            }
            return result
        }
        let payload: RasterClipboardPayload
        let origin: (x: Int, y: Int), size: (width: Int, height: Int)
        var typed = true, external = false, converted = false, originDepth = 8
        var payloadPPI: (Double, Double)?
        var storageDepth = depth(of: facade)
        var editable: (type: String, restored: Bool, reason: String?)?
        var conversion: ColorConversion?
        let destination = facade.iccProfile ?? RasterProfile.builtin(.sRGB)
        let profileOutcome: String
        switch plan {
        case let .editable(t, record):
            let name = name ?? record.name
            let (outcome, shift) = editableOutcome(
                t, record, facade: facade,
                landing: position.landingIndex(in: facade, anchor: anchor),
                spot: spot, name: name, mask: mask)
            guard let restored = try facade.restoringEditable(
                record, anchor: anchor, position: position, shift: shift, name: name),
                let result = restored.precision else {
                throw DoctoredCoreError(message: "The editable record could not be restored.")
            }
            return (try masked(result, at: outcome.landing), outcome)
        case .typed(let t):
            payload = t.handle
            origin = t.offset; size = t.size; originDepth = t.originDepth
            storageDepth = t.storageDepth; payloadPPI = (t.ppi.x, t.ppi.y)
            editable = rasterEditable(t, in: facade)
            // #1 phase 4c: across modes the core always converts; between
            // CMYK documents the CMYK profiles decide.
            conversion = ColorConversion.paste(t, into: facade, mode: profile)
            if t.isCmyk != facade.isCmyk || t.isGray != facade.isGray || t.isLab != facade.isLab {
                converted = true
                profileOutcome = "converted"
            } else if t.isCmyk || t.isGray || t.isLab {
                // Lab phase 4c: Lab → Lab is one twin, so its codes are kept.
                let own = ColorConversion.profileData(of: facade)
                converted = t.profileData != own && profile == .convert
                profileOutcome = profile.outcome(source: t.profileData, destination: own)
            } else {
                converted = t.storageDepth != depth(of: facade) || t.profileData != facade.iccProfile
                profileOutcome = profile.outcome(source: t.profileData, destination: destination)
            }
        case .external(let image)
            where facade.isCmyk || image.cmyk != nil || facade.isGray || image.gray != nil
                || facade.isLab:
            // #1 phase 4c: a CMYK destination converts another app's image
            // from its own space through the core, and a CMYK TIFF pastes as
            // its CMYK payload at every destination; phase 4: a Gray
            // destination and a gray TIFF the same way; Lab phase 4c: a Lab
            // destination too (the core's RGB → Lab).
            payload = try image.cmykDestinationPayload(into: facade)
            origin = (0, 0); size = image.size
            typed = false; external = true
            payloadPPI = nil
            conversion = ColorConversion.external(image, into: facade, mode: profile)
            converted = conversion != nil
            if let cmyk = image.cmyk, facade.isCmyk {
                let own = ColorConversion.profileData(of: facade)
                profileOutcome = profile.outcome(source: cmyk.profileData, destination: own)
            } else if let gray = image.gray, facade.isGray {
                let own = ColorConversion.profileData(of: facade)
                profileOutcome = profile.outcome(source: gray.profileData, destination: own)
            } else {
                profileOutcome = "converted"
            }
        case .external(let image):
            guard let pixels = image.pixels(into: facade, profile: profile) else {
                throw DoctoredCoreError(message: "The clipboard's image could not be decoded.")
            }
            payload = try facade.bytePayload(pixels, offset: (0, 0))
            origin = (0, 0); size = image.size
            typed = false; external = true
            payloadPPI = nil
            profileOutcome = image.outcome(profile, destination: destination)
            conversion = ColorConversion.external(image, into: facade, mode: profile)
        case .refused(let reason):
            throw DoctoredCoreError(message: reason)
        }
        let name = name ?? defaultName
        // #1 Indexed phase 2d: the paste lands IN the one layer (index 0), its mask applied by
        // the same core call, so `masked` is skipped.
        var indexedPlanBytes: UInt64?
        let pasted: RasterDocument
        if facade.isIndexed {
            let indexed = try facade.indexedPasting(
                payload, placement: spot.placement, frame: spot.frame, name: name,
                profile: profile, mask: mask)
            pasted = indexed.document
            indexedPlanBytes = indexed.planBytes
        } else {
            pasted = try facade.clipboardPasting(
                payload, anchor: anchor, position: position, placement: spot.placement,
                frame: spot.frame, name: name, profile: profile)
        }
        guard let result = pasted.precision else {
            throw DoctoredCoreError(message: "The paste produced no typed document.")
        }
        let landing = facade.isIndexed ? 0 : position.landingIndex(in: facade, anchor: anchor)
        var outcome = PasteOutcome(
            landing: landing, placement: spot.placement,
            offset: spot.placement.offset(payload: origin, size: size,
                                          canvas: (facade.width, facade.height),
                                          frame: spot.frame),
            depth: depth(of: facade), typed: typed, originDepth: originDepth,
            converted: converted, external: external,
            narrowed: storageDepth > depth(of: facade), profile: profileOutcome,
            ppi: (payload: payloadPPI, document: (facade.resolution.x, facade.resolution.y)),
            landedBytes: payload.bytes, name: name, editable: editable, mask: mask,
            colorConversion: conversion)
        if let indexedPlanBytes {
            outcome.indexedPlanBytes = indexedPlanBytes
            return (result, outcome)
        }
        return (try masked(result, at: landing), outcome)
    }
}

/// Where a SAVED-PAYLOAD paste step's payload lives, so the step can name it
/// as a file (`payload_path`) and `package_action` can carry it as a `$asset`.
/// Only MCP `paste` with `payload_path` or `record_payload`
/// records one; every other recorded paste is the live step
/// (`ActionSteps.paste(payloadPath: nil, …)`) and writes nothing here.
enum ClipboardPayloadStore {
    static let environmentName = "DR_CLIPBOARD_PAYLOADS_DIR"

    static var directory: URL {
        if let path = ProcessInfo.processInfo.environment[environmentName], !path.isEmpty {
            return URL(fileURLWithPath: path, isDirectory: true)
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                               in: .userDomainMask)[0]
        return support.appendingPathComponent("Doctored/Clipboard Payloads", isDirectory: true)
    }

    /// `.notACommand` unless a recording is running; otherwise the landed
    /// payload is written atomically as `<uuid>.drcb` and the step names it.
    /// A payload that could not be built (an 8-bit document whose profile
    /// has no matrix/TRC model) records the visible placeholder instead.
    /// The step carries `outcome.name`, the RESOLVED name, so a replay
    /// never renames a restored layer. `placement` is the REQUEST: an `auto`
    /// or `at` paste, a Paste in Place of an external image, which
    /// lands like Paste, and a Paste in Place that landed
    /// `relativeInPlace`, record `at` with the landed `outcome.offset`,
    /// so a replay never depends on the replaying view or clipboard source;
    /// every other request records itself. `profile` is recorded only when it is
    /// `.preserve` (the Don't Convert choice).
    static func pasteStep(_ outcome: PasteOutcome, placement: PastePlacement,
                          profile: PasteProfile, note: String) throws -> [ActionStep] {
        guard ActionRecorder.shared.isRecording else { return .notACommand }
        guard !outcome.landedBytes.isEmpty else { return .unrecorded("Paste") }
        let folder = directory
        let url = folder.appendingPathComponent(
            UUID().uuidString + "." + SideFileExtension.clipboardPayload.current)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try DurableFile.write(outcome.landedBytes, to: url)
        } catch {
            throw DoctoredCoreError(message: "Could not record the pasted payload at \(url.path): "
                + error.localizedDescription)
        }
        let explicit = placement == .auto || placement == .at
            || (placement == .inPlace && (outcome.external
                                          || outcome.placement == .relativeInPlace))
        return .paste(payloadPath: url.path, placement: explicit ? .at : placement,
                      offset: explicit ? outcome.offset : nil, name: outcome.name,
                      profile: profile, note: note)
    }
}

/// File ▸ New from Clipboard at the payload's own depth: a byte-origin
/// payload makes an 8-bit document of its exact bytes, a 16-bit or float
/// payload a typed document of its depth, profile, size and PPI (a float
/// payload's tone map carried), and an external image today's 8-bit
/// document. No sheet: the window's own mode readout is the disclosure.
///
/// A typed payload is first TRIMMED to its content box (every pixel
/// with alpha > 0; always on, no argument) — a fully
/// transparent payload, such as an adjustment layer's copy, keeps its whole
/// window, and an external image keeps its size. Then a payload carrying an
/// editable record is rebuilt around it (`restoringEditableDocument`):
/// a text or shape copy opens as that editable layer, an adjustment copy as
/// the adjustment above the transparent "Background", and a record that
/// cannot restore leaves the pixel document with its reason. No undo step
/// is added and a fallback shows no alert: `Made.editable` discloses it.
enum NewFromClipboard {
    enum Outcome {
        case typed(depth: Int, profile: String)
        case byteOrigin
        case external
        /// A held layer clipboard (`LayerClipboard.Entry.newDocument`).
        case layers(records: Int, depth: Int, profile: String)
    }

    /// New from Clipboard sizes a layers copy to the copied layers'
    /// content box (paste-layers-470c2).
    static let layersTrimToContent = true

    /// The new document's attributes for a layers copy; a nil field is the
    /// clip's own value, and a nil `base` puts no layer under the pasted ones.
    struct LayersAttributes {
        let depth: Int?
        let profile: NewDocumentProfile?
        let ppi: Double?
        let base: NewDocumentBackground?
    }

    /// Measured: the source's depth, profile and PPI, over a white
    /// Background.
    static let layersAttributes = LayersAttributes(depth: nil, profile: nil, ppi: nil,
                                                   base: .white)

    /// What `make` built: the document, how its pixels arrived, the
    /// editable record's disposition (nil when the payload carried none),
    /// whether the payload was trimmed, and — typed only — where the new
    /// canvas's origin sat on the source canvas.
    struct Made {
        let document: ImageDocument
        let outcome: Outcome
        let editable: (type: String, restored: Bool, reason: String?)?
        let trimmed: Bool
        let sourceOffset: (x: Int, y: Int)?
    }

    static var isAvailable: Bool {
        TypedClipboardPayload.isAvailable
            || Clipboard.pasteboard.canReadObject(forClasses: [NSImage.self], options: nil)
    }

    /// nil when the pasteboard holds no image at all. A held layer clipboard
    /// is asked first: the copied layers land as layers.
    static func make(pasteboard: NSPasteboard) throws -> Made? {
        if let entry = LayerClipboard.shared.current(on: pasteboard) {
            let made = try entry.newDocument()
            return Made(document: made.document,
                        outcome: .layers(records: entry.layers.layerCount,
                                         depth: LayerPastePlan.depth(of: entry.layers),
                                         profile: entry.layers.profileName),
                        editable: nil, trimmed: made.trimmed, sourceOffset: made.sourceOffset)
        }
        if let read = try TypedClipboardPayload.read(pasteboard) {
            let trimmedPayload = try read.trimmed()
            let typed = trimmedPayload ?? read
            let trimmed = trimmedPayload != nil
            let budget = try PrecisionWorkContext()
            if typed.byteOrigin {
                let image = try typed.handle.rgba8(workingLimit: budget.limit,
                                                   otherRetained: budget.otherRetained)
                guard let document = ImageDocument.makeUntitled(
                    with: image, profile: typed.profileData) else {
                    throw DoctoredCoreError(message: "A document could not be made from the clipboard.")
                }
                if let resized = try document.doc.settingResolution(x: typed.ppi.x, y: typed.ppi.y) {
                    document.doc = resized
                }
                let restore = document.doc.restoringEditableDocument(typed)
                document.doc = restore.document
                document.activeLayerIndex = restore.active
                document.refreshProjection()
                if restore.editable?.restored == true {
                    document.fileType = ImageDocument.nativeTypeIdentifier
                }
                return Made(document: document, outcome: .byteOrigin, editable: restore.editable,
                            trimmed: trimmed, sourceOffset: typed.offset)
            }
            let precision = try RasterPrecisionDocument.fromClipboard(
                typed.handle, workingLimit: budget.limit, otherRetained: budget.otherRetained)
            let restore = try RasterDocument(precision: precision).restoringEditableDocument(typed)
            let document = ImageDocument()
            document.doc = restore.document
            document.activeLayerIndex = restore.active
            document.refreshProjection()
            document.fileType = ImageDocument.nativeTypeIdentifier
            document.countEditChange(.changeDone)
            return Made(document: document,
                        outcome: .typed(depth: typed.storageDepth, profile: typed.profileDescription),
                        editable: restore.editable, trimmed: trimmed, sourceOffset: typed.offset)
        }
        guard let pasted = RasterImage.fromPasteboard(pasteboard) else { return nil }
        guard let document = ImageDocument.makeUntitled(
            with: pasted.image, profile: pasted.profile, profileRefusal: pasted.refusal)
        else { return nil }
        return Made(document: document, outcome: .external, editable: nil, trimmed: false,
                    sourceOffset: nil)
    }

    /// The menu / Welcome window action: beeps on an empty clipboard and
    /// explains any refusal. A record that fell back to pixels is not an
    /// error — the document opens holding them.
    static func open() {
        do {
            guard let made = try make(pasteboard: Clipboard.pasteboard) else {
                NSSound.beep()
                return
            }
            NSDocumentController.shared.addDocument(made.document)
            made.document.makeWindowControllers()
            made.document.showWindows()
        } catch {
            let alert = NSAlert()
            alert.messageText = "Could Not Create the Document"
            alert.informativeText = (error as? DoctoredCoreError)?.message ?? error.localizedDescription
            alert.runModal()
        }
    }
}
