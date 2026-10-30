import Foundation

/// `apply_filter`'s 8-bit `dr_image_filter_op` filters: Box Blur,
/// Surface Blur, Add Grain, Radial Blur, Clouds, Difference Clouds, Offset, Motion Blur,
/// Lens Blur and Tilt-Shift (Filters ▸ Blur ▸ Box Blur… / Lens Blur… / Motion Blur… /
/// Radial Blur… / Surface Blur… / Tilt-Shift…, Filters ▸ Noise ▸
/// Add Grain…, Filters ▸ Render ▸ Clouds / Difference Clouds, Filters ▸ Other ▸
/// Offset…), Lens Correction, Presence and Develop Filter (Filters ▸ Lens Correction… /
/// Presence… / Develop Filter…), Smart Sharpen (Filter ▸ Sharpen ▸ Smart
/// Sharpen…), the photographic
/// sharpening and cleanup filters (Unsharp Mask…, High Pass…, Reduce Noise…,
/// Dust & Scratches…) and Gaussian Blur (`blur`, Filters ▸ Blur ▸
/// Gaussian Blur…, whose request `AgentServer.filtered` builds after its own
/// sigma guard). Each is planned by the core and admitted against the
/// working limit, then runs on the worker with the progress panel and Cancel,
/// as the sheets' Apply does (`ByteFilterAdmission.run`), so a filter that
/// does not fit answers admission's sentence in-band and the call still waits
/// for its result. Replayed Actions and ⌃F reach it through `apply_filter`.
extension AgentServer {
    /// The unframed filters' flat arguments as a request, or nil for every
    /// other filter (Radial Blur and Offset go through `framedByteFilter`).
    /// The photographic four take their defaults here; Box Blur, Surface Blur,
    /// Add Grain, the two Clouds, Motion Blur, Lens Blur and Tilt-Shift pass
    /// only the flat arguments the call
    /// carries, through the numeric coercion the 16-bit request uses, so the
    /// core's own defaults fill the rest.
    func byteFilterRequest(_ filter: String, _ a: [String: Any]) -> ByteFilterRequest? {
        let params: [String: Any]
        switch filter {
        case "unsharp_mask":
            params = [
                "radius": doubleArg(a, "radius") ?? 1.5, "amount": doubleArg(a, "amount") ?? 1,
                "threshold": doubleArg(a, "threshold") ?? 0]
        case "high_pass":
            params = ["radius": doubleArg(a, "radius") ?? 3]
        case "reduce_noise":
            params = [
                "radius": doubleArg(a, "radius") ?? 1, "luminance": doubleArg(a, "luminance") ?? 0.01,
                "detail": doubleArg(a, "detail") ?? 0.5, "color": doubleArg(a, "color") ?? 0.5]
        case "dust_and_scratches":
            params = ["radius": doubleArg(a, "radius") ?? 2, "threshold": doubleArg(a, "threshold") ?? 0]
        case "box_blur", "surface_blur", "add_grain", "clouds", "difference_clouds",
             "motion_blur", "lens_blur", "tilt_shift", "lens_correction", "presence", "develop_filter",
             "smart_sharpen":
            let keys = Self.flatFilterArguments[filter] ?? []
            params = SelectedAdjustment.numericParameters(
                keys.reduce(into: [String: Any]()) { if let value = a[$1] { $0[$1] = value } })
        default:
            return nil
        }
        return ByteFilterRequest(op: filter, params: params)
    }

    /// Radial Blur and Offset at 8 bits (Filters ▸ Blur ▸ Radial Blur…,
    /// Filters ▸ Other ▸ Offset…), or nil for every other filter. Their frame
    /// is the selection's bounding box on the captured target, so the request
    /// is built from the capture: the host-level arguments translated for its
    /// target (`FilterFrame.kernelParameters`), then its `region_*`.
    func framedByteFilter(_ document: ImageDocument, _ a: [String: Any], filter: String, layer: Int,
                          target: PaintTarget) throws -> String? {
        guard FilterFrame.framedOps.contains(filter) else { return nil }
        let keys = Self.flatFilterArguments[filter] ?? []
        let params = SelectedAdjustment.numericParameters(
            keys.reduce(into: [String: Any]()) { if let value = a[$1] { $0[$1] = value } })
        return try applyScopedFilter(document, target: target, layer: layer, filter: filter) { capture in
            let kernel = try FilterFrame.kernelParameters(filter, params, target: capture.target)
            let request = ByteFilterRequest(
                op: filter, params: capture.framedParameters(kernel, op: filter, for: capture.source))
            return try self.byteFiltered(capture.source, request, in: document)
        }
    }

    /// Admits the request's plan on `image` beside the document's own retained
    /// bytes, then runs it as a cancellable job. A nil core result is the
    /// exact bypass, handed back unchanged so the scoped write reports "no
    /// covered pixels changed" rather than a failure. Throws when there is no
    /// document.
    func byteFiltered(_ image: RasterImage, _ request: ByteFilterRequest, in document: ImageDocument) throws
        -> RasterImage {
        guard let doc = document.doc else { throw ToolError(message: "No document is open.") }
        // `execute` already waited for this call's slot, so `run` does not wait again.
        return try ByteFilterAdmission.run(request, on: image, in: doc, title: "Applying \(request.op)",
                                           waitForSlot: false) ?? image
    }
}
