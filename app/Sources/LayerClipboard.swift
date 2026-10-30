import AppKit

// `LayerClipboard.copy` builds the clip with ONE `duplicatingToNewDocument`, writes ONE pasteboard
// item (the flat item — one root's whole-layer Copy, several roots' Copy Merged of the clip — plus
// a UUID under `LayerClipboard.tokenType` and the source's `ClipboardSource`, through
// `Clipboard.writeItem(_:typed:layerToken:source:)`) and holds the entry in process;
// `current(on:)` drops an entry the pasteboard no longer names, `isAvailable(on:)` is the
// validation's type query.
//
// `LayerPastePlan` holds the policy questions (nothing is refused for depth or profile at any
// depth; `resolve` and its float refusals are GONE): `profileMismatch` (the Ask When
// Pasting alert) and `preserveRefusal` (Don't Convert into a profile that fails the Assign check
// for the copied text/shape or calibrated layers, worded about the copied layers and asked BEFORE
// the alert, which then omits the button; MCP `profile: "preserve"` throws it; always nil for a
// float destination). `LayerPasteWork` converts the clip's depth (`narrowedStructureToU8/U16`,
// `promotedStructure`) and, under `.convert`, its profile (`convertingToProfile`); Don't Convert
// takes no host step — the
// core's paste RELABELS the clip, keeping every layer's numbers and its RAW
// layers bound (`LayerPasteOutcome.profile`) — for a 32-bit float
// document it only promotes an 8-bit clip to 16 bits and takes NO host profile step, because the
// core's paste promotes into float under the `profile` `pastingLayers` now takes, returning
// `(document, report)`; `LayerPasteOutcome.report` puts the core's `rasterized`
// / `blend_reset` / `effect_blend_reset` in the MCP reply (empty arrays for every other paste), and a copied layer the
// float mode cannot hold is the core's refusal sentence, shown as any paste error — computes the
// placement offset (`offset(of:placement:canvas:context:)`: `PasteLanding.swift`'s rule over the
// roots' union `layerBounds`, returning the landed placement) and makes ONE `pastingLayers` call —
// 8 bits through `applyEdit` / `performGroupedEdit`, typed through `capture.publish`, recorded as
// the live `paste` step (the pixel paste's own; MCP refuses `record_payload` for
// layers), the pasted roots then selected. Paste Into / Outside of a held entry is
// refused (`maskedPasteRefusal`, measured against Photoshop) and `Entry.newDocument` is New from
// Clipboard's document for copied layers.

/// Copy and Paste of LAYERS between documents.
///
/// With layers selected and NO pixel selection, Edit ▸ Copy copies the
/// selected entries — each with its whole subtree and every property — into
/// a LAYER CLIPBOARD: the document `duplicatingToNewDocument` builds from
/// them in one core call, held here in process. Edit ▸ Paste lands every
/// record above the active layer with the roots' union CONTENT box centred
/// on a destination selection, otherwise on the visible canvas — never at
/// the source coordinates (measured against Photoshop,
/// `PasteContext.landing`); Paste in Place keeps their source canvas
/// coordinates on a canvas of the source's size and otherwise moves the box
/// to the same relative location (`ClipboardSource.relativeOffset`). Either
/// keeps the destination selection and is ONE core call
/// (`RasterDocument.pastingLayers`) and so one undo step.
///
/// The pasteboard still carries a flat image for other apps and MCP's
/// `pixels` paste (Paste Into / Outside of copied layers are refused, and New
/// from Clipboard lands the layers themselves): one root's
/// whole-layer Copy (the raw layer pixels or a group's raw projection, with
/// its editable record as today's Copy attaches it), or the clip's Copy
/// Merged when several roots are copied. Beside it a private TOKEN type names
/// the held entry, so a later copy by anything else makes the entry stale.
///
/// Depth and profile follow the paste policy (`LayerPastePlan`): into 8
/// and 16 bits anything pastes, its depth always converted (narrowed, or
/// promoted exactly ×257) and its profile converted silently — or, with "Ask
/// When Pasting" on, converted or assigned ("Don't Convert") as the Paste
/// Profile Mismatch alert chooses. Into 32-bit float anything pastes too
/// (measured against Photoshop): the CORE promotes the copied
/// layers into linear light under the same Convert / Don't Convert choice,
/// keeps a RAW layer bound to its source (re-rendered from its
/// development into linear light under Convert), keeps text and shape layers
/// described (their colour numbers are read in linear light,
/// rewritten so they look the same; under Don't Convert only in the
/// document's own profile), lands any warp as pixels, and refuses by name a
/// copied layer the float mode cannot hold (`LayerPasteOutcome.report` is
/// what it changed).
///
/// What travels (measured against Photoshop): the copied
/// layers bring no layer comp, alpha channel, guide or color sampler, the
/// destination keeps its own and its global light (a pasted style that uses
/// the global light casts by the destination's), and at 8 and 16 bits a
/// linked RAW layer stays linked, its file missing or not.
final class LayerClipboard {
    static let shared = LayerClipboard()

    /// The private pasteboard type naming the held entry.
    static let tokenType = NSPasteboard.PasteboardType("com.kgalligan.doctored.layer-clipboard")

    /// One copy: the pasteboard token, the clip document and the source
    /// roots it was copied from.
    struct Entry {
        let token: String
        let layers: RasterDocument
        let sourceRoots: [Int]
    }

    private var entry: Entry?

    private init() {}

    /// Validation only: a TYPE query plus the held entry, never a data read.
    func isAvailable(on pasteboard: NSPasteboard) -> Bool {
        entry != nil && pasteboard.availableType(from: [Self.tokenType]) != nil
    }

    /// The held entry when the pasteboard still names it; a stale entry
    /// (something else was copied since) is dropped.
    func current(on pasteboard: NSPasteboard) -> Entry? {
        guard let held = entry else { return nil }
        guard pasteboard.string(forType: Self.tokenType) == held.token else {
            entry = nil
            return nil
        }
        return held
    }

    /// Edit ▸ Copy with no pixel selection: `indices` (subtrees included)
    /// copied into the layer clipboard, and ONE pasteboard item written —
    /// the token, the typed payload and the public TIFF/PNG of the flat item.
    static func copy(_ indices: [Int], from document: ImageDocument) throws -> LayerCopyOutcome {
        guard let doc = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        guard let clip = try doc.duplicatingToNewDocument(indices) else {
            throw DoctoredCoreError(message: "The selected layers could not be copied.")
        }
        let sourceRoots = doc.layerTree.independentRoots(indices)
        let roots = clip.layerTree.topLevel()
        let canvas = CGRect(x: 0, y: 0, width: clip.width, height: clip.height)
        let rendering: RasterImage
        var payload: RasterClipboardPayload?
        let flat: String
        // #547 phase 4c: a CMYK clip whose one root is an adjustment record
        // (no ink of its own, no editable record) writes the clip's Copy
        // Merged, and a CMYK root's copy carries no editable record.
        let cmykAdjustment = clip.isCmyk && roots.count == 1
            && roots.first.map { clip.layerIsAdjustment($0) } == true
        if roots.count == 1, let root = roots.first, !cmykAdjustment {
            flat = "copy"
            // Today's whole-layer Copy of that one root (`copyPrecision` /
            // `typedBytes(forByteCopy:offset:layer:)`).
            if clip.isCmyk {
                // Since #547's artwork batch a CMYK type or shape layer's copy
                // carries its record (`cmykAdjustment` excluded it above).
                let copied = try clip.attachingEditable(
                    clip.clipboardCopy(layer: root, window: canvas, selection: nil), layer: root)
                rendering = try clip.publicRendering(of: copied)
                payload = copied
            } else if clip.isHighDepth {
                var copied = clip.layerIsAdjustment(root)
                    ? try clip.transparentPayload()
                    : try clip.clipboardCopy(layer: root, window: canvas, selection: nil)
                copied = try clip.attachingEditable(copied, layer: root)
                rendering = try clip.publicRendering(of: copied)
                payload = copied
            } else {
                guard let image = clip.byteCopySource(layer: root, whole: true) else {
                    throw DoctoredCoreError(message: "The copied layer's pixels could not be read.")
                }
                rendering = image
                // Public types only when the byte-origin payload cannot be
                // built — `typedBytes(forByteCopy:offset:layer:)`'s rule.
                do {
                    payload = try clip.attachingEditable(
                        clip.bytePayload(image, offset: (0, 0)), layer: root)
                } catch { payload = nil }
            }
        } else {
            flat = "copy_merged"
            if clip.isHighDepth {
                let merged = try clip.clipboardCopyMerged(window: canvas, selection: nil)
                rendering = try clip.publicRendering(of: merged)
                payload = merged
            } else {
                let image = try document.byteComposite(of: clip)
                rendering = image
                payload = try? clip.bytePayload(image, offset: (0, 0))
            }
        }
        guard let cgImage = rendering.makeCGImage(in: clip.colorSpace) else {
            throw DoctoredCoreError(message: "The clipboard could not be written.")
        }
        let token = UUID().uuidString
        let typed = payload?.bytes
        try Clipboard.writeItem(cgImage, typed: typed, layerToken: token,
                                source: ClipboardSource(document: document, canvas: doc))
        shared.entry = Entry(token: token, layers: clip, sourceRoots: sourceRoots)
        let editable = (try? payload?.info())?["editable"] as? [String: Any]
        return LayerCopyOutcome(
            sourceRoots: sourceRoots, records: clip.layerCount, depth: LayerPastePlan.depth(of: clip),
            width: clip.width, height: clip.height, typedBytes: typed?.count ?? 0,
            editable: editable?["type"] as? String, flat: flat)
    }

    /// `get_clipboard`'s `layers`: null, or the held entry's description.
    func json(on pasteboard: NSPasteboard) -> Any {
        guard let held = current(on: pasteboard) else { return NSNull() }
        let clip = held.layers
        return [
            "records": clip.layerCount, "depth": LayerPastePlan.depth(of: clip),
            "profile_description": clip.profileName, "width": clip.width, "height": clip.height,
        ] as [String: Any]
    }

    /// The sentence the MCP `copy` description appends.
    static let copyCatalogSentence = "mode \"auto\" decides as Edit ▸ Copy does, from this "
        + "document's own state: with a pixel selection it is pixels of the given layer "
        + "(default the active layer); with none it is layers, of the given layers (default "
        + "the SELECTED layers). A recorded Edit ▸ Copy is this step, so a replay re-decides "
        + "on the replaying document. "
        + "mode \"layers\" ignores any pixel selection and copies "
        + "the given layers (layers, or layer/index; default the active layer), each with its "
        + "whole subtree and every property, to an in-process layer clipboard that paste "
        + "restores; the pasteboard still receives a flat image — one root's whole-layer copy, "
        + "or the copied layers' merged composite when several roots are copied (flat "
        + "\"copy\" or \"copy_merged\"). Layers reply: ok, mode, source_layers, records, depth, "
        + "width, height, typed, typed_bytes, public_types, editable, flat."

    /// `copy`'s `mode: "auto"` resolved against `document`,
    /// exactly as Edit ▸ Copy decides (`EditorViewController.copy(_:)`): no
    /// pixel selection → `"layers"`, the SELECTED layers when the call names
    /// none; a selection → `"pixels"` (the active layer when none is named).
    /// The capability gate (`DocumentPrecision`) and the handler both read
    /// this, so they ask about the same mode. Every other call is returned
    /// unchanged.
    static func resolvingAutoCopy(tool: String, _ arguments: [String: Any],
                                  in document: ImageDocument) -> [String: Any] {
        guard tool == "copy", arguments["mode"] as? String == "auto" else { return arguments }
        var resolved = arguments
        if document.canvasSelection == nil {
            resolved["mode"] = "layers"
            if arguments["layers"] == nil, arguments["layer"] == nil, arguments["index"] == nil {
                resolved["layers"] = document.selectedLayerIndices
            }
        } else {
            resolved["mode"] = "pixels"
        }
        return resolved
    }

    /// The sentence the MCP `paste` description appends.
    static let pasteCatalogSentence = "mode \"auto\" (default) pastes the layer clipboard when "
        + "the clipboard still holds a layers copy and pixels otherwise; \"layers\" requires one, "
        + "\"pixels\" ignores it. A layers paste lands every copied layer with its structure, "
        + "masks, blend modes, styles, clipping and adjustment layers as ONE undo step: "
        + "auto (default) centres the pasted roots' combined content box on a selection or "
        + "the view (placement selection or view; centered with no window) and never keeps "
        + "its source position; centered centres that box on the canvas; "
        + "in_place keeps their source canvas coordinates on a canvas of the source's size "
        + "and otherwise moves the box to the same relative location (placement "
        + "relative_in_place); a layers paste keeps the selection; "
        + "into, outside, at, x, y, payload_path, record_payload and name are pixel-paste "
        + "arguments and are refused. While a layers copy is held, placement into or outside "
        + "with mode auto is refused; mode \"pixels\" pastes their flat image "
        + "there. Into 8 and "
        + "16 bits any copy pastes (but one holding a 32-bit float Camera RAW development, "
        + "refused by name until it is redeveloped with raw.output_encoding sdr16 or "
        + "rasterized), its depth "
        + "converted (narrowed, or promoted exactly ×257) and its profile converted (profile \"convert\", the default; "
        + "RAW layers re-render from their sources and keep them) "
        + "or kept (\"preserve\", Don't Convert: every layer keeps its numbers and RAW "
        + "layers keep their sources; refused by name only when the document's "
        + "profile fails the Assign check for the copied text/shape or calibrated layers). "
        + "Into a 32-bit float document any copy pastes "
        + "too, promoted into linear light under the same profile choice: a RAW layer keeps "
        + "its source (re-rendered from its development into linear light under "
        + "\"convert\", its numbers kept under \"preserve\"), text and shape layers stay "
        + "editable (their colours rewritten into linear light so they look the same; under "
        + "\"preserve\" only when the copied layer is in the document's own profile, else they "
        + "land as their pixels), while warps land as their pixels (rasterized), a Pass Through "
        + "group lands Pass Through, and the blend modes reset at 32 bits land Normal "
        + "(blend_reset), layer styles and Blend If ride with an outer_glow, inner_glow or bevel "
        + "highlight in Screen landing linear_dodge (effect_blend_reset); a "
        + "copied layer the float mode cannot hold (an effect blend mode outside the float set, "
        + "another unavailable blend mode, an adjustment the float mode cannot evaluate "
        + "(Hue/Saturation, a Color Lookup other than a 3D table within 0 to 1, and "
        + "the others refused at 32 bits)) refuses the "
        + "whole paste, naming the layer and the feature. "
        + "The copied layers bring no layer comp, alpha channel, guide or color sampler, the "
        + "document keeps its own and its global light (a pasted style that uses the global "
        + "light casts by this document's), and at 8 and 16 bits a linked RAW layer stays "
        + "linked, its file missing or not. Layers reply: ok, mode, "
        + "placement, offset {x, y}, layers (the pasted roots), layer, records, depth, "
        + "origin_depth, converted, narrowed, profile_converted, profile (same, converted or "
        + "preserved), rasterized [{layer, reason}], blend_reset [{layer, "
        + "from}], effect_blend_reset [{layer, effect, from, to}] (all three empty unless a float "
        + "document promoted the copy), "
        + "ppi {clipboard, document, match}, history."

    /// Paste Into and Paste Outside are unavailable while
    /// the clipboard holds copied LAYERS. The UI disables the items
    /// (`validatePasteInto`); MCP `paste` in mode auto throws this.
    static func maskedPasteRefusal(_ placement: PastePlacement) -> String {
        let command = placement == .outside ? "Paste Outside" : "Paste Into"
        return "“\(command)” is unavailable while the clipboard holds copied layers: "
            + "layers do not paste into or outside a selection. "
            + "Pass mode \"pixels\" to paste their flat image."
    }

    /// The sentence the MCP `get_clipboard` description appends.
    static let clipboardCatalogSentence = "layers: null, or the held layer clipboard's {records, "
        + "depth, profile_description, width, height} while the clipboard still names it."
}

extension LayerClipboard.Entry {
    /// File ▸ New from Clipboard (measured against Photoshop)
    /// with copied LAYERS makes a document the size of the copied layers'
    /// content box at the source's depth, profile and PPI, with a white
    /// Background under the layers, each landed by name. The clip is cropped
    /// to the roots' union `layerBounds` ∩ canvas
    /// when `NewFromClipboard.layersTrimToContent` (an empty union stays
    /// untrimmed). Every `layersAttributes` field nil → `makeDuplicate`'s
    /// shape over that clip; otherwise `makeNew` at the clip's size, each nil
    /// field taken from the clip (a float clip's depth too, since New
    /// Document makes float documents), the clip landed
    /// by `LayerPasteWork` `.inPlace` and makeNew's layer removed when `base`
    /// is nil. No history step; the topmost root is active.
    func newDocument() throws -> (document: ImageDocument, sourceOffset: (x: Int, y: Int),
                                  trimmed: Bool) {
        // #547 phase 4c: a CMYK clip opens as itself, untrimmed (the trim is
        // a crop, which a CMYK document does not take yet).
        if layers.isCmyk {
            let document = try ImageDocument.makeDuplicate(
                of: layers, layers: layers.layerTree.topLevel(), name: nil)
            return (document, (0, 0), false)
        }
        var clip = layers
        var origin = (x: 0, y: 0)
        var trimmed = false
        if NewFromClipboard.layersTrimToContent {
            let canvas = CGRect(x: 0, y: 0, width: clip.width, height: clip.height)
            let union = clip.layerTree.topLevel().compactMap { clip.layerBounds($0) }
                .map { CGRect(x: $0.x, y: $0.y, width: $0.width, height: $0.height) }
                .reduce(CGRect.null) { $0.union($1) }
                .intersection(canvas)
            if !union.isNull, !union.isEmpty, union != canvas {
                // `cropped` refits a copied fill layer to the trimmed clip.
                guard let cut = try clip.cropped(x: Int(union.minX), y: Int(union.minY),
                                                 w: Int(union.width), h: Int(union.height)) else {
                    throw DoctoredCoreError(message: "The copied layers could not be trimmed.")
                }
                clip = cut
                origin = (Int(union.minX), Int(union.minY))
                trimmed = true
            }
        }
        // #547 Lab phase 4c: a Lab clip, trimmed as above, opens as itself (New Document makes
        // no Lab document, so the `makeNew` base path is not taken).
        if clip.isLab {
            let document = try ImageDocument.makeDuplicate(
                of: clip, layers: clip.layerTree.topLevel(), name: nil)
            return (document, origin, trimmed)
        }
        let attributes = NewFromClipboard.layersAttributes
        if attributes.depth == nil, attributes.profile == nil, attributes.ppi == nil,
           attributes.base == nil {
            let document = try ImageDocument.makeDuplicate(
                of: clip, layers: clip.layerTree.topLevel(), name: nil)
            return (document, origin, trimmed)
        }
        let resolution = clip.resolution
        let request = try NewDocumentRequest(
            width: clip.width, height: clip.height, ppi: attributes.ppi ?? resolution.x,
            profile: attributes.profile ?? .srgb,
            depth: attributes.depth ?? LayerPastePlan.depth(of: clip),
            background: attributes.base ?? .transparent)
        let document = try ImageDocument.makeNew(request)
        guard var doc = document.doc else {
            throw DoctoredCoreError(message: "A document could not be made from the clipboard.")
        }
        if attributes.profile == nil, let bytes = clip.iccProfile {
            doc = try doc.assigningProfile(bytes) ?? doc
        }
        if attributes.ppi == nil, resolution.y != resolution.x {
            doc = try doc.settingResolution(x: resolution.x, y: resolution.y) ?? doc
        }
        document.doc = doc
        let entry = LayerClipboard.Entry(token: token, layers: clip, sourceRoots: sourceRoots)
        let context = PasteContext(selection: nil, quickMask: false, in: doc, visible: nil,
                                   origin: nil)
        let outcome: LayerPasteOutcome
        if doc.isHighDepth {
            let capture = try PrecisionEditorCapture(document)
            let (pasted, work) = try LayerPasteWork.typedDocument(
                entry, capture: capture, anchor: 0, position: .above, placement: .inPlace,
                profile: .convert, context: context)
            doc = try RasterDocument(precision: pasted)
            outcome = work
        } else {
            let (transform, work) = try LayerPasteWork.byteTransform(
                entry, document: document, anchor: 0, position: .above, placement: .inPlace,
                profile: .convert, context: context)
            guard let pasted = try transform(doc) else {
                throw DoctoredCoreError(message: "The copied layers could not be placed.")
            }
            doc = pasted
            outcome = work
        }
        var active = outcome.topRoot
        if attributes.base == nil, let removed = try doc.removeLayers([0]) {
            doc = removed
            active -= 1
        }
        document.doc = doc
        document.activeLayerIndex = max(0, active)
        document.refreshProjection()
        // Layers need the native format, as `makeDuplicate`'s document has.
        document.fileType = ImageDocument.nativeTypeIdentifier
        return (document, origin, trimmed)
    }
}

/// What a layers Copy did — the MCP reply.
struct LayerCopyOutcome {
    let sourceRoots: [Int]
    let records: Int
    let depth: Int
    let width: Int
    let height: Int
    let typedBytes: Int
    let editable: String?
    let flat: String

    var json: [String: Any] {
        let pasteboard = Clipboard.pasteboard
        var publicTypes: [String] = []
        if pasteboard.availableType(from: [.tiff]) != nil { publicTypes.append("tiff") }
        if pasteboard.availableType(from: [.png]) != nil { publicTypes.append("png") }
        return [
            "ok": true, "mode": "layers", "source_layers": sourceRoots, "records": records,
            "depth": depth, "width": width, "height": height, "typed": typedBytes > 0,
            "typed_bytes": typedBytes, "public_types": publicTypes,
            "editable": editable.map { $0 as Any } ?? NSNull(), "flat": flat,
            "typed_store": Clipboard.typedStore(on: pasteboard) ?? NSNull(),
        ]
    }
}

/// The layers paste's policy questions, asked before any edit — `PastePlan`'s
/// twin for the layer clipboard. Depth always converts and a
/// profile mismatch converts (or, chosen, assigns) at every depth, 32-bit
/// float included: nothing is refused here for depth or profile.
enum LayerPastePlan {
    static func depth(of doc: RasterDocument) -> Int {
        doc.isFloat ? 32 : doc.isHighDepth ? 16 : 8
    }

    static func depthPhrase(_ depth: Int) -> String {
        depth == 32 ? "32-bit float" : "\(depth)-bit"
    }

    /// The Paste Profile Mismatch alert's two names when the clip's profile
    /// `PasteProfile.differ`s from the document's (an untagged document
    /// counts as sRGB); nil otherwise.
    /// #547 phase 4c: a clip of the other colour mode always converts, so
    /// it never asks; between CMYK documents the CMYK profiles are compared.
    static func profileMismatch(_ entry: LayerClipboard.Entry,
                                into doc: RasterDocument) -> (source: String, destination: String)? {
        let clip = entry.layers
        // #547 phase 4: so does a clip into or out of Grayscale; between Gray documents the Gray
        // profiles are compared. Lab phase 4c: and Lab Color (one twin, so Lab → Lab never asks).
        guard clip.isCmyk == doc.isCmyk, clip.isGray == doc.isGray, clip.isLab == doc.isLab
        else { return nil }
        let source = ColorConversion.profileData(of: clip)
        let destination = ColorConversion.profileData(of: doc)
        guard PasteProfile.differ(source, destination) else { return nil }
        return (ColorConversion.profileName(of: clip), RasterProfile.inspect(destination).name)
    }

    /// Why "Don't Convert" (the core's relabel of the clip under the
    /// document's profile) is unavailable, worded about the
    /// copied layers: the Assign check of this profile refuses for the
    /// copied text/shape artwork or calibrated layers
    /// (`assignProfileRefusal`, the same check the core's relabel makes).
    /// RAW layers do not refuse: the relabel keeps
    /// them bound. nil when it is available. Asked BEFORE the alert, so the
    /// alert omits the button and says why, and an MCP `profile: "preserve"`
    /// refuses with it. Always nil for a 32-bit float destination: there the
    /// core's promotion takes Don't Convert itself.
    static func preserveRefusal(_ entry: LayerClipboard.Entry,
                                into doc: RasterDocument) -> String? {
        // #547 phase 4c: between CMYK documents Don't Convert keeps the ink; phase 4: between
        // Gray documents it keeps the gray codes.
        if doc.isFloat || (doc.isCmyk && entry.layers.isCmyk)
            || (doc.isGray && entry.layers.isGray) { return nil }
        // #547 Lab phase 4c: between Lab documents it keeps the codes; across modes it refuses.
        if doc.isLab || entry.layers.isLab {
            return doc.isLab == entry.layers.isLab ? nil : ColorConversion.crossModePreserveLab
        }
        let destination = doc.iccProfile ?? RasterProfile.builtin(.sRGB)
        return entry.layers.assignProfileRefusal(destination).map {
            "Don't Convert is unavailable: " + $0
        }
    }
}

/// What a layers Paste did — the MCP reply and the history row.
struct LayerPasteOutcome {
    let placement: PastePlacement
    let offset: (x: Int, y: Int)
    /// The landed block's first index; the block is `landing ..< landing + records`.
    let landing: Int
    let records: Int
    /// The landed block's independent roots, bottom-first: the clip's
    /// top-level entries shifted by `landing`.
    let roots: [Int]
    let depth: Int
    let originDepth: Int
    let narrowed: Bool
    let promoted: Bool
    let profileConverted: Bool
    /// "same", "converted" or "preserved" (the document's profile
    /// assigned to the clip, Don't Convert).
    let profile: String
    /// What a promotion into a 32-bit float document changed —
    /// `rasterized`, `blend_reset`, `effect_blend_reset`
    /// (`pastingLayers`' report; empty arrays for every other
    /// paste).
    let report: [String: Any]
    let ppi: (clipboard: (Double, Double), document: (Double, Double))
    /// #547 phase 4c: the colour model or CMYK profile the layers crossed;
    /// nil when they kept their model.
    var colorConversion: ColorConversion?

    var converted: Bool { narrowed || promoted || profileConverted }
    var ppiMatches: Bool {
        ppi.clipboard.0 == ppi.document.0 && ppi.clipboard.1 == ppi.document.1
    }
    /// The topmost pasted root — the layer the paste leaves active.
    var topRoot: Int { roots.last ?? landing }

    /// "Paste Layers", with " (CMYK → RGB)" across colour modes (#547 phase
    /// 4c) and " (300 → 72 ppi)" on a PPI mismatch.
    var historyName: String {
        let base = "Paste Layers" + (colorConversion?.historySuffix ?? "")
        guard !ppiMatches else { return base }
        func text(_ v: Double) -> String { String(format: "%g", v) }
        return "\(base) (\(text(ppi.clipboard.0)) → \(text(ppi.document.0)) ppi)"
    }

    var json: [String: Any] {
        [
            "ok": true, "mode": "layers", "placement": placement.rawValue,
            "offset": ["x": offset.x, "y": offset.y], "layers": roots, "layer": topRoot,
            "records": records, "depth": depth, "origin_depth": originDepth,
            "converted": converted, "narrowed": narrowed, "profile_converted": profileConverted,
            "profile": profile,
            "rasterized": report["rasterized"] ?? [[String: Any]](),
            "blend_reset": report["blend_reset"] ?? [[String: Any]](),
            "effect_blend_reset": report["effect_blend_reset"] ?? [[String: Any]](),
            "color_conversion": ColorConversion.json(colorConversion),
            "ppi": ["clipboard": [ppi.clipboard.0, ppi.clipboard.1],
                    "document": [ppi.document.0, ppi.document.1],
                    "match": ppiMatches] as [String: Any],
            "history": historyName,
        ]
    }
}

/// Runs a resolved layers paste: the clip converted under the plan, the
/// placement offset, then ONE `pastingLayers`.
enum LayerPasteWork {
    /// The offset and the LANDED placement over U, the union of
    /// `clip.layerBounds` over `clip.layerTree.topLevel()`; nil U →
    /// ((0, 0), `.inPlace`). `.auto` lands by `context.landing` (pasted
    /// layers centre on the selection or the view and never keep
    /// their source position); `.inPlace` → (0, 0) on a canvas
    /// of the clip's size, and otherwise U moved to
    /// `ClipboardSource.relativeOffset` (`.relativeInPlace`, measured as for
    /// pixels); `.centered` → `PastePlacement.centered.offset(U) − U's
    /// origin`; a frame placement → the frame-centred offset of U − U's
    /// origin. Into, outside and at throw.
    static func offset(of clip: RasterDocument, placement: PastePlacement,
                       canvas: (width: Int, height: Int), context: PasteContext) throws
        -> (offset: (x: Int, y: Int), placement: PastePlacement) {
        switch placement {
        case .into, .outside:
            throw DoctoredCoreError(message: "Paste Into and Paste Outside paste pixels; pass "
                + "mode \"pixels\".")
        case .at:
            throw DoctoredCoreError(message: "placement at pastes pixels; pass mode \"pixels\".")
        case .selection, .view, .relativeInPlace:
            throw DoctoredCoreError(message: "placement \(placement.rawValue) is a landed "
                + "placement, not a request.")
        case .auto, .centered, .inPlace:
            break
        }
        let boxes = clip.layerTree.topLevel().compactMap { clip.layerBounds($0) }
        guard let first = boxes.first else { return ((0, 0), .inPlace) }
        var minX = first.x, minY = first.y
        var maxX = first.x + first.width, maxY = first.y + first.height
        for box in boxes.dropFirst() {
            minX = min(minX, box.x); minY = min(minY, box.y)
            maxX = max(maxX, box.x + box.width); maxY = max(maxY, box.y + box.height)
        }
        let size = (width: maxX - minX, height: maxY - minY)
        var landed = PasteLanding(placement: placement, frame: nil)
        if placement == .auto {
            landed = context.landing
        } else if placement == .inPlace,
                  clip.width != canvas.width || clip.height != canvas.height {
            let at = ClipboardSource.relativeOffset(of: (minX, minY), size: size,
                                                    from: (clip.width, clip.height),
                                                    into: canvas)
            return ((at.x - minX, at.y - minY), .relativeInPlace)
        }
        switch landed.placement {
        case .inPlace:
            return ((0, 0), .inPlace)
        default:
            let at = landed.placement.offset(payload: (minX, minY), size: size, canvas: canvas,
                                             frame: landed.frame)
            return ((at.x - minX, at.y - minY), landed.placement)
        }
    }

    /// The clip at `doc`'s depth, what was done to it, and `core`, the
    /// `PasteProfile` handed to `pastingLayers`. Depth always converts.
    /// Another profile under `.convert` is converted here through
    /// `convertingToProfile`, and `core` is `.convert`; a conversion that
    /// answers nil is an identity (two spellings of one model), so the clip
    /// keeps its bytes and `core` is `.preserve`. Under `.preserve` (Don't
    /// Convert) it first refuses with
    /// `LayerPastePlan.preserveRefusal` and takes no host step: `core` is
    /// `.preserve` and the core RELABELS the clip, keeping every layer's
    /// numbers and its RAW layers bound. Conversion refusals (blocked
    /// rows) surface verbatim. A 32-bit float `doc` takes no
    /// host profile step: an 8-bit clip is promoted to 16 bits here and the
    /// CORE's paste promotes every typed clip into float under `mode`
    /// (`core`), so `promoted`, `profileConverted` and `profile` only
    /// describe what it will do. `profile` is `PasteProfile.outcome`.
    private static func converted(_ entry: LayerClipboard.Entry, into doc: RasterDocument,
                                  profile mode: PasteProfile) throws
        -> (clip: RasterDocument, narrowed: Bool, promoted: Bool, profileConverted: Bool,
            profile: String, core: PasteProfile, conversion: ColorConversion?) {
        // #547 Lab phase 4c: a Lab side first (Lab → Gray and Lab → CMYK cross through RGB,
        // `LabClipboard.swift`); phase 4: a Grayscale side before the CMYK one (Gray → CMYK
        // crosses through RGB first).
        if doc.isLab || entry.layers.isLab {
            return try labConverted(entry, into: doc, profile: mode)
        }
        if doc.isGray || entry.layers.isGray {
            return try grayConverted(entry, into: doc, profile: mode)
        }
        if doc.isCmyk || entry.layers.isCmyk {
            return try cmykConverted(entry, into: doc, profile: mode)
        }
        var clip = entry.layers
        var narrowed = false, promoted = false, profileConverted = false
        let target = LayerPastePlan.depth(of: doc)
        if let precision = clip.precision, LayerPastePlan.depth(of: clip) > target {
            let budget = try PrecisionWorkContext(source: precision)
            clip = target == 8
                ? try precision.narrowedStructureToU8(
                    dither: 0, workingLimit: budget.limit, otherRetained: budget.otherRetained)
                : try RasterDocument(precision: precision.narrowedStructureToU16(
                    workingLimit: budget.limit, otherRetained: budget.otherRetained))
            narrowed = true
        } else if !clip.isHighDepth, target >= 16 {
            clip = try RasterDocument(precision: clip.promotedStructure())
            promoted = true
        }
        // An untagged document counts as sRGB, as `profileMismatch` has it.
        let profile = doc.iccProfile ?? RasterProfile.builtin(.sRGB)
        let outcome = mode.outcome(source: entry.layers.iccProfile
                                       ?? RasterProfile.builtin(.sRGB),
                                   destination: profile)
        var core = PasteProfile.convert
        if target == 32 {
            promoted = LayerPastePlan.depth(of: entry.layers) < 32
            profileConverted = outcome == "converted"
            core = mode
        } else if clip.iccProfile != profile {
            switch mode {
            case .convert:
                let result = try clip.convertingToProfile(profile, options: .default)?.document
                if let result {
                    clip = result
                    profileConverted = true
                } else {
                    core = .preserve
                }
            case .preserve:
                if let refusal = LayerPastePlan.preserveRefusal(entry, into: doc) {
                    throw DoctoredCoreError(message: refusal)
                }
                core = .preserve
            }
        }
        return (clip, narrowed, promoted, profileConverted, outcome, core, nil)
    }

    /// #547 phase 4c: `converted`'s CMYK arms. A clip of the other colour
    /// mode is brought into the document's here (`CmykLayerClip.converted`:
    /// the mode conversions, relative colorimetric with BPC through the
    /// profiles) and the core splices it as it is; between CMYK documents
    /// the core converts the ink into this profile under `.convert` and
    /// keeps it under `.preserve`, so nothing happens here.
    private static func cmykConverted(_ entry: LayerClipboard.Entry, into doc: RasterDocument,
                                      profile mode: PasteProfile) throws
        -> (clip: RasterDocument, narrowed: Bool, promoted: Bool, profileConverted: Bool,
            profile: String, core: PasteProfile, conversion: ColorConversion?) {
        let clip = entry.layers
        if let crossed = try CmykLayerClip.converted(clip, into: doc, mode: mode) {
            let narrowed = clip.isFloat || (clip.isCmyk && !doc.isHighDepth)
            let promoted = !clip.isHighDepth || (clip.isCmyk && doc.isFloat)
            return (crossed.clip, narrowed, promoted, true, "converted", .convert,
                    crossed.conversion)
        }
        let source = ColorConversion.profileData(of: clip)
        let destination = ColorConversion.profileData(of: doc)
        let outcome = mode.outcome(source: source, destination: destination)
        return (clip, false, false, outcome == "converted", outcome, mode,
                ColorConversion.layers(clip, into: doc, mode: mode))
    }

    /// #547 phase 4: `converted`'s Grayscale arms. A clip of another mode (or, under `.convert`,
    /// of another Gray profile) is brought into the document's here (`GrayLayerClip.converted`)
    /// and the core splices it as it is; between Gray documents of one profile, or under
    /// `.preserve`, the core keeps the codes.
    private static func grayConverted(_ entry: LayerClipboard.Entry, into doc: RasterDocument,
                                      profile mode: PasteProfile) throws
        -> (clip: RasterDocument, narrowed: Bool, promoted: Bool, profileConverted: Bool,
            profile: String, core: PasteProfile, conversion: ColorConversion?) {
        let clip = entry.layers
        let source = ColorConversion.profileData(of: clip)
        let destination = ColorConversion.profileData(of: doc)
        if let crossed = try GrayLayerClip.converted(clip, into: doc, mode: mode) {
            let from = LayerPastePlan.depth(of: clip), to = LayerPastePlan.depth(of: doc)
            return (crossed.clip, from > to, from < to, true, "converted", .convert,
                    crossed.conversion)
        }
        let outcome = mode.outcome(source: source, destination: destination)
        // A 32-bit Gray clip into an 8 or 16-bit Gray document is narrowed first, in its own
        // profile, which the core then keeps or relabels under `mode`.
        let fitted = try GrayLayerClip.depthFitted(clip, into: doc)
        return (fitted ?? clip, fitted != nil, false, outcome == "converted", outcome, mode,
                ColorConversion.layers(clip, into: doc, mode: mode))
    }

    private static func outcome(_ entry: LayerClipboard.Entry, clip: RasterDocument,
                                into doc: RasterDocument, anchor: Int,
                                position: LayerInsertPosition, placement: PastePlacement,
                                offset: (x: Int, y: Int), narrowed: Bool, promoted: Bool,
                                profileConverted: Bool, profile: String,
                                conversion: ColorConversion? = nil,
                                report: [String: Any] = RasterDocument.emptyLayerPasteReport)
        -> LayerPasteOutcome {
        let landing = position.landingIndex(in: doc, anchor: anchor)
        let source = entry.layers.resolution
        return LayerPasteOutcome(
            placement: placement, offset: offset, landing: landing, records: clip.layerCount,
            roots: clip.layerTree.topLevel().map { landing + $0 },
            depth: LayerPastePlan.depth(of: doc), originDepth: LayerPastePlan.depth(of: entry.layers),
            narrowed: narrowed, promoted: promoted, profileConverted: profileConverted,
            profile: profile, report: report,
            ppi: (clipboard: (source.x, source.y), document: (doc.resolution.x, doc.resolution.y)),
            colorConversion: conversion)
    }

    /// 8 bits: the transform the caller publishes (the UI's `applyEdit`,
    /// MCP's `performGroupedEdit`) and its outcome. The clip is narrowed,
    /// and converted here or relabelled by the core under `profile`.
    static func byteTransform(_ entry: LayerClipboard.Entry, document: ImageDocument, anchor: Int,
                              position: LayerInsertPosition, placement: PastePlacement,
                              profile: PasteProfile, context: PasteContext) throws
        -> (transform: (RasterDocument) throws -> RasterDocument?, outcome: LayerPasteOutcome) {
        guard let doc = document.doc, !doc.isHighDepth else {
            throw DoctoredCoreError(message: "The 8-bit layers paste requires an 8-bit document.")
        }
        let work = try converted(entry, into: doc, profile: profile)
        let (offset, landed) = try offset(of: work.clip, placement: placement,
                                          canvas: (doc.width, doc.height), context: context)
        let result = outcome(entry, clip: work.clip, into: doc, anchor: anchor, position: position,
                             placement: landed, offset: offset, narrowed: work.narrowed,
                             promoted: work.promoted, profileConverted: work.profileConverted,
                             profile: work.profile, conversion: work.conversion)
        let clip = work.clip, core = work.core
        // A pasted fill layer is re-rendered to cover THIS canvas.
        let transform: (RasterDocument) throws -> RasterDocument? = { current in
            try current.pastingLayers(clip, anchor: anchor, position: position,
                                      offset: offset, profile: core).document
                .refittingFillLayers()
        }
        return (transform, result)
    }

    /// 16 bits and float: the typed document to publish through
    /// `PrecisionEditorCapture.publish`, the clip converted here or
    /// relabelled by the core under `profile` — at float promoted by the
    /// core's paste itself, whose report
    /// rides in the outcome.
    static func typedDocument(_ entry: LayerClipboard.Entry, capture: PrecisionEditorCapture,
                              anchor: Int, position: LayerInsertPosition,
                              placement: PastePlacement, profile: PasteProfile,
                              context: PasteContext) throws
        -> (RasterPrecisionDocument, LayerPasteOutcome) {
        let facade = capture.facade
        let work = try converted(entry, into: facade, profile: profile)
        // #547 phase 4c: a clip converted INTO CMYK is placed by its RGB
        // original's content boxes — the same geometry — because a CMYK
        // record reports no content box yet (4b's).
        let geometry = work.clip.isCmyk && !entry.layers.isCmyk ? entry.layers : work.clip
        let (offset, landed) = try offset(of: geometry, placement: placement,
                                          canvas: (facade.width, facade.height),
                                          context: context)
        let pasted = try facade.pastingLayers(work.clip, anchor: anchor, position: position,
                                              offset: offset, profile: work.core)
        // A pasted fill layer is re-rendered to cover THIS canvas.
        guard let result = try pasted.document.refittingFillLayers().precision else {
            throw DoctoredCoreError(message: "The paste produced no typed document.")
        }
        return (result, outcome(entry, clip: work.clip, into: facade, anchor: anchor,
                                position: position, placement: landed, offset: offset,
                                narrowed: work.narrowed, promoted: work.promoted,
                                profileConverted: work.profileConverted, profile: work.profile,
                                conversion: work.conversion, report: pasted.report))
    }
}
