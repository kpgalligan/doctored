import AppKit

// Adjustment layers carry the same twenty-six non-Normal modes as rasters (native v37/schema7 —
// `isPrecisionNative` admits 37), so `allows(_:group:adjustment:)` now forbids only Pass Through on
// them. Prepare every typed property through throwing APIs before one captured publication; never
// use `try?` to let a later failed name/opacity/visibility/placement publish an earlier blend edit.
// Native group `open` works at every depth as a render-neutral bookkeeping step through the typed
// `layer_open` edit. Native v26/schema1 retains Multiply/Screen-only metadata; the prior
// seven modes use conditional v27/schema2 and Color Dodge/Burn plus Linear Dodge/Add and Linear
// Burn use v30/schema3. Subtract, Vivid Light, Linear Light, Pin Light and Divide use conditional
// v31/schema4; Hue, Saturation, Color and Luminosity use conditional v33/schema5
// (`isPrecisionNative` admits 33); Dissolve, Darker/Lighter Color and Hard Mix use conditional
// v34/schema6 (`isPrecisionNative` admits 34) under the core's band policy. Outside255 mask29 wraps
// those bodies; removing either feature restores its prior writer. A clipping member adds the
// outermost conditional v32/`LCLP` wrapper (`isPrecisionNative` admits 32); a Blend If adds the
// outermost v35/`LBIF` wrapper above it (`isPrecisionNative` admits 35); a layer style (fill
// opacity or effects) adds the outermost v36/`LSTY` wrapper above that (`isPrecisionNative` admits
// 36). A group carrying a style or Blend If may use Pass Through (it then composites
// isolated); every caller (popup, `set_layer_properties` pre-check, apply) shares
// `allows(_:document:layer:)`. In a 32-bit float document Pass Through stays group-only
// (measured against Photoshop 26.10.0) and every other mode takes
// `floatModes`; in a CMYK document every mode takes `cmykModes` (#547 phase 4a); in a Grayscale
// document `grayModes(for:)`, asked before the float arm (#547 phase 1b); in a Lab Color document
// `labModes`, asked first (#547 Lab phase 1b). Legacy byte properties keep their existing path.

/// The existing Layers blend menu and set_layer_properties/Actions share this
/// bounded native semantic set. All pixel work stays in the captured core owner.
enum NativeLayerBlending {
    static let colorModes: [DrBlendMode] = [DR_BLEND_NORMAL, DR_BLEND_MULTIPLY, DR_BLEND_SCREEN,
        DR_BLEND_OVERLAY, DR_BLEND_SOFT_LIGHT, DR_BLEND_HARD_LIGHT, DR_BLEND_DARKEN,
        DR_BLEND_LIGHTEN, DR_BLEND_DIFFERENCE, DR_BLEND_EXCLUSION, DR_BLEND_COLOR_DODGE,
        DR_BLEND_COLOR_BURN, DR_BLEND_ADDITION, DR_BLEND_LINEAR_BURN, DR_BLEND_SUBTRACT,
        DR_BLEND_VIVID_LIGHT, DR_BLEND_LINEAR_LIGHT, DR_BLEND_PIN_LIGHT, DR_BLEND_DIVIDE,
        DR_BLEND_HUE, DR_BLEND_SATURATION, DR_BLEND_COLOR, DR_BLEND_LUMINOSITY,
        DR_BLEND_DISSOLVE, DR_BLEND_DARKER_COLOR, DR_BLEND_LIGHTER_COLOR, DR_BLEND_HARD_MIX]
    static var modeNames: [String] { colorModes.map { DrBlendMode.displayName(for: $0) } }
    static var limitation: String {
        "16-bit layer blending supports " + modeNames.joined(separator: ", ")
            + " on raster layers, adjustment layers and isolated groups. Pass Through requires a group."
    }

    static func mode(named name: String) -> DrBlendMode? {
        (DrBlendMode.allBlendModes + [(DR_BLEND_PASS_THROUGH, "Pass Through")])
            .first { $0.1.caseInsensitiveCompare(name) == .orderedSame }?.0
    }

    /// An adjustment layer blends its adjusted result as the source (native
    /// v37 / LBLD schema7); only Pass Through is still group-only.
    static func allows(_ mode: DrBlendMode, group: Bool, adjustment: Bool) -> Bool {
        if mode == DR_BLEND_NORMAL { return true }
        if mode == DR_BLEND_PASS_THROUGH { return group }
        return colorModes.contains(mode)
    }

    /// The modes a 32-bit float document admits on a layer, as Fill… /
    /// Stroke… and paint ink and on a layer effect
    /// (`float_document::admitted_blend`): the five `blend.rs` kernels with no
    /// clamp of their own, the nine measured and built — Dissolve,
    /// Darker/Lighter Color, Linear Dodge (Add), Subtract, Hue, Saturation,
    /// Color, Luminosity — and four more: Linear Burn and Linear Light
    /// unclamped, Pin Light on its master, Divide floored at 1/65504 and held
    /// to ±65504. The nine whose formula inverts about white (Screen, Overlay,
    /// Soft Light, Hard Light, Color Dodge, Color Burn, Vivid Light, Hard Mix,
    /// Exclusion) stay out; the core's operator table says why.
    static let floatModes: [DrBlendMode] = [
        DR_BLEND_NORMAL, DR_BLEND_DISSOLVE, DR_BLEND_DARKEN, DR_BLEND_MULTIPLY,
        DR_BLEND_DARKER_COLOR, DR_BLEND_LINEAR_BURN, DR_BLEND_LIGHTEN, DR_BLEND_ADDITION,
        DR_BLEND_LIGHTER_COLOR, DR_BLEND_LINEAR_LIGHT, DR_BLEND_PIN_LIGHT,
        DR_BLEND_DIFFERENCE, DR_BLEND_SUBTRACT, DR_BLEND_DIVIDE, DR_BLEND_HUE,
        DR_BLEND_SATURATION, DR_BLEND_COLOR, DR_BLEND_LUMINOSITY,
    ]
    static var floatModeNames: [String] { floatModes.map { DrBlendMode.displayName(for: $0) } }
    static var floatLimitation: String {
        "32-bit float layer blending supports "
            + floatModeNames.joined(separator: ", ")
            + " only; every other mode is unavailable in a 32-bit float document."
            + " Pass Through requires a group."
    }

    /// The modes a CMYK document offers on a raster layer, an adjustment layer or an isolated group
    /// (#547 phase 4a), in menu order: the core's `cmyk_document::CMYK_BLEND_MODES`, all 27
    /// (`scripts/photoshop-probe/cmyk_blend_take.py`), evaluated on `1 − ink`. Pass Through takes
    /// the group rule, as at every depth.
    static let cmykModes: [DrBlendMode] = [
        DR_BLEND_NORMAL, DR_BLEND_DISSOLVE, DR_BLEND_DARKEN, DR_BLEND_MULTIPLY,
        DR_BLEND_COLOR_BURN, DR_BLEND_LINEAR_BURN, DR_BLEND_DARKER_COLOR, DR_BLEND_LIGHTEN,
        DR_BLEND_SCREEN, DR_BLEND_COLOR_DODGE, DR_BLEND_ADDITION, DR_BLEND_LIGHTER_COLOR,
        DR_BLEND_OVERLAY, DR_BLEND_SOFT_LIGHT, DR_BLEND_HARD_LIGHT, DR_BLEND_VIVID_LIGHT,
        DR_BLEND_LINEAR_LIGHT, DR_BLEND_PIN_LIGHT, DR_BLEND_HARD_MIX, DR_BLEND_DIFFERENCE,
        DR_BLEND_EXCLUSION, DR_BLEND_SUBTRACT, DR_BLEND_DIVIDE, DR_BLEND_HUE,
        DR_BLEND_SATURATION, DR_BLEND_COLOR, DR_BLEND_LUMINOSITY,
    ]
    static var cmykLimitation: String {
        "CMYK layer blending supports "
            + cmykModes.map { DrBlendMode.displayName(for: $0) }.joined(separator: ", ")
            + " on raster layers, adjustment layers and isolated groups; every other mode is"
            + " unavailable in a CMYK document. Pass Through requires a group."
    }

    /// The modes a Grayscale document offers (#547), in menu order: the core's
    /// `gray_document::GRAY_BLEND_MODES`, 21 modes (measured against Photoshop 26.10,
    /// `scripts/photoshop-probe/gray_mode_take.py`): every mode but Darker Color, Lighter Color,
    /// Hue, Saturation, Color and Luminosity. Pass Through takes the group rule.
    static let grayModes: [DrBlendMode] = [
        DR_BLEND_NORMAL, DR_BLEND_DISSOLVE, DR_BLEND_DARKEN, DR_BLEND_MULTIPLY,
        DR_BLEND_COLOR_BURN, DR_BLEND_LINEAR_BURN, DR_BLEND_LIGHTEN,
        DR_BLEND_SCREEN, DR_BLEND_COLOR_DODGE, DR_BLEND_ADDITION,
        DR_BLEND_OVERLAY, DR_BLEND_SOFT_LIGHT, DR_BLEND_HARD_LIGHT, DR_BLEND_VIVID_LIGHT,
        DR_BLEND_LINEAR_LIGHT, DR_BLEND_PIN_LIGHT, DR_BLEND_HARD_MIX, DR_BLEND_DIFFERENCE,
        DR_BLEND_EXCLUSION, DR_BLEND_SUBTRACT, DR_BLEND_DIVIDE,
    ]

    /// `grayModes` at `document`'s depth: at 32 bits only those a float document admits (12).
    static func grayModes(for document: RasterDocument) -> [DrBlendMode] {
        document.isFloat ? grayModes.filter { floatModes.contains($0) } : grayModes
    }

    static func grayLimitation(for document: RasterDocument) -> String {
        "Grayscale layer blending supports "
            + grayModes(for: document).map { DrBlendMode.displayName(for: $0) }.joined(separator: ", ")
            + "; every other mode is unavailable in a Grayscale document."
            + " Pass Through requires a group."
    }

    /// The modes a Lab Color document offers (#547 Lab phase 1b), in menu order: the core's
    /// `lab_document::LAB_BLEND_MODES`, every mode but `labNotOffered` at 8 and 16 bits (measured
    /// against Photoshop 26.10, `scripts/photoshop-probe/lab_mode_take.py`, which dropped Subtract
    /// and Divide). Darker Color, Lighter Color and the four HSL modes joined in phase 3;
    /// each mode composites by its Lab arm (`lab_blend`). Pass Through takes the group rule.
    static let labModes: [DrBlendMode] = [
        DR_BLEND_NORMAL, DR_BLEND_DISSOLVE, DR_BLEND_MULTIPLY, DR_BLEND_LINEAR_BURN,
        DR_BLEND_DARKER_COLOR, DR_BLEND_SCREEN, DR_BLEND_ADDITION, DR_BLEND_LIGHTER_COLOR,
        DR_BLEND_OVERLAY, DR_BLEND_SOFT_LIGHT, DR_BLEND_HARD_LIGHT, DR_BLEND_VIVID_LIGHT,
        DR_BLEND_LINEAR_LIGHT, DR_BLEND_PIN_LIGHT, DR_BLEND_HARD_MIX, DR_BLEND_HUE,
        DR_BLEND_SATURATION, DR_BLEND_COLOR, DR_BLEND_LUMINOSITY,
    ]

    /// The modes not offered in Lab at 8 or 16 bits, in menu order (measured against Photoshop
    /// 26.10 by the same take): the core's `lab_document::LAB_NOT_OFFERED`, refused for good.
    static let labNotOffered: [DrBlendMode] = [
        DR_BLEND_DARKEN, DR_BLEND_COLOR_BURN, DR_BLEND_LIGHTEN, DR_BLEND_COLOR_DODGE,
        DR_BLEND_DIFFERENCE, DR_BLEND_EXCLUSION, DR_BLEND_SUBTRACT, DR_BLEND_DIVIDE,
    ]

    static var labLimitation: String {
        "Lab Color layer blending supports "
            + labModes.map { DrBlendMode.displayName(for: $0) }.joined(separator: ", ")
            + "; every other mode is unavailable in a Lab Color document."
            + " Pass Through requires a group."
    }

    /// The limitation sentence of `document`'s depth or mode.
    static func limitation(for document: RasterDocument) -> String {
        if document.isLab { return labLimitation }
        if document.isGray { return grayLimitation(for: document) }
        return document.isCmyk ? cmykLimitation : document.isFloat ? floatLimitation : limitation
    }

    static func allows(_ mode: DrBlendMode, document: RasterDocument, layer: Int) -> Bool {
        guard document.layerInfo(layer) != nil else { return false }
        if document.isLab, mode != DR_BLEND_PASS_THROUGH { return labModes.contains(mode) }
        if document.isGray, mode != DR_BLEND_PASS_THROUGH {
            return grayModes(for: document).contains(mode)
        }
        if document.isFloat, mode != DR_BLEND_PASS_THROUGH { return floatModes.contains(mode) }
        if document.isCmyk, mode != DR_BLEND_PASS_THROUGH, !cmykModes.contains(mode) { return false }
        return allows(mode, group: document.layerIsGroup(layer), adjustment: document.layerIsAdjustment(layer))
    }

    static func apply(_ mode: DrBlendMode, to document: ImageDocument) {
        do {
            let capture = try PrecisionEditorCapture(document)
            guard capture.selectedLayers.allSatisfy({ allows(mode, document: capture.facade, layer: $0) }) else {
                throw DoctoredCoreError(message: limitation(for: capture.facade))
            }
            var updated = capture.facade
            var changed = false
            for index in capture.selectedLayers {
                if let next = try updated.settingNativeLayerBlendMode(index, mode: mode) {
                    updated = next
                    changed = true
                }
            }
            try capture.requireCurrent(document)
            guard changed else { return }
            try capture.publish(updated.precision, to: document, name: "Layer Blend Mode",
                record: .selectionProperty("blend_mode", DrBlendMode.displayName(for: mode),
                    selectionCount: capture.selectedLayers.count, note: "Layer Blend Mode"))
        } catch { document.presentError(error) }
    }
}

extension RasterDocument {
    /// Version1 JSON through the existing admitted/cancellable typed edit ABI.
    /// The throwing UI/MCP paths preserve refusal, including compound edits.
    func settingNativeLayerBlendMode(_ layer: Int, mode: DrBlendMode) throws -> RasterDocument? {
        guard NativeLayerBlending.allows(mode, document: self, layer: layer) else {
            throw DoctoredCoreError(message: NativeLayerBlending.limitation(for: self))
        }
        return try performingPrecisionPixelEdit(PrecisionPixelCommand([
            "op": "layer_blend", "version": 1, "layer": layer, "blend_mode": mode.rawValue
        ]))
    }
}

extension AgentServer {
    /// Mirrors the Layers property controls. Prepare every requested native
    /// property through throwing APIs, then publish once; a later failure or
    /// cancellation cannot publish an earlier successful blend/name change.
    func nativeLayerProperties(_ document: ImageDocument, index: Int, name: String?, opacity: Double?,
                               mode: DrBlendMode?, visible: Bool?, x: Int?, y: Int?, open: Bool?) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        guard let info = capture.facade.layerInfo(index) else { throw ToolError(message: "Invalid layer index.") }
        if let opacity, !opacity.isFinite { throw ToolError(message: "Layer opacity must be finite.") }
        var updated = capture.facade
        var changed = false
        // Whether any step OTHER than the bookkeeping one (the name) changed
        // something: a rename alone changes no pixel and publishes
        // render-neutral below. A group's disclosure is not part of this
        // publication: it is the panel triangle's no-undo change,
        // applied after it through `applyGroupDisclosure`.
        var beyondBookkeeping = false
        func accept(_ next: RasterDocument?, bookkeeping: Bool = false) {
            if let next { updated = next; changed = true; if !bookkeeping { beyondBookkeeping = true } }
        }
        if let mode { accept(try updated.settingNativeLayerBlendMode(index, mode: mode)) }
        if let name { accept(try updated.withLayerName(index, name), bookkeeping: true) }
        if let opacity {
            guard let native = updated.precision else { throw ToolError(message: "Missing native owner.") }
            let next = try native.settingLayerOpacity(index, opacity: Float(min(max(opacity, 0), 1)))
            accept(try next.map { try RasterDocument(precision: $0) })
        }
        if let visible { accept(try updated.performingPrecisionPixelEdit(.init(["op": "layer_visible", "layer": index, "visible": visible]))) }
        // A group's offset is the union of its rasters' buffers, and writing it
        // shifts the subtree (byte `dr_doc_with_layer_offset`); a group with no
        // raster has no origin to move, which the byte path answers as no change.
        if x != nil || y != nil, !info.isGroup || info.width > 0 || info.height > 0 {
            // A moved pattern fill is re-rendered over the canvas
            // (FillLayer.swift's PIN), as `withLayerOffset` does.
            accept(try updated.precisionLayerOffset(index, dx: (x ?? info.offsetX) - info.offsetX,
                                                    dy: (y ?? info.offsetY) - info.offsetY)?
                .refittingFillLayers())
        }
        try capture.requireCurrent(document)
        var published = false
        if changed, !beyondBookkeeping {
            // Only the name changed: no rendered pixel moved, so the current
            // projection is carried instead of preparing a new one.
            let renamed = updated
            try performGroupedEdit(document, "Layer Properties", renderNeutral: true) { _ in renamed }
            published = true
        } else if changed {
            guard try capture.publish(updated.precision, to: document, name: "Layer Properties") else {
                throw ToolError(message: "The prepared native properties could not be published.")
            }
            published = true
        }
        var disclosed = false
        if let open {
            disclosed = try applyGroupDisclosure(document, index: index, open: open, edited: published)
        }
        guard published || disclosed else {
            return try structureNoOp(["layer": index], why: "the layer already carries every requested property")
        }
        return try jsonResult(["ok": true, "document": summary(document)])
    }
}
