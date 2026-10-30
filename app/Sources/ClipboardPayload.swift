import AppKit

// `Clipboard.pasteboard` is `NSPasteboard(name:)` when `DR_PASTEBOARD_NAME` is set (a drive's
// private pasteboard, released at quit by `Clipboard.releasePrivatePasteboard`) and `.general`
// otherwise, and every image Copy/Cut/Paste — plus `RasterImage.fromPasteboard`'s default,
// `ExternalClipboardImage.read` and New from Clipboard's validation — goes through it; text
// pasteboard writes keep `.general`. Copy/Copy Merged/Cut write the core's RZCB payload under
// `Clipboard.typedType` beside the public TIFF/PNG rendering (`publicRendering`: u16 preview, float
// tone map) in ONE item through `Clipboard.writeItem` (`HeldClipboardPayload.swift`) — at 8 bits
// too, as a byte-origin payload (`RasterDocument.bytePayload`,
// `typedBytes(forByteCopy:offset:layer:)`: nil, never a beep, when it cannot be built).
// `TypedClipboardPayload.read` is the one `data(forType:)` call (validation asks `isAvailable`, a
// type query).

/// Phase 12G-1: the TYPED clipboard as the host speaks it.
///
/// A Doctored→Doctored Copy at 16 bits or 32-bit float writes two things
/// into ONE pasteboard item: the private `typedType` — the core's RZCB
/// payload (`precision_clipboard`), which carries samples, alpha, profile,
/// offset, coverage and PPI bit for bit — and the public TIFF/PNG, which is a
/// RENDERING for every other app. Since 12G-2 an 8-bit Copy writes a
/// byte-origin payload too, and every Paste is planned first
/// (`ClipboardBoundaries.swift`): another depth or profile, or an
/// external image, converts into the destination (promotion into 32-bit float
/// included), with the "Ask When Pasting" preference bringing back the Paste
/// Profile Mismatch choice. Every `dr_*` call behind this lives in
/// `DoctoredCore+Clipboard.swift`.
enum Clipboard {
    /// The private pasteboard type of the RZCB payload.
    static let typedType = NSPasteboard.PasteboardType("com.kgalligan.doctored.typed-clipboard")

    /// The type a build before the Rasterize-to-Doctored rename wrote; read, never written.
    static let legacyTypedType = NSPasteboard.PasteboardType("com.kgalligan.rasterize.typed-clipboard")

    /// Every type a typed payload is read from, the current one first.
    static let readableTypedTypes = [typedType, legacyTypedType]

    /// A verification drive's private pasteboard: when set and non-empty,
    /// every image read and write goes to `NSPasteboard(name:)` instead of
    /// the user's general pasteboard.
    static let environmentName = "DR_PASTEBOARD_NAME"

    /// The pasteboard every image Copy/Cut/Paste uses.
    static var pasteboard: NSPasteboard {
        if let name = ProcessInfo.processInfo.environment[environmentName], !name.isEmpty {
            return NSPasteboard(name: NSPasteboard.Name(name))
        }
        return .general
    }

    /// Phase 12G-2: a drive's private pasteboard is released when the app
    /// quits, so a run leaves no named pasteboard behind. The user's general
    /// pasteboard is never released.
    static func releasePrivatePasteboard() {
        guard let name = ProcessInfo.processInfo.environment[environmentName], !name.isEmpty
        else { return }
        NSPasteboard(name: NSPasteboard.Name(name)).releaseGlobally()
    }
}

/// Where a pasted typed record sits: centred on the canvas, at the canvas
/// offset the payload was copied from (Edit ▸ Paste in Place), or centred on
/// the selection's bounds under a mask built from it (Edit ▸ Paste Special ▸
/// Paste Into / Paste Outside). Edit ▸ Paste (⌘V) requests
/// `auto` (`PasteContext.landing`), which lands as `selection` (centred
/// on the destination selection, unmasked), `view` (centred on the visible
/// canvas) or `centered` — never at its source position.
/// Paste in Place (`inPlace`) into a canvas of another size
/// than the copy's lands as `relativeInPlace` (`PasteContext.inPlace`);
/// `at` is an explicit position, and what a recorded `auto` paste replays.
/// The raw value is the MCP/Action spelling; the core only ever receives
/// `coreName`.
enum PastePlacement: String {
    /// Request only: Edit ▸ Paste's rule; never reaches the core or a reply.
    case auto
    case centered
    case inPlace = "in_place"
    case into
    case outside
    /// Landed only: centred on the destination selection's frame, unmasked.
    case selection
    /// Landed only: centred on the visible canvas frame.
    case view
    /// Landed only: Paste in Place across canvas sizes, at
    /// `ClipboardSource.relativeOffset`; the frame is that position and the
    /// payload's size.
    case relativeInPlace = "relative_in_place"
    /// Exactly at (x, y): the frame is (x, y, width, height).
    case at

    /// Paste Into and Paste Outside: the pasted layer gets a mask from the
    /// selection (`PasteMask`), and the frame is the selection's bounds.
    var masksSelection: Bool { self == .into || self == .outside }

    /// The placements the core receives as `into` plus a frame.
    var usesFrame: Bool {
        switch self {
        case .into, .outside, .selection, .view, .relativeInPlace, .at: return true
        case .auto, .centered, .inPlace: return false
        }
    }

    /// The core's placement: every framed case is its `into` (a frame).
    var coreName: String { usesFrame ? "into" : rawValue }

    /// Where a `size` window lands on a `canvas`: centred by the core's
    /// `div_euclid` formula (`precision_clipboard::clipboard_paste`) — at
    /// EVERY depth, the 8-bit paste included — at the payload's own offset,
    /// or, framed, by the same formula on `frame` (the canvas when nil).
    func offset(payload: (x: Int, y: Int), size: (width: Int, height: Int),
                canvas: (width: Int, height: Int), frame: CGRect? = nil) -> (x: Int, y: Int) {
        func half(_ v: Int) -> Int { v >= 0 ? v / 2 : -((-v + 1) / 2) }
        switch self {
        case .inPlace: return payload
        case .centered, .auto:
            return (half(canvas.width - size.width), half(canvas.height - size.height))
        case .into, .outside, .selection, .view, .relativeInPlace, .at:
            let rect = frame ?? CGRect(x: 0, y: 0, width: canvas.width, height: canvas.height)
            return (Int(rect.minX) + half(Int(rect.width) - size.width),
                    Int(rect.minY) + half(Int(rect.height) - size.height))
        }
    }
}

/// A typed payload read off the pasteboard, with its description.
struct TypedClipboardPayload {
    let handle: RasterClipboardPayload
    let info: [String: Any]

    /// nil when the private type is absent. The ONLY `data(forType:)` read of
    /// the typed blob: validation asks `isAvailable` instead. A payload
    /// held in process (`HeldClipboardPayload`) is read first; its staleness
    /// check always runs, so an inline payload from another writer cannot
    /// leave a stale held entry charged.
    static func read(_ pasteboard: NSPasteboard) throws -> TypedClipboardPayload? {
        if let held = HeldClipboardPayload.shared.current(on: pasteboard) {
            // The held bytes are already registered as retained.
            let budget = try PrecisionWorkContext()
            return try decode(held, budget: budget, otherRetained: budget.otherRetained)
        }
        guard let data = pasteboard.data(forType: Clipboard.typedType)
                ?? pasteboard.data(forType: Clipboard.legacyTypedType) else { return nil }
        let budget = try PrecisionWorkContext()
        // The pasteboard `Data` stays alive through the decode, so it is
        // charged alongside everything else already retained.
        let other = try PrecisionDocumentDescriptor.sum(budget.otherRetained, UInt64(data.count))
        return try decode(data, budget: budget, otherRetained: other)
    }

    private static func decode(_ data: Data, budget: PrecisionWorkContext,
                               otherRetained: UInt64) throws -> TypedClipboardPayload {
        let handle = try RasterClipboardPayload.decode(
            data, workingLimit: budget.limit, otherRetained: otherRetained)
        return TypedClipboardPayload(handle: handle, info: try handle.info())
    }

    /// A TYPE query, never a data read — this runs on every Edit-menu open
    /// and every ⌘V / ⇧⌘V key equivalent. The held check runs first and is
    /// never short-circuited, so it drops an entry whose token is gone.
    static var isAvailable: Bool {
        let pasteboard = Clipboard.pasteboard
        let held = HeldClipboardPayload.shared.isAvailable(on: pasteboard)
        return held || pasteboard.availableType(from: Clipboard.readableTypedTypes) != nil
    }

    /// Phase 12G-2: an RZCB file (MCP `payload_path`, a recorded paste's
    /// `$asset`) read the way the pasteboard's blob is.
    static func read(file url: URL) throws -> TypedClipboardPayload {
        let data: Data
        do { data = try Data(contentsOf: url) } catch {
            throw DoctoredCoreError(message: "Could not read the clipboard payload at \(url.path): "
                + error.localizedDescription)
        }
        let budget = try PrecisionWorkContext()
        let other = try PrecisionDocumentDescriptor.sum(budget.otherRetained, UInt64(data.count))
        let handle = try RasterClipboardPayload.decode(
            data, workingLimit: budget.limit, otherRetained: other)
        return TypedClipboardPayload(handle: handle, info: try handle.info())
    }
}

/// Phase 12G-2: the payload's header, read off the core's `info` JSON.
extension TypedClipboardPayload {
    private func int(_ key: String) -> Int { (info[key] as? NSNumber)?.intValue ?? 0 }
    private func pair(_ key: String) -> [Double] {
        (info[key] as? [Any])?.compactMap { ($0 as? NSNumber)?.doubleValue } ?? []
    }

    /// 16 or 32: the storage the samples are in.
    var storageDepth: Int { int("depth") }
    /// 8 for a byte-origin payload, otherwise the storage depth (a CMYK
    /// payload's declared depth, `cmykDepth`).
    var originDepth: Int { (info["origin_depth"] as? NSNumber)?.intValue ?? storageDepth }
    /// Exactly `v · 257` of 8-bit RGB pixels. Never a CMYK payload, whose
    /// 8-bit ink is the declared depth of a CMYK document (#547 phase 4c).
    var byteOrigin: Bool { !isCmyk && originDepth == 8 }
    /// #547 phase 4c: a CMYK payload (RZCB v4) — straight CMYKA16 ink in its
    /// CMYK profile (`profileData`), which only a CMYK document keeps as ink.
    var isCmyk: Bool { info["mode"] as? String == "cmyk" }
    /// A CMYK payload's declared depth (8 or 16); nil for an RGB payload.
    var cmykDepth: Int? { isCmyk ? int("cmyk_depth") : nil }
    /// #547 phase 4: a Grayscale payload (RZCB v5) — R = G = B in its Gray profile's twin, with
    /// the Gray ICC (`profileData`), which only a Gray document keeps as gray codes.
    var isGray: Bool { info["mode"] as? String == "gray" }
    /// A Gray payload's declared depth (8, 16 or 32); nil for every other payload.
    var grayDepth: Int? { isGray ? int("gray_depth") : nil }
    /// #547 Lab phase 4c: a Lab Color payload (RZCB v6) — the L*a*b* codes in the Lab twin
    /// (`profileData`), which only a Lab document keeps as codes.
    var isLab: Bool { info["mode"] as? String == "lab" }
    /// A Lab payload's declared depth (8 or 16); nil for every other payload.
    var labDepth: Int? { isLab ? int("lab_depth") : nil }
    var ppi: (x: Double, y: Double) {
        let v = pair("ppi")
        return v.count == 2 ? (v[0], v[1]) : (72, 72)
    }
    var offset: (x: Int, y: Int) {
        let v = pair("offset")
        return v.count == 2 ? (Int(v[0]), Int(v[1])) : (0, 0)
    }
    var size: (width: Int, height: Int) { (int("width"), int("height")) }
    /// The exact ICC bytes the samples are in: the RGB profile, or a CMYK
    /// payload's CMYK profile, or a Gray payload's Gray profile
    /// (`RasterClipboardPayload.profileData`).
    var profileData: Data { handle.profileData }
    /// The profile's name; a Gray payload's is its Gray profile's.
    var profileDescription: String {
        (isGray ? info["gray_profile"] : info["profile_description"]) as? String ?? ""
    }
    /// A float payload's carried tone map, as the core's JSON; nil at u16.
    var toneMapJSON: String? {
        guard let map = info["tone_map"],
              let data = try? JSONSerialization.data(withJSONObject: map, options: [.sortedKeys])
        else { return nil }
        return String(data: data, encoding: .utf8)
    }
    /// Phase 12G-3: the core's `info["editable"]` object — type, name?,
    /// anchor?, record_bytes — or nil when the payload carries no section.
    var editable: [String: Any]? { info["editable"] as? [String: Any] }
}

extension TypedClipboardPayload {
    /// New from Clipboard's payload trimmed to its content box
    /// (`RasterClipboardPayload.trim`); nil when there is nothing to trim.
    func trimmed() throws -> TypedClipboardPayload? {
        let budget = try PrecisionWorkContext()
        guard let handle = try handle.trim(workingLimit: budget.limit,
                                           otherRetained: budget.otherRetained) else { return nil }
        return TypedClipboardPayload(handle: handle, info: try handle.info())
    }
}

extension RasterDocument {
    /// The copy window: the selection's integral bounds clipped to the
    /// canvas (the byte path's crop), or the whole canvas; nil when the
    /// selection lies entirely off the canvas.
    func clipboardWindow(_ selection: CanvasSelection?) -> CGRect? {
        let canvas = CGRect(x: 0, y: 0, width: width, height: height)
        guard let selection else { return canvas }
        let window = selection.bounds.integral.intersection(canvas)
        return window.isNull || window.isEmpty ? nil : window
    }

    private static func requestText(_ object: [String: Any]) throws -> String {
        let data = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
        guard let text = String(data: data, encoding: .utf8) else {
            throw DoctoredCoreError(message: "Could not encode the clipboard request.")
        }
        return text
    }

    private static func windowFields(_ window: CGRect) -> [String: Any] {
        ["x": Int(window.minX), "y": Int(window.minY),
         "width": Int(window.width), "height": Int(window.height)]
    }

    private func requirePrecision() throws -> RasterPrecisionDocument {
        guard let precision else {
            throw DoctoredCoreError(message: "The typed clipboard requires a 16-bit or float document.")
        }
        return precision
    }

    /// Edit ▸ Copy at depth: layer `layer`'s RAW samples in `window`, alpha
    /// multiplied by the selection's u16 coverage.
    func clipboardCopy(layer: Int, window: CGRect,
                       selection: CanvasSelection?) throws -> RasterClipboardPayload {
        let precision = try requirePrecision()
        let budget = try PrecisionWorkContext(source: precision)
        var fields = Self.windowFields(window)
        fields["op"] = "copy"
        fields["layer"] = layer
        return try precision.clipboardCopy(
            request: Self.requestText(fields), coverage: selection?.maskSamples16() ?? [],
            workingLimit: budget.limit, otherRetained: budget.otherRetained, control: nil)
    }

    /// Edit ▸ Copy Merged at depth: the projection in `window`.
    func clipboardCopyMerged(window: CGRect,
                             selection: CanvasSelection?) throws -> RasterClipboardPayload {
        let precision = try requirePrecision()
        let budget = try PrecisionWorkContext(source: precision)
        var fields = Self.windowFields(window)
        fields["op"] = "copy_merged"
        return try precision.clipboardCopy(
            request: Self.requestText(fields), coverage: selection?.maskSamples16() ?? [],
            workingLimit: budget.limit, otherRetained: budget.otherRetained, control: nil)
    }

    /// Edit ▸ Cut at 16 bits: the payload, and the erased document — nil
    /// when the erase changed no byte (a transparency lock). On an Indexed
    /// document `indexedBackground` is the colour a cut pixel's entry is
    /// nearest with no transparent index (#547 Indexed phase 3b); nil is white.
    func clipboardCut(layer: Int, window: CGRect, selection: CanvasSelection?,
                      indexedBackground: NSColor? = nil) throws
        -> (RasterClipboardPayload, RasterDocument?) {
        let precision = try requirePrecision()
        let budget = try PrecisionWorkContext(source: precision)
        var fields = Self.windowFields(window)
        fields["op"] = "cut"
        fields["layer"] = layer
        fields.merge(try IndexedDocument.backgroundFields(indexedBackground, in: self)) { $1 }
        let result = try precision.clipboardCut(
            request: Self.requestText(fields), coverage: selection?.maskSamples16() ?? [],
            workingLimit: budget.limit, otherRetained: budget.otherRetained, control: nil)
        return (result.payload, try result.document.map { try RasterDocument(precision: $0) })
    }

    /// Paste / Paste in Place at depth: one new raster record at the slot
    /// `position` names relative to `anchor`. A payload of another depth or
    /// profile is converted into this document's (a 16-bit payload is
    /// promoted into float); `profile` chooses whether another profile's
    /// pixels keep their appearance (`.convert`) or their numbers
    /// (`.preserve`, "Don't Convert"). A framed placement (Paste Into / Outside, and the landed
    /// `selection`, `view` and `at`) passes its `frame` (required by, and
    /// only valid with, a framed placement); `auto` must be resolved first.
    func clipboardPasting(_ payload: RasterClipboardPayload, anchor: Int,
                          position: LayerInsertPosition, placement: PastePlacement,
                          frame: CGRect? = nil,
                          name: String, profile: PasteProfile) throws -> RasterDocument {
        guard placement != .auto else {
            throw DoctoredCoreError(message: "Paste's auto placement must be resolved before the "
                + "paste.")
        }
        guard placement.usesFrame == (frame != nil) else {
            throw DoctoredCoreError(message: "A paste frame is required by, and only valid with, "
                + "a framed placement.")
        }
        let precision = try requirePrecision()
        let budget = try PrecisionWorkContext(source: precision)
        var fields: [String: Any] = [
            "name": name, "anchor": anchor, "position": position.rawValue,
            "placement": placement.coreName, "profile": profile.rawValue,
        ]
        if let frame {
            fields["frame"] = [Int(frame.minX), Int(frame.minY),
                               Int(frame.width), Int(frame.height)]
        }
        let request = try Self.requestText(fields)
        return try RasterDocument(precision: precision.clipboardPasting(
            payload, request: request, workingLimit: budget.limit,
            otherRetained: budget.otherRetained, control: nil))
    }

    /// #547 Indexed phase 2d: Paste and Paste Special on an Indexed document — the payload merged
    /// into the one layer and re-indexed, masked by `mask` (Paste Into / Outside) in the same core
    /// call (`dr_precision_doc_indexed_paste`). Answers the document and its plan's
    /// `scratch_bytes`, which the reply reports as `working_memory_plan_bytes`.
    func indexedPasting(_ payload: RasterClipboardPayload, placement: PastePlacement,
                        frame: CGRect?, name: String, profile: PasteProfile,
                        mask: PasteMask?) throws -> (document: RasterDocument, planBytes: UInt64) {
        guard placement != .auto else {
            throw DoctoredCoreError(message: "Paste's auto placement must be resolved before the "
                + "paste.")
        }
        guard placement.usesFrame == (frame != nil) else {
            throw DoctoredCoreError(message: "A paste frame is required by, and only valid with, "
                + "a framed placement.")
        }
        let precision = try requirePrecision()
        let budget = try PrecisionWorkContext(source: precision)
        var fields: [String: Any] = [
            "name": name, "anchor": 0, "position": LayerInsertPosition.above.rawValue,
            "placement": placement.coreName, "profile": profile.rawValue,
        ]
        if let frame {
            fields["frame"] = [Int(frame.minX), Int(frame.minY),
                               Int(frame.width), Int(frame.height)]
        }
        let request = try Self.requestText(fields)
        let plan = try precision.indexedPastePlan(payload, request: request)
        let planBytes = (plan["scratch_bytes"] as? NSNumber)?.uint64Value ?? 0
        let coverage = try mask?.samples(width: width, height: height).samples ?? []
        let pasted = try precision.indexedPasting(
            payload, request: request, coverage: coverage,
            outsideCoverage: mask?.outsideCoverage ?? 0, workingLimit: budget.limit,
            otherRetained: budget.otherRetained, control: nil)
        return (try RasterDocument(precision: pasted), planBytes)
    }

    /// Phase 12G-2: `image` (straight RGBA8 in this document's profile) as
    /// a byte-origin payload at canvas `offset`, carrying this document's
    /// profile bytes and PPI — what an 8-bit Copy writes beside its public
    /// TIFF/PNG, and how an external image is promoted into 16 bits.
    func bytePayload(_ image: RasterImage, offset: (x: Int, y: Int)) throws -> RasterClipboardPayload {
        guard let profile = iccProfile, !profile.isEmpty else {
            throw DoctoredCoreError(message: "The document's colour profile could not be read.")
        }
        let budget = try PrecisionWorkContext()
        return try RasterClipboardPayload.fromRGBA8(
            image, profile: profile, offset: offset, ppi: resolution,
            workingLimit: budget.limit, otherRetained: budget.otherRetained)
    }

    /// The public TIFF/PNG's pixels: the ordinary u16 preview, or the float
    /// document's own tone map (`display(of:context:)`, the ONE place a typed
    /// projection becomes display bytes — `to_rgba8` refuses float). A
    /// byte-origin payload (12G-3: an adjustment layer's transparent copy,
    /// also in a float document) renders as its exact `rgba8()`.
    func publicRendering(of payload: RasterClipboardPayload) throws -> RasterImage {
        let precision = try requirePrecision()
        let context = try PrecisionWorkContext(source: precision)
        let info = try payload.info()
        // #547 phase 4c: a CMYK payload's rendering is its ink through the
        // CMYK profile into this (CMYK) document's view space, Display P3.
        if info["mode"] as? String == "cmyk" {
            return try payload.rgba8(in: iccProfile, workingLimit: context.limit,
                                     otherRetained: context.otherRetained)
        }
        if (info["origin_depth"] as? NSNumber)?.intValue == 8 {
            return try payload.rgba8(workingLimit: context.limit,
                                     otherRetained: context.otherRetained)
        }
        guard let image = payload.image() else {
            throw DoctoredCoreError(message: "The clipboard payload has no pixels.")
        }
        return try display(of: image, context: context)
    }
}
