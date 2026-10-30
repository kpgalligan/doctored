import AppKit

// Auto scopes are explicit and independent of selected writeback; old Actions retain whole-target
// analysis.

/// Analysis is independent of the captured write coverage. Historical Actions
/// omit the field and continue to measure the whole target.
enum ToneAnalysisScope: String, CaseIterable {
    case wholeTarget = "whole_target"
    case selection

    static func parse(_ value: Any?) throws -> Self {
        guard let value else { return .wholeTarget }
        guard let text = value as? String, let scope = Self(rawValue: text) else {
            throw DoctoredCoreError(message: "analysis_scope must be whole_target or selection.")
        }
        return scope
    }
}

/// Native destructive edits retain the original source and canvas coverage;
/// neighborhood evaluation never sees the display image or clipped source.
enum SelectedAdjustment {
    static let spatial: Set<String> = ["blur", "sharpen", "unsharp_mask", "high_pass", "reduce_noise",
        "dust_and_scratches", "edge_detect", "emboss", "pixelate", "add_noise",
        "box_blur", "surface_blur", "add_grain", "radial_blur", "clouds", "difference_clouds", "offset",
        "motion_blur", "lens_blur", "tilt_shift", "lens_correction", "presence", "develop_filter",
        "smart_sharpen", "depth_blur"]

    static func numericParameters(_ parameters: [String: Any]) -> [String: Any] {
        var result = parameters
        for (key, value) in parameters {
            guard let text = value as? String else { continue }
            if key == "high_contrast", text == "true" || text == "false" { result[key] = text == "true" }
            else if key == "seed", let integer = Int64(text) { result[key] = integer }
            else if key == "seed", let integer = UInt64(text) { result[key] = integer }
            else if key.hasSuffix("_color") { continue }
            else if let number = Double(text) { result[key] = number }
        }
        return result
    }

    static func request(layer: Int, filter: String, params: [String: Any]) throws -> String {
        var fields: [String: Any] = ["layer": layer]
        if let op = AdjustmentLayerOp(rawValue: filter == "adjust" ? "bcs" : filter) {
            fields["op"] = "adjustment"
            fields["description"] = try PrecisionAdjustment(op: op, params: params).description
        } else {
            guard spatial.contains(filter) else { throw DoctoredCoreError(message: "Unknown native filter: \(filter).") }
            fields["op"] = filter
            fields["params"] = params
        }
        return try json(fields)
    }

    static func json(_ fields: [String: Any]) throws -> String {
        guard JSONSerialization.isValidJSONObject(fields) else {
            throw DoctoredCoreError(message: "Filter parameters must be finite JSON values.")
        }
        let data = try JSONSerialization.data(withJSONObject: fields, options: .sortedKeys)
        guard data.count <= 20 * 1024 * 1024, let text = String(data: data, encoding: .utf8) else {
            throw DoctoredCoreError(message: "Filter parameters exceed their size limit.")
        }
        return text
    }

    static func edit(_ capture: PrecisionEditorCapture, layer: Int, filter: String,
                     params: [String: Any], target: PaintTarget? = nil) throws -> PrecisionCapturedEdit {
        if let reason = OperationScope.refusalReason(.filter, target: target ?? capture.target, doc: capture.facade, layerIndex: layer) {
            throw DoctoredCoreError(message: reason)
        }
        // A recorded Lens Correction step names its profile ("auto" or a lens id); the kernel
        // takes the object, matched here to this document (v1-lensfun).
        let named = filter == "lens_correction" ? try capture.facade.lensResolvedParameters(params, layer: layer) : params
        var params = try FilterFrame.kernelParameters(filter, named, target: target ?? capture.target,
                                                      lab: capture.facade.isLab)
        // #547: a CMYK document's Clouds take ink — the swatches' stored ink, else their colours
        // through the profile.
        if capture.source.isCmyk {
            params = try CmykAdjustFilter.inkParams(filter: filter, params: params, document: capture.source)
        }
        let request = try request(layer: layer, filter: filter, params: params)
        return try edit(capture, request: request, target: target)
    }

    static func edit(_ capture: PrecisionEditorCapture, request: String, target: PaintTarget? = nil) throws -> PrecisionCapturedEdit {
        guard (target ?? capture.target) == .layer, !capture.quickMask else {
            throw DoctoredCoreError(message: "Native destructive filters require raster-layer RGB outside Quick Mask.")
        }
        let coverage = try capture.selection.map {
            try PrecisionStrokeCoverage.selection(width: capture.facade.width, height: capture.facade.height, selection: $0)
        }
        return try PrecisionCapturedEdit(capture.source, operation: .selected(request: request, coverage: coverage))
    }

    /// Legacy flat filters keep their original names/units in new recordings.
    static func record(filter: String, params: [String: Any]) -> [ActionStep] {
        let name = filter == "bcs" ? "adjust" : filter
        return .filter(name, AdjustmentSchema.op(forFilter: name) == nil && name != "curves" ? params : ["params": params], target: "layer")
    }
}

extension EditorViewController {
    func applyPrecisionFilter(_ filter: String, params: [String: Any] = [:]) -> Bool {
        guard let document, document.doc?.isHighDepth == true else { return false }
        PrecisionAdjustmentSession(document, canvas: canvas, mode: .destructive)?.commit(filter: filter, params: params)
        return true
    }
}

extension OperationCapture {
    func analysisCoverage(_ scope: ToneAnalysisScope) throws -> [UInt8]? {
        guard scope == .selection else { return nil }
        guard let selection else { throw DoctoredCoreError(message: "Selection analysis requires an active selection.") }
        if let placement {
            _ = try RasterResources.shared.admit(scratch: UInt64(source.width) * UInt64(source.height))
            guard let mask = placement.layerPlane(from: selection) else {
                throw DoctoredCoreError(message: "Could not map analysis coverage into the captured target.")
            }
            return mask
        }
        return selection
    }
}
