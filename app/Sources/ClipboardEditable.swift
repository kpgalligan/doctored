import AppKit

// **12G-3 (this file):** a whole-layer Copy (no selection) of a text, shape or adjustment layer
// attaches its `EditableClipboardRecord` as the RZCB v3 section
// (`RasterDocument.attachingEditable`; an adjustment layer's copy is `transparentPayload()`, u16
// byte-origin at every depth, which `publicRendering` renders as its exact `rgba8()`);
// `PastePlan.resolve` answers `.editable` BEFORE every depth/profile rule when
// `editableDisposition` says the record restores, through the tools' own paths (`nativeArtwork`;
// `addingAdjustmentLayer` / `addingAdjustment(anchor:position:)`), and otherwise the payload pastes
// as pixels with `outcome.editable.reason`.
//
// 12G-3's editable copy declines RAW (retained-source) layers (they copy as pixels only).

/// Phase 12G-3: the EDITABLE clipboard record. A whole-layer Copy — no
/// selection — of a text, shape or adjustment layer carries the layer's
/// record as the RZCB v3 editable section (`precision_clipboard_codec`), at
/// every depth, and Paste restores it through the tools' own paths: artwork
/// re-rendered at native depth from its description (`nativeArtwork`), an
/// adjustment re-created from its meta (`addingAdjustmentLayer` at 8 bits,
/// `addingAdjustment(anchor:position:)` at depth). Everything else stays
/// raster, and a record this build or this document cannot take pastes as
/// pixels with its reason disclosed (`EditableDisposition`).
///
/// Section JSON: `{"type": "text"|"shape"|"adjust", "name": <layer name>,
/// "record": <meta verbatim>, "anchor": [x, y] (artwork only)}`. The core
/// validates that shape and never reads `record`.
enum EditableClipboardRecord {
    case artwork(LayerDescription, anchor: CGPoint, name: String)
    /// `record` is the layer's meta VERBATIM (`layerMeta`), which the 8-bit
    /// restore attaches unchanged.
    case adjustment(AdjustmentLayerPayload, record: String, name: String)

    var type: String {
        switch self {
        case let .artwork(description, _, _): return description.kindName
        case .adjustment: return AdjustmentLayerPayload.typeName
        }
    }

    var name: String {
        switch self {
        case let .artwork(_, _, name), let .adjustment(_, _, name): return name
        }
    }

    func section() throws -> String {
        var object: [String: Any] = ["type": type, "name": name]
        switch self {
        case let .artwork(description, anchor, _):
            guard let record = description.json() else {
                throw DoctoredCoreError(message: "The layer's description could not be encoded.")
            }
            object["record"] = record
            object["anchor"] = [Double(anchor.x), Double(anchor.y)]
        case let .adjustment(_, record, _):
            object["record"] = record
        }
        guard JSONSerialization.isValidJSONObject(object),
              let text = String(
                data: try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
                encoding: .utf8)
        else { throw DoctoredCoreError(message: "The editable record could not be encoded.") }
        return text
    }

    /// nil: a type or a record this build cannot decode.
    static func decode(section: String) -> EditableClipboardRecord? {
        guard let object = (try? JSONSerialization.jsonObject(with: Data(section.utf8)))
                as? [String: Any],
              let type = object["type"] as? String,
              let record = object["record"] as? String
        else { return nil }
        let name = object["name"] as? String ?? "Pasted Layer"
        switch type {
        case DescribedKind.text.rawValue, DescribedKind.shape.rawValue:
            guard let description = LayerDescription.decode(record),
                  description.kindName == type,
                  let pair = object["anchor"] as? [NSNumber], pair.count == 2
            else { return nil }
            let anchor = CGPoint(x: pair[0].doubleValue, y: pair[1].doubleValue)
            return .artwork(description, anchor: anchor, name: name)
        case AdjustmentLayerPayload.typeName:
            guard let payload = AdjustmentLayerPayload.decode(record) else { return nil }
            return .adjustment(payload, record: record, name: name)
        default:
            return nil
        }
    }
}

/// What a Paste will do with a payload's section: restore `record`, or —
/// `record` nil — paste the pixels, for the named `reason`.
struct EditableDisposition {
    let type: String
    let record: EditableClipboardRecord?
    let reason: String?
}

extension RasterDocument {
    /// Layer `layer`'s editable record: an adjustment layer's meta, or a
    /// text/shape description (a legacy box resolved from the raster) with
    /// its exact anchor; nil for every other layer — raster, group,
    /// retained source.
    func editableClipboardRecord(layer: Int) -> EditableClipboardRecord? {
        guard let info = layerInfo(layer) else { return nil }
        if layerIsAdjustment(layer) {
            guard let meta = layerMeta(layer),
                  let payload = AdjustmentLayerPayload.decode(meta) else { return nil }
            return .adjustment(payload, record: meta, name: info.name)
        }
        guard let description = layerDescription(layer),
              description.kind == .text || description.kind == .shape,
              let anchor = describedAnchor(layer)
        else { return nil }
        return .artwork(description.resolved(rasterWidth: info.width), anchor: anchor,
                        name: info.name)
    }

    /// `payload` carrying layer `layer`'s editable record, or `payload`
    /// itself when the layer has none. The whole-layer Copy's one attach.
    func attachingEditable(_ payload: RasterClipboardPayload,
                           layer: Int) throws -> RasterClipboardPayload {
        guard let record = editableClipboardRecord(layer: layer) else { return payload }
        let budget = try PrecisionWorkContext()
        return try payload.withEditable(try record.section(), workingLimit: budget.limit,
                                        otherRetained: budget.otherRetained)
    }

    /// An adjustment layer's Copy at every depth: a canvas-sized transparent
    /// byte-origin payload at (0, 0) — u16, also in a float document.
    func transparentPayload() throws -> RasterClipboardPayload {
        guard let image = transparentCanvasImage()
        else { throw DoctoredCoreError(message: "The transparent copy could not be made.") }
        return try bytePayload(image, offset: (0, 0))
    }

    /// The canvas-sized transparent image `transparentPayload()` carries —
    /// also the 8-bit whole-layer Copy's pixels for an adjustment layer.
    func transparentCanvasImage() -> RasterImage? {
        RasterImage.from(
            rgba: [UInt8](repeating: 0, count: width * height * 4), width: width, height: height)
    }

    /// The pixels an 8-bit Copy of layer `layer` starts from: an adjustment
    /// layer copied whole (no selection) is transparent, like
    /// `transparentPayload()` at depth; every other copy is the layer's own.
    func byteCopySource(layer: Int, whole: Bool) -> RasterImage? {
        whole && layerIsAdjustment(layer) ? transparentCanvasImage() : layerCanvasImage(layer)
    }

    /// nil when `payload` carries no section; otherwise the record to
    /// restore into this document, or nil with the reason it pastes as
    /// pixels instead. `profile` is the paste's choice: Don't Convert keeps
    /// a CMYK record's ink verbatim in a CMYK document.
    func editableDisposition(of payload: TypedClipboardPayload,
                             profile: PasteProfile = .convert) -> EditableDisposition? {
        guard let editable = payload.editable else { return nil }
        let type = editable["type"] as? String ?? ""
        func raster(_ why: String) -> EditableDisposition {
            EditableDisposition(type: type, record: nil,
                                reason: "The clipboard's editable \(type) record pastes as pixels: "
                                    + why + ".")
        }
        // #547 phase 4: a Grayscale document restores a record only from a Grayscale copy (its
        // colours are already gray); any other pastes as the converted pixels.
        if isGray, !payload.isGray {
            return raster("a Grayscale document restores an editable record only from a "
                + "Grayscale copy")
        }
        // #547 Lab phase 4c: a record restores only between Lab Color documents (a Lab copy's
        // colours are Lab codes, an RGB copy's are not); any other pair pastes as the converted
        // pixels.
        if isLab != payload.isLab {
            return raster(isLab
                ? "a Lab Color document restores an editable record only from a Lab Color copy"
                : "a Lab Color copy's editable record restores only in a Lab Color document")
        }
        let section = (try? payload.handle.editableSection()) ?? nil
        guard let section, let record = EditableClipboardRecord.decode(section: section) else {
            return raster("this build cannot decode it")
        }
        switch record {
        case let .adjustment(adjustment, _, _):
            if adjustment.knownOp == nil {
                return raster("its adjustment \"\(adjustment.op)\" is not one this build knows")
            }
            // A NEW calibrated layer in a profile it cannot evaluate in:
            // the core's sentence, already capitalised and punctuated. 8 bits
            // only: at 16 bits the paste refuses before any record restores.
            if !isHighDepth, CalibratedAdjustment.supports(adjustment.op),
               adjustment.string("evaluation", default: CalibratedAdjustment.legacy)
                   == CalibratedAdjustment.document,
               let refusal = calibratedRefusal(op: adjustment.op) {
                return EditableDisposition(
                    type: type, record: nil,
                    reason: "The clipboard's editable \(type) record pastes as pixels. " + refusal)
            }
        case let .artwork(description, anchor, name):
            // A 32-bit float document restores it too, its colour
            // numbers rewritten into linear light (`nativeArtwork`).
            if !description.isRenderable {
                return raster("its text family is not installed on this Mac")
            }
            // #547: a record in ink (a CMYK document's copy) restores in this
            // document's colours: into another CMYK profile, or into RGB,
            // relative colorimetric with BPC (`convertedArtwork`); the same
            // profile, and Don't Convert into a CMYK document, keep the ink
            // verbatim. An sRGB record converts into a CMYK document's ink in
            // `nativeArtwork`.
            if description.artworkInk == .cmyk, !(profile == .preserve && isCmyk) {
                let source = ArtworkColorEnd.cmyk(profile: payload.profileData)
                do {
                    let converted = try convertedArtwork(description, from: source,
                                                         options: Self.artworkColorOptions)
                    return EditableDisposition(type: type, record: .artwork(converted, anchor: anchor, name: name),
                                               reason: nil)
                } catch {
                    return raster("its ink could not be converted ("
                        + ((error as? DoctoredCoreError)?.message ?? error.localizedDescription) + ")")
                }
            }
        }
        return EditableDisposition(type: type, record: record, reason: nil)
    }

    /// New from Clipboard's restore: this freshly made one-"Background"
    /// document rebuilt around `payload`'s editable record, with the layer
    /// to make active and what happened to the record. Artwork replaces the
    /// Background (its anchor moved by `-payload.offset`, the new canvas's
    /// origin on the source canvas); an adjustment lands above the kept
    /// transparent Background. A payload with no section, a record this
    /// document cannot take, and any restore that throws or produces
    /// nothing all keep the pixel document, the last with its reason.
    /// Never throws.
    func restoringEditableDocument(_ payload: TypedClipboardPayload)
        -> (document: RasterDocument, active: Int,
            editable: (type: String, restored: Bool, reason: String?)?) {
        guard let disposition = editableDisposition(of: payload) else { return (self, 0, nil) }
        let type = disposition.type
        guard let record = disposition.record else {
            return (self, 0, (type, false, disposition.reason))
        }
        func failed(_ why: String?) -> (document: RasterDocument, active: Int,
                                        editable: (type: String, restored: Bool, reason: String?)?) {
            (self, 0, (type, false,
                       "The clipboard's editable \(type) record could not be restored; the new "
                           + "document holds its pixels. "
                           + (why ?? "The restore produced no document.")))
        }
        do {
            switch record {
            case .artwork:
                guard let restored = try restoringEditable(
                    record, anchor: 0, position: .above,
                    shift: (-payload.offset.x, -payload.offset.y), name: record.name),
                    let result = try restored.removingLayer(0)
                else { return failed(nil) }
                return (result, 0, (type, true, nil))
            case .adjustment:
                guard let result = try restoringEditable(
                    record, anchor: 0, position: .above, shift: (0, 0), name: record.name)
                else { return failed(nil) }
                return (result, 1, (type, true, nil))
            }
        } catch {
            return failed((error as? DoctoredCoreError)?.message ?? error.localizedDescription)
        }
    }

    /// The record restored as a new layer at the slot `position` names
    /// relative to `anchor`, named `name`; artwork moves by `shift` (the
    /// placement's offset minus the payload's). nil = exact no-op.
    func restoringEditable(_ record: EditableClipboardRecord, anchor: Int,
                           position: LayerInsertPosition, shift: (x: Int, y: Int),
                           name: String) throws -> RasterDocument? {
        switch record {
        case let .artwork(description, point, _):
            let moved = CGPoint(x: point.x + CGFloat(shift.x), y: point.y + CGFloat(shift.y))
            return try nativeArtwork(description, anchor: moved, layer: anchor, replacing: false,
                                     name: name, position: position)
        case let .adjustment(payload, meta, _):
            guard let precision else {
                return addingAdjustmentLayer(anchor: anchor, position: position, name: name,
                                             meta: meta, selection: nil)
            }
            guard let op = payload.knownOp else {
                throw DoctoredCoreError(message: "The adjustment \"\(payload.op)\" is not one this "
                    + "build knows.")
            }
            let description = try PrecisionAdjustment(op: op, params: payload.params).description
            let budget = try PrecisionWorkContext(source: precision)
            return try precision.addingAdjustment(
                name: name, description: description, anchor: anchor, position: position,
                workingLimit: budget.limit, otherRetained: budget.otherRetained)
                .map { try RasterDocument(precision: $0) }
        }
    }
}
