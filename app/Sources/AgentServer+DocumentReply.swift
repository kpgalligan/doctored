import AppKit

// `documentFields` (every byte key but `summary`, `layers` and `global_light`) and
// `selectionFields` serve the byte reply in the frozen `AgentServer.swift`, and
// `typedDocumentReply` brings the core's typed `info()` to the byte shape and vocabulary —
// byte document keys and channel rows (`channelFields` merges the core row's
// `id`/`dimensions`/`provenance` by index at 16/32 bits, so `list_channels`, which the byte
// `listChannels` answers at every depth, returns the same rows), and per row `structureFields`
// (kind layer/group, parent, children/open, lock NAMES, linked; the core's every-row `open`
// dropped), the display-name `blend_mode`, the two-decimal `opacity`, the byte `adjustment` object
// `{op, params}` plus the typed `evaluation`, width/height, the mask fields and
// `is_adjustment`. The core JSON is untouched (`PrecisionFacadeMetadata` parses it). What the typed
// reply adds, named in the catalog: the typed extras; `global_light` is reported at every depth
// through `globalLightFields`. `scripts/verify_phase12h_conversion.py` g4 asserts the key
// parity.
//
// `get_document` reports `label` at both depths by NAME — the typed reply maps the core's numeric
// info row into the same vocabulary in this file.

/// `get_document`'s document-level block, shared by the byte reply
/// (AgentServer.swift) and the typed one below, so the two depths answer
/// in ONE shape and one vocabulary: the row `kind`, the rounded
/// `opacity` and the `adjustment` object are the byte ones, the typed
/// adjustment adding its `evaluation`. What the typed reply still adds or
/// lacks is named in the catalog entry: the typed extras (`subtree_start`,
/// `bounds`, `dimensions`, `link`, `mask`, `blend_if`, `offset`,
/// `description_bytes`, `blend_evaluation`). `global_light` is written by
/// each reply itself through `globalLightFields`, at every depth.
extension AgentServer {
    /// Every byte document-level key other than `summary`, `layers` and
    /// `global_light`.
    func documentFields(_ document: ImageDocument, _ doc: RasterDocument,
                        _ tree: LayerTree) throws -> [String: Any] {
        var result: [String: Any] = [:]
        if let imported = document.psdImport { result["psd_import"] = imported.json }
        // The token a recorded {"$layer": {"name", "id", "session"}} symbol
        // must carry for its `id` half to be honoured on its own. Layer ids
        // are persisted in `.rz` since 12E-1, but the SESSION decides whether
        // an id alone is honoured: in another session it must be corroborated
        // by the recorded name (`ActionIdentity`). Reporting it here is what
        // lets a caller author a symbol that resolves by id, and lets one
        // deliberately author a stale session to exercise the name fallback.
        result["identity_session"] = ActionIdentity.session
        // save_types is File > Save's policy, and losses_by_format is
        // save_copy's flat_save_losses per format (at 16 bits the only MCP
        // surface of the 11D HEIC depth sentence). A 32-bit float document
        // reports it too: its 16-bit flat save_copy tone-maps through the
        // export workspace's float stage, and the builder words those losses
        // for a float master.
        result["save_preservation"] = try Self.preservationFields(document)
        // Colour management: the document's profile, its print resolution
        // and which metadata packets it carries (AgentServer+Color.swift).
        result["color_profile"] = Self.colorProfileFields(doc, document)
        // The evaluation policy, beside the profile it is evaluated through
        // (AgentServer+Evaluation.swift).
        result["evaluation"] = Self.evaluationFields(doc)
        // The 32-bit float block, beside it (AgentServer+FloatDocument.swift).
        if let float = Self.floatFields(doc) { result["float"] = float }
        result["resolution"] = Self.resolutionFields(doc)
        var meta = Self.metadataFields(doc)
        meta.merge(Self.notCapturedField(document)) { _, new in new }
        if !meta.isEmpty { result["metadata"] = meta }
        // The document's ALPHA CHANNELS — saved selections, never part of
        // the picture (AgentServer+ChannelTargets.swift).
        let channels = Self.channelFields(doc)
        if !channels.isEmpty { result["channels"] = channels }
        // The document's GUIDES — non-printing lines tools snap to, never
        // part of the picture — and its ruler block (AgentServer+Guides.swift).
        // The guides key is omitted when there are none, on the channels
        // line's own pattern; the ruler block is always present, because a
        // document always has an origin.
        let guides = Self.guideFields(doc)
        if !guides.isEmpty { result["guides"] = guides }
        let ruler = Self.rulerFields(doc)
        if !ruler.isEmpty { result["ruler"] = ruler }
        // How many PATHS the document holds; `list_paths` has the
        // shapes. Each row's `vector_mask` flag is `structureFields`'.
        result["paths"] = doc.paths.entries.count
        if let selection = selectionFields(document) { result["selection"] = selection }
        result["pixel_layer_count"] = tree.pixelLayerCount
        result["operation_scope"] = OperationScope.supportTable
        result["edit_target"] = document.planeEditTarget.agentName(in: doc)
        result["selected_layers"] = document.selectedLayerIndices
        result["note"] = Self.layerListNote
        return result
    }

    /// The shared agent/UI selection's shape and bounds; nil when nothing
    /// is selected.
    func selectionFields(_ document: ImageDocument) -> [String: Any]? {
        guard let selection = editor(document)?.agentSelection else { return nil }
        let b = selection.bounds
        let kind: String
        switch selection.shape {
        case .rect: kind = "rect"
        case .ellipse: kind = "ellipse"
        case .polygon: kind = "polygon"
        case .mask: kind = "mask"
        case .mask16: kind = "mask16"
        }
        return [
            "kind": kind,
            "outline_simplified": selection.contourIsSimplified,
            "outline_note": selection.outlineNotice ?? "",
            "x": Int(b.minX), "y": Int(b.minY),
            "width": Int(b.width), "height": Int(b.height),
        ]
    }

    /// The 16-bit and 32-bit float `get_document`: the core's own `info()`
    /// rows and budget fields, brought to the byte reply's shape. The core
    /// JSON itself is unchanged (`PrecisionFacadeMetadata` parses it); only
    /// this MCP dictionary is rewritten.
    func typedDocumentReply(_ document: ImageDocument) throws -> String {
        guard let doc = document.doc, let typed = doc.precision else {
            throw ToolError(message: "Missing typed source.")
        }
        var info = try typed.info()
        var layers = info["layers"] as? [[String: Any]] ?? []
        let descriptionBytes = layers.reduce(UInt64(0)) {
            $0 + (($1["description_bytes"] as? NSNumber)?.uint64Value ?? 0)
        }
        // Retain the aggregate parse/field/JSON budget through serialization,
        // not only while each bounded description is read from the core.
        let artworkLease = try PrecisionEditPayload.admitted(bytes: descriptionBytes * 256 + 65_536)
        defer { withExtendedLifetime(artworkLease) {} }
        let hostLease = try PrecisionEditPayload.admitted(bytes:
            UInt64(doc.retainedSourceIndices.count) * 65_536 + 65_536)
        defer { withExtendedLifetime(hostLease) {} }
        let tree = doc.layerTree
        // The typed content boxes, one bottom-up pass as at 8 bits.
        let boxes = Self.contentBoxes(doc, tree)
        for row in layers.indices {
            if let offset = layers[row]["offset"] as? [NSNumber], offset.count == 2 {
                layers[row]["offset_x"] = offset[0].intValue
                layers[row]["offset_y"] = offset[1].intValue
            }
            // ONE label vocabulary at both depths: the core's info row
            // carries the CODE, every byte row carries the NAME, and a
            // caller must not have to know which depth it is reading
            // (LayerLabel.swift).
            if let code = (layers[row]["label"] as? NSNumber)?.uint8Value {
                layers[row]["label"] = (LayerLabel(rawValue: code) ?? .none).name
            }
            if let index = layers[row]["index"] as? Int {
                layers[row].merge(RetainedSourceInspection.fields(doc, layer: index)) { _, new in new }
                // A typed layer's whole style (NativeLayerStyle: fill opacity, effects,
                // Blend If), reported exactly as the byte rows report theirs.
                if let style = Self.layerStyleFields(doc, index) { layers[row]["style"] = style }
                // The byte row's own fields through the byte builders: the
                // structure block (kind "layer"/"group", parent,
                // children/open, the locks as NAMES, inherited_locks/
                // lock_owner, linked) replaces the core's integer `locks`,
                // its typed `kind` and its every-row `open`, which the byte
                // row carries on groups only.
                layers[row].removeValue(forKey: "locks")
                layers[row].removeValue(forKey: "open")
                layers[row].merge(Self.structureFields(
                    doc, index, tree, contentBox: index < boxes.count ? boxes[index] : nil)
                ) { _, new in new }
                // Two decimals, as the byte row reports it.
                if let opacity = (layers[row]["opacity"] as? NSNumber)?.doubleValue {
                    layers[row]["opacity"] = (opacity * 100).rounded() / 100
                }
                // The byte `adjustment` object — op and elided params from
                // the description — plus the typed evaluation; absent on
                // every non-adjustment row, as at 8 bits.
                if let core = layers[row]["adjustment"] as? [String: Any] {
                    let payload = doc.adjustmentPayload(index)
                    var adjustment: [String: Any] = [
                        "op": payload?.op ?? core["operation"] ?? NSNull(),
                        "evaluation": core["evaluation"] ?? NSNull(),
                    ]
                    if let payload { adjustment["params"] = elidedAdjustmentParams(payload) }
                    layers[row]["adjustment"] = adjustment
                } else {
                    layers[row].removeValue(forKey: "adjustment")
                }
                if let facade = doc.layerInfo(index) {
                    layers[row]["blend_mode"] = DrBlendMode.displayName(for: facade.blendMode)
                    layers[row]["width"] = facade.width
                    layers[row]["height"] = facade.height
                }
                layers[row]["has_mask"] = doc.layerHasMask(index)
                layers[row]["mask_enabled"] = doc.layerMaskEnabled(index)
                if let mask = doc.layerMaskProperties(index) { layers[row]["mask_properties"] = mask.report }
                layers[row]["is_adjustment"] = doc.layerIsAdjustment(index)
            }
            guard let index = layers[row]["index"] as? Int,
                  (layers[row]["description_bytes"] as? NSNumber)?.intValue ?? 0 > 0 else { continue }
            if let payload = doc.textPayload(index) {
                layers[row]["text"] = Self.textFields(payload, anchor: doc.describedAnchor(index))
            } else if let payload = doc.shapePayload(index) {
                layers[row]["shape"] = Self.shapeFields(payload, anchor: doc.describedAnchor(index))
            } else if let payload = doc.fillPayload(index) {
                layers[row]["fill"] = Self.fillFields(payload)
            }
        }
        info["layers"] = layers
        info.removeValue(forKey: "supported_operations")
        info.removeValue(forKey: "unsupported_operations")
        info.merge(summary(document)) { _, new in new }
        // ONE shape for the evaluation policy at both depths
        // (AgentServer+Evaluation.swift). The core's descriptor tag — the
        // value this key held before Phase 11E — moves beside it rather
        // than being dropped; `documentFields` then writes the policy.
        info["evaluation_tag"] = info["evaluation"] ?? NSNull()
        // The byte channel rows replace the core's; `channelFields` keeps
        // the core row's id, dimensions and provenance on each.
        info.removeValue(forKey: "channels")
        info.merge(try documentFields(document, doc, tree)) { _, new in new }
        // The byte reply's own `global_light` fields: the core's info
        // object, read through the facade and rounded as the byte one is.
        info["global_light"] = Self.globalLightFields(doc)
        info["precision_support"] = doc.isGray ? GrayDocument.support(doc)
            : doc.isFloat ? FloatDocument.support : DocumentPrecision.support
        // #547 Lab phase 1c: a Lab Color document's own lists.
        if doc.isLab { info["precision_support"] = LabDocument.support(doc) }
        return try jsonResult(info)
    }
}
