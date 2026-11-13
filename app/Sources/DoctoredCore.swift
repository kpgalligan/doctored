import AppKit
import UniformTypeIdentifiers
import CryptoKit

// C *constants* (`DR_FILTER_*`, …) may be used anywhere. The agent server's handle lifecycle
// belongs in a `DoctoredCore`-adjacent file, not in feature code.

/// Preserving typed export formats supported by the public high-depth mode.
enum PrecisionExportFormat: UInt32 { case png16 = 0, tiff16 = 1 }

func precisionJSON(_ pointer: UnsafeMutablePointer<CChar>?,
                           _ error: UnsafeMutablePointer<CChar>?) throws -> [String: Any] {
    guard let pointer else {
        throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth inspection failed."))
    }
    defer { dr_string_free(pointer) }
    guard let value = try JSONSerialization.jsonObject(with: Data(String(cString: pointer).utf8))
            as? [String: Any] else {
        throw DoctoredCoreError(message: "Invalid high-depth inspection result.")
    }
    return value
}

/// Immutable preserving owner behind the unified RasterDocument facade. It has
/// no byte surrogate, ImageIO fallback or automatic working-space adoption.
/// Captures retain it independently of any separately generated display preview.
final class RasterPrecisionDocument {
    let ptr: OpaquePointer

    init(owning pointer: OpaquePointer) {
        ptr = pointer
        RasterResources.shared.register(self)
    }
    deinit { dr_precision_doc_free(ptr) }

    static func openPreserving(url: URL, workingLimit: UInt64, otherRetained: UInt64,
                               control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_open(url.path, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not open high-depth input."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    func info() throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_info(ptr, &error)
        return try precisionJSON(result, error)
    }

    /// Scalar storage/resource policy only. Captures must not materialize the
    /// potentially large per-layer inspection tree just to plan a save.
    func descriptor() throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_descriptor(ptr, &error)
        return try precisionJSON(result, error)
    }

    var profileData: Data {
        var count = 0
        guard let bytes = dr_precision_doc_profile(ptr, &count) else { return Data() }
        return withExtendedLifetime(self) { Data(bytes: bytes, count: count) }
    }

    /// True when the profile is an RGB matrix/TRC model passing the high-depth
    /// precision checks: the precondition of calibrated Exposure/White Balance
    /// and of a high-depth conversion's source. False for a LUT profile.
    var profileIsConvertible: Bool { dr_precision_doc_profile_is_convertible(ptr) }

    /// Why a NEW or EDITED `document_v2` `op` cannot evaluate in this profile
    /// — the core's sentence, shown verbatim; nil when admitted. Existing
    /// layers are not judged.
    func calibratedRefusal(op: String) -> String? {
        guard let raw = dr_precision_doc_calibrated_refusal(ptr, op) else { return nil }
        defer { dr_string_free(raw) }
        return String(cString: raw)
    }

    func metadata(_ kind: DrMetadataKind) -> Data? {
        var count = 0
        guard let bytes = dr_precision_doc_metadata(ptr, kind.rawValue, &count), count > 0 else { return nil }
        return withExtendedLifetime(self) { Data(bytes: bytes, count: count) }
    }

    /// The three Phase 11D metadata edits, each the byte twin's contract
    /// verbatim: capture packets from a carrier, one packet stored or
    /// cleared, and the print resolution. They are what lets a host-decoded
    /// or typed-RAW import put the packets and the original PPI it recovered
    /// INTO the typed document, so an export carries them.
    func importingMetadata(from carrier: Data) throws
        -> (document: RasterPrecisionDocument?, warnings: [String]) {
        var error: UnsafeMutablePointer<CChar>?
        var warningJSON: UnsafeMutablePointer<CChar>?
        defer { if let warningJSON { dr_string_free(warningJSON) } }
        let result = carrier.withUnsafeBytes { raw in
            dr_precision_doc_import_metadata(
                ptr, raw.bindMemory(to: UInt8.self).baseAddress, raw.count, &warningJSON, &error)
        }
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "Could not import metadata."))
        }
        let warnings: [String]
        if let warningJSON {
            warnings = (try? JSONSerialization.jsonObject(
                with: Data(String(cString: warningJSON).utf8))) as? [String] ?? []
        } else { warnings = [] }
        return (result.map(RasterPrecisionDocument.init(owning:)), warnings)
    }

    /// nil for no change; a refusal (a packet over 16 MiB) throws the core's
    /// sentence.
    func settingMetadata(_ kind: DrMetadataKind, _ bytes: Data?) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer?
        if let bytes {
            result = bytes.withUnsafeBytes { raw in
                dr_precision_doc_set_metadata(
                    ptr, kind, raw.bindMemory(to: UInt8.self).baseAddress, raw.count, &error)
            }
        } else {
            result = dr_precision_doc_set_metadata(ptr, kind, nil, 0, &error)
        }
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "Could not set metadata."))
        }
        return result.map(RasterPrecisionDocument.init(owning:))
    }

    /// nil for no change; a non-finite or non-positive component throws the
    /// core's sentence.
    func settingResolution(x: Float, y: Float) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_set_resolution(ptr, x, y, &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "Could not set the print resolution."))
        }
        return result.map(RasterPrecisionDocument.init(owning:))
    }

    /// Which of typed record `idx`'s lock bits would block an edit of `kind`
    /// (0 = allowed) — `dr_doc_lock_block`'s twin at high depth, with exactly
    /// its rules. 0 for an out-of-range index and an unrecognized kind.
    func lockBlocking(_ idx: Int, kind: DrEditKind) -> UInt32 {
        guard idx >= 0 else { return 0 }
        return dr_precision_doc_lock_block(ptr, idx, kind)
    }

    /// The record whose OWN bits hold every bit of `locks` — `idx` itself,
    /// else its nearest enclosing group. nil when none does.
    func lockOwner(_ idx: Int, locks: UInt32) -> Int? {
        guard idx >= 0 else { return nil }
        var owner = 0
        return dr_precision_doc_lock_owner(ptr, idx, locks, &owner) ? owner : nil
    }

    func adjustmentDescription(_ index: Int) throws -> String? {
        guard index >= 0 else { return nil }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_adjustment_description(ptr, index, &error)
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth adjustment read failed.")) }
        guard let result else { return nil }
        defer { dr_string_free(result) }
        return String(cString: result)
    }

    func layerSource(_ index: Int) -> RasterPrecisionImage? {
        guard index >= 0 else { return nil }
        return dr_precision_doc_layer_source(ptr, index).map { RasterPrecisionImage(owning: $0) }
    }

    /// Record `index`'s raw u16 mask plane as the Layers panel's opaque gray
    /// RGBA8 thumbnail: fitted to `maxSide` without upscaling, a large
    /// plane box-reduced in the core before any whole-plane image. A group's
    /// mask shows its canvas view instead, as at 8 bits. Throws the
    /// core's sentence for no mask, a bad index, the budget or a cancel.
    func layerMaskThumbnailRGBA8(_ index: Int, maxSide: Int, workingLimit: UInt64,
                                 otherRetained: UInt64,
                                 control: RasterJobControl? = nil) throws -> RasterImage {
        guard index >= 0 else { throw DoctoredCoreError(message: "layer index is out of range") }
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_layer_mask_thumbnail_rgba8(
            ptr, index, UInt32(clamping: maxSide), workingLimit, otherRetained, control?.ptr,
            &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "High-depth mask thumbnail failed."))
        }
        return RasterImage(owning: result)
    }

    /// Alpha channel `index` as the 16-bit Channels panel's opaque gray RGBA8
    /// row thumbnail over the canvas: fitted to `maxSide` without
    /// upscaling, a large plane box-reduced in the core before any
    /// canvas-sized image. Throws the core's sentence for a bad index, the
    /// budget or a cancel.
    func channelThumbnailRGBA8(_ index: Int, maxSide: Int, workingLimit: UInt64,
                               otherRetained: UInt64,
                               control: RasterJobControl? = nil) throws -> RasterImage {
        guard index >= 0 else {
            throw DoctoredCoreError(message: "high-depth channel index is out of range")
        }
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_channel_thumbnail_rgba8(
            ptr, index, UInt32(clamping: maxSide), workingLimit, otherRetained, control?.ptr,
            &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "High-depth channel thumbnail failed."))
        }
        return RasterImage(owning: result)
    }

    /// Record `index`'s mask in its CANVAS view (the plane at its offset, its
    /// outside coverage beyond it) as the 16-bit Channels panel's opaque gray
    /// RGBA8 row thumbnail, fitted like `channelThumbnailRGBA8`.
    /// Throws the core's sentence for no mask, a bad index, the budget or a
    /// cancel.
    func layerMaskCanvasThumbnailRGBA8(_ index: Int, maxSide: Int, workingLimit: UInt64,
                                       otherRetained: UInt64,
                                       control: RasterJobControl? = nil) throws -> RasterImage {
        guard index >= 0 else { throw DoctoredCoreError(message: "layer index is out of range") }
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_layer_mask_canvas_thumbnail_rgba8(
            ptr, index, UInt32(clamping: maxSide), workingLimit, otherRetained, control?.ptr,
            &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "High-depth mask thumbnail failed."))
        }
        return RasterImage(owning: result)
    }

    func render(workingLimit: UInt64, otherRetained: UInt64,
                control: RasterJobControl? = nil) throws -> RasterPrecisionImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_render(ptr, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth rendering failed."))
        }
        return RasterPrecisionImage(owning: result)
    }

    /// Record `index`'s raw pixels on a transparent canvas-sized typed image
    /// in the document's render storage (a group rendered alone); an
    /// adjustment or an out-of-range index throws by name.
    func layerCanvasImage(_ index: Int, workingLimit: UInt64, otherRetained: UInt64,
                          control: RasterJobControl? = nil) throws -> RasterPrecisionImage {
        guard index >= 0 else { throw DoctoredCoreError(message: "layer index is out of range") }
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_layer_canvas_image(ptr, index, workingLimit, otherRetained,
                                                               control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth layer rendering failed."))
        }
        return RasterPrecisionImage(owning: result)
    }

    /// The typed regional render: the projection's window sample for sample, in
    /// the document's render storage. A divisor above 1 is a preview only.
    func renderRegion(_ request: RegionRenderRequest, workingLimit: UInt64, otherRetained: UInt64,
                      control: RasterJobControl? = nil) throws -> RasterPrecisionImage {
        try request.validate()
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_render_region(ptr,
            UInt32(request.x), UInt32(request.y), UInt32(request.width), UInt32(request.height),
            UInt32(request.previewDivisor), workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth region rendering failed."))
        }
        return RasterPrecisionImage(owning: result)
    }

    func editResult(_ pointer: OpaquePointer?, _ error: UnsafeMutablePointer<CChar>?)
        throws -> RasterPrecisionDocument? {
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth edit failed.")) }
        return pointer.map { RasterPrecisionDocument(owning: $0) }
    }

    func duplicateLayer(_ index: Int) throws -> RasterPrecisionDocument? {
        guard index >= 0 else { throw DoctoredCoreError(message: "Invalid high-depth layer index.") }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_duplicate_layer(ptr, index, &error)
        return try editResult(result, error)
    }

    func settingLayerOpacity(_ index: Int, opacity: Float) throws -> RasterPrecisionDocument? {
        guard index >= 0 else { throw DoctoredCoreError(message: "Invalid high-depth layer index.") }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_set_layer_opacity(ptr, index, opacity, &error)
        return try editResult(result, error)
    }

    func addingAdjustment(name: String, description: String, opacity: Float = 1,
                          workingLimit: UInt64, otherRetained: UInt64,
                          control: RasterJobControl? = nil) throws -> RasterPrecisionDocument? {
        try precisionCString(name)
        try precisionCString(description)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_add_adjustment(ptr, name, description, opacity,
            workingLimit, otherRetained, control?.ptr, &error)
        return try editResult(result, error)
    }

    /// `addingAdjustment` at a chosen stack POSITION (12E-2). The core owns
    /// the slot rule, the clip-run join and every refusal
    /// (`core/src/layer_insert.rs`); this wrapper only carries the anchor and
    /// the position code across.
    func addingAdjustment(name: String, description: String, opacity: Float = 1,
                          anchor: Int, position: LayerInsertPosition,
                          workingLimit: UInt64, otherRetained: UInt64,
                          control: RasterJobControl? = nil) throws -> RasterPrecisionDocument? {
        guard anchor >= 0 else {
            throw DoctoredCoreError(message: "Invalid layer index.")
        }
        try precisionCString(name)
        try precisionCString(description)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_add_adjustment_at(ptr, name, description, opacity,
            anchor, position.code, workingLimit, otherRetained, control?.ptr, &error)
        return try editResult(result, error)
    }

    func settingAdjustment(_ index: Int, description: String, workingLimit: UInt64,
                           otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> RasterPrecisionDocument? {
        guard index >= 0 else { throw DoctoredCoreError(message: "Invalid high-depth adjustment index.") }
        try precisionCString(description)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_set_adjustment(ptr, index, description,
            workingLimit, otherRetained, control?.ptr, &error)
        return try editResult(result, error)
    }

    func applyingAdjustment(_ index: Int, description: String, workingLimit: UInt64,
                           otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> RasterPrecisionDocument? {
        guard index >= 0 else { throw DoctoredCoreError(message: "Invalid high-depth raster index.") }
        try precisionCString(description)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_apply_adjustment(ptr, index, description,
            workingLimit, otherRetained, control?.ptr, &error)
        return try editResult(result, error)
    }

    /// nil index means append; otherwise the plan validates the concrete
    /// adjustment/raster target before the host admits any preparation work.
    func adjustmentPlan(index: Int?, description: String, destructive: Bool) throws -> [String: Any] {
        if let index, index < 0 {
            throw DoctoredCoreError(message: "Invalid high-depth adjustment index.")
        }
        try precisionCString(description)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_adjustment_plan(ptr, Int64(index ?? -1), description,
                                                     destructive, &error)
        return try precisionJSON(result, error)
    }

    func colorPlan(profile: Data, convert: Bool, workingLimit: UInt64,
                   otherRetained: UInt64) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = profile.withUnsafeBytes { bytes in
            dr_precision_doc_color_plan(ptr, bytes.bindMemory(to: UInt8.self).baseAddress,
                                        bytes.count, convert, workingLimit, otherRetained, &error)
        }
        return try precisionJSON(result, error)
    }

    func assigningProfile(_ profile: Data, workingLimit: UInt64, otherRetained: UInt64,
                          control: RasterJobControl? = nil) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        let result = profile.withUnsafeBytes { bytes in
            dr_precision_doc_assign_profile(ptr, bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count, workingLimit, otherRetained, control?.ptr, &error)
        }
        return try editResult(result, error)
    }

    func convertingToProfile(_ profile: Data, workingLimit: UInt64, otherRetained: UInt64,
                             control: RasterJobControl? = nil) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        let result = profile.withUnsafeBytes { bytes in
            dr_precision_doc_convert_to_profile(ptr, bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count, workingLimit, otherRetained, control?.ptr, &error)
        }
        return try editResult(result, error)
    }

    func pixelEditPlan(request: String, workingLimit: UInt64,
                       otherRetained: UInt64) throws -> [String: Any] {
        if let retained = try retainedAffineRequest(request) {
            var plan = try retainedSourcePlan(request: retained)
            plan["input_samples"] = 0
            return plan
        }
        try precisionCString(request, maximumBytes: PrecisionPixelCommand.maximumStyleBytes)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_edit_plan(ptr, request, workingLimit, otherRetained, &error)
        return try precisionJSON(result, error)
    }

    func editingPixels(request: String, samples: [UInt16], workingLimit: UInt64,
                       otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> RasterPrecisionDocument? {
        if let retained = try retainedAffineRequest(request) {
            guard samples.isEmpty else { throw DoctoredCoreError(message: "An embedded source transform takes no scalar input.") }
            return try editingRetainedSource(request: retained, workingLimit: workingLimit,
                                            otherRetained: otherRetained, control: control)
        }
        try precisionCString(request, maximumBytes: PrecisionPixelCommand.maximumStyleBytes)
        var error: UnsafeMutablePointer<CChar>?
        let result = samples.withUnsafeBufferPointer { input in
            dr_precision_doc_edit(ptr, request, input.baseAddress, input.count,
                                  workingLimit, otherRetained, control?.ptr, &error)
        }
        return try editResult(result, error)
    }

    /// `RasterDocument.selectedPixelsBounds` at 16 bits.
    func selectedPixelsBounds(layer: Int, coverage: [UInt16],
                              control: RasterJobControl? = nil) throws -> CGRect? {
        var rect = [UInt32](repeating: 0, count: 4)
        var error: UnsafeMutablePointer<CChar>?
        let found = coverage.withUnsafeBufferPointer { samples in
            rect.withUnsafeMutableBufferPointer { out in
                dr_precision_doc_selected_pixels_bounds(ptr, layer, samples.baseAddress,
                    samples.count, control?.ptr, out.baseAddress, &error)
            }
        }
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error,
                fallback: "The selected pixels could not be measured."))
        }
        guard found else { return nil }
        return CGRect(x: Int(rect[0]), y: Int(rect[1]), width: Int(rect[2]), height: Int(rect[3]))
    }

    /// `RasterDocument.layerBounds` at 16/32 bits: record `idx`'s
    /// CONTENT box, `dr_doc_layer_bounds`' typed twin. nil for an
    /// out-of-range index, an adjustment record and nothing opaque.
    func layerBounds(_ idx: Int) -> (x: Int, y: Int, width: Int, height: Int)? {
        guard idx >= 0 else { return nil }
        var xywh = [Int32](repeating: 0, count: 4)
        guard xywh.withUnsafeMutableBufferPointer({
            dr_precision_doc_layer_bounds(ptr, idx, $0.baseAddress)
        }) else { return nil }
        return (Int(xywh[0]), Int(xywh[1]), Int(xywh[2]), Int(xywh[3]))
    }

    /// `RasterDocument.layerAt` at 16/32 bits: the postorder index of
    /// the record a click at canvas `(x, y)` activates, `dr_doc_layer_at`'s
    /// typed twin. nil when nothing is hit.
    func layerAt(x: Int32, y: Int32, topLevel: Bool) -> Int? {
        var idx = 0
        guard dr_precision_doc_layer_at(ptr, x, y, topLevel, &idx) else { return nil }
        return idx
    }

    func readingPlane(request: String, workingLimit: UInt64, otherRetained: UInt64,
                      control: RasterJobControl? = nil) throws -> [UInt16] {
        let plan = try pixelEditPlan(request: request, workingLimit: workingLimit,
                                     otherRetained: otherRetained)
        let count = try PrecisionDocumentDescriptor.integer(plan, "output_samples")
        guard count <= UInt64(RasterImage.maxResizePixels) else {
            throw DoctoredCoreError(message: "High-depth plane exceeds the scalar allocation limit.")
        }
        var output = [UInt16](repeating: 0, count: Int(count))
        var error: UnsafeMutablePointer<CChar>?
        let success = output.withUnsafeMutableBufferPointer { buffer in
            dr_precision_doc_read_plane(ptr, request, buffer.baseAddress, buffer.count,
                                         workingLimit, otherRetained, control?.ptr, &error)
        }
        guard success else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth plane read failed."))
        }
        return output
    }

    func saveNative(to url: URL, workingLimit: UInt64, otherRetained: UInt64,
                    control: RasterJobControl? = nil) throws {
        var error: UnsafeMutablePointer<CChar>?
        guard dr_precision_doc_save_native(ptr, url.path, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth native save failed."))
        }
    }

    /// Reuse an already captured authoritative projection. Core validates its
    /// u16 extent/profile; the editor capture owns its document provenance.
    func projectionExportPlan(_ image: RasterPrecisionImage) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_projection_export_plan(ptr, image.ptr, &error)
        return try precisionJSON(result, error)
    }

    func saveProjection16(_ image: RasterPrecisionImage, to url: URL,
                          format: PrecisionExportFormat, embedProfile: Bool,
                          stripMetadata: Bool, workingLimit: UInt64,
                          otherRetained: UInt64, control: RasterJobControl? = nil) throws -> UInt32 {
        let path = url.path
        try precisionCString(path, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        var report: UInt32 = 0
        guard dr_precision_doc_save_projection16(ptr, image.ptr, path, format.rawValue,
            embedProfile, stripMetadata, workingLimit, otherRetained, &report, control?.ptr,
            &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Captured high-depth projection export failed."))
        }
        return report
    }
}

func precisionCString(_ text: String, maximumBytes: Int = 16 * 1024 * 1024) throws {
    guard text.utf8.count <= maximumBytes, !text.utf8.contains(0) else {
        throw DoctoredCoreError(message: "High-depth text must be bounded and contain no NUL bytes.")
    }
}

/// Exact u16 source or authoritative projection. Byte display conversion is
/// explicit; preserving export pairs this image with its captured document metadata.
final class RasterPrecisionImage {
    let ptr: OpaquePointer
    init(owning pointer: OpaquePointer) {
        ptr = pointer
        RasterResources.shared.register(self)
    }
    deinit { dr_precision_image_free(ptr) }

    var width: Int { Int(dr_precision_image_width(ptr)) }
    var height: Int { Int(dr_precision_image_height(ptr)) }
    /// Opaque storage identity; equal for two images sharing one tile index
    /// (nothing copied); compare while both storages are owned.
    var storageIdentity: UInt64 { dr_precision_image_storage_identity(ptr) }

    /// The borrowed buffer cannot outlive this closure or its immutable owner.
    func withRGBA16<T>(_ body: (UnsafeBufferPointer<UInt16>) throws -> T) throws -> T {
        guard let pixels = dr_precision_image_pixels_rgba16(ptr) else {
            throw DoctoredCoreError(message: "This image has no RGBA16 source.")
        }
        return try withExtendedLifetime(self) {
            try body(UnsafeBufferPointer(start: pixels, count: width * height * 4))
        }
    }

    var profileData: Data {
        var count = 0
        guard let bytes = dr_precision_image_profile(ptr, &count) else { return Data() }
        return withExtendedLifetime(self) { Data(bytes: bytes, count: count) }
    }

    func previewRGBA8(workingLimit: UInt64, otherRetained: UInt64,
                      control: RasterJobControl? = nil) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_image_preview_rgba8(ptr, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth preview failed."))
        }
        return RasterImage(owning: result)
    }

    /// The Layers panel's RGBA8 thumbnail of a u16 image: fitted to
    /// `maxSide` without upscaling, a large image box-reduced in the core
    /// before any whole-layer copy. Float storage refuses.
    func thumbnailRGBA8(maxSide: Int, workingLimit: UInt64, otherRetained: UInt64,
                        control: RasterJobControl? = nil) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_image_thumbnail_rgba8(
            ptr, UInt32(clamping: maxSide), workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth thumbnail failed."))
        }
        return RasterImage(owning: result)
    }
}

/// Error thrown by the Rust core, carrying the message from `err_out`.
struct DoctoredCoreError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// Consumes a Rust-allocated error string and returns its contents.
func takeErrorMessage(_ err: UnsafeMutablePointer<CChar>?, fallback: String) -> String {
    guard let err = err else { return fallback }
    defer { dr_string_free(err) }
    return String(cString: err)
}

/// Owning Swift wrapper around a `DrImage *` handle from the Rust core.
/// Handles are immutable: every operation returns a new handle.
final class RasterImage {
    let ptr: OpaquePointer

    init(owning pointer: OpaquePointer) {
        self.ptr = pointer
        RasterResources.shared.register(self)
    }

    deinit {
        dr_image_free(ptr)
    }

    static func open(url: URL) throws -> RasterImage {
        var err: UnsafeMutablePointer<CChar>? = nil
        guard let handle = dr_image_open(url.path, &err) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not open \(url.lastPathComponent)."))
        }
        return RasterImage(owning: handle)
    }

    /// Wraps an in-memory STRAIGHT-alpha (non-premultiplied) RGBA8 buffer,
    /// row 0 = top, exactly `width * height * 4` bytes — the same convention
    /// as `setLayerContent(_:rgba:width:height:offsetX:offsetY:mask:preservingMask:)`,
    /// and the way pixels the core cannot decode itself (a HEIC still, a Live
    /// Photo video frame) become an image without a round trip through a
    /// temporary file. nil for a bad size or a buffer whose length disagrees
    /// with it.
    static func from(rgba pixels: [UInt8], width: Int, height: Int) -> RasterImage? {
        guard width > 0, height > 0, pixels.count == width * height * 4,
              width * height <= maxResizePixels
        else { return nil }
        return pixels.withUnsafeBufferPointer { buffer in
            dr_image_from_rgba(buffer.baseAddress, UInt32(width), UInt32(height))
                .map { RasterImage(owning: $0) }
        }
    }

    var width: Int { Int(dr_image_width(ptr)) }
    var height: Int { Int(dr_image_height(ptr)) }
    var pixelSize: NSSize { NSSize(width: width, height: height) }

    func wrap(_ result: OpaquePointer?) -> RasterImage? {
        result.map { RasterImage(owning: $0) }
    }

    func rotated90() -> RasterImage? { wrap(dr_image_rotate90(ptr)) }
    func rotated180() -> RasterImage? { wrap(dr_image_rotate180(ptr)) }
    func rotated270() -> RasterImage? { wrap(dr_image_rotate270(ptr)) }
    func flippedH() -> RasterImage? { wrap(dr_image_flip_horizontal(ptr)) }
    func flippedV() -> RasterImage? { wrap(dr_image_flip_vertical(ptr)) }

    func cropped(x: Int, y: Int, w: Int, h: Int) -> RasterImage? {
        guard x >= 0, y >= 0, w > 0, h > 0, x + w <= width, y + h <= height else { return nil }
        return wrap(dr_image_crop(ptr, UInt32(x), UInt32(y), UInt32(w), UInt32(h)))
    }

    /// Upper bound on resize targets, mirroring the core's contract
    /// (`dr_image_resize` returns NULL above w*h = 100,000,000).
    static let maxResizePixels = 100_000_000

    func resized(w: Int, h: Int, filter: DrResizeFilter) -> RasterImage? {
        guard w > 0, h > 0, w * h <= Self.maxResizePixels else { return nil }
        return wrap(dr_image_resize(ptr, UInt32(w), UInt32(h), filter))
    }

    func adjusted(brightness: Double, contrast: Double, saturation: Double) -> RasterImage? {
        wrap(dr_image_adjust(ptr, Float(brightness), Float(contrast), Float(saturation)))
    }

    func grayscaled() -> RasterImage? { wrap(dr_image_grayscale(ptr)) }
    func inverted() -> RasterImage? { wrap(dr_image_invert(ptr)) }
    func sepia() -> RasterImage? { wrap(dr_image_sepia(ptr)) }
    func sharpened(amount: Double) -> RasterImage? { wrap(dr_image_sharpen(ptr, Float(amount))) }
    /// The byte twin of the 16-bit selected filters of the same names, through
    /// one core dispatch: `blur` (Gaussian Blur, key `sigma`, never a
    /// bypass), `box_blur`, `surface_blur`, `add_grain`,
    /// `radial_blur`, `clouds`, `difference_clouds`, `offset`, `motion_blur`,
    /// `lens_blur`, `tilt_shift`, `lens_correction`, `presence`, `develop_filter`, `smart_sharpen`, `liquify`, `unsharp_mask`,
    /// `high_pass`, `dust_and_scratches` and `reduce_noise`.
    /// A nil result with no thrown error is an exact bypass (a zero grain
    /// amount; a zero offset; radius 0 or amount 0 for the sharpeners; a radius below one
    /// pixel or a threshold of 1 for Dust & Scratches; a zero radius or both
    /// halves off for Reduce Noise; liquify strokes that move nothing), so the
    /// caller adds no undo step; every refusal explains itself. `control` (single-use) makes the preview and
    /// the Apply job cancellable; a cancel throws `operation cancelled`. With `evaluation` (the
    /// document this image is a layer of) it is `dr_doc_filter_op`: `liquify` follows that
    /// document's evaluation policy, and every other op is the same bytes.
    func filterOp(_ op: String, params: [String: Any], control: RasterJobControl? = nil,
                  evaluation: RasterDocument? = nil) throws -> RasterImage? {
        let json = try Self.filterParamsJSON(params)
        var error: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer?
        if let evaluation {
            result = dr_doc_filter_op(evaluation.ptr, ptr, op, json, control?.ptr, &error)
        } else {
            result = dr_image_filter_op(ptr, op, json, control?.ptr, &error)
        }
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not apply \(op).")) }
        return result.map { RasterImage(owning: $0) }
    }
    /// dr_image_filter_op_scratch_bytes: `filterOp`'s planned peak
    /// scratch for the same op and params on this image. 0 is an exact bypass.
    /// Throws the core's refusal, the one `filterOp` would make.
    func filterOpScratchBytes(_ op: String, params: [String: Any]) throws -> UInt64 {
        let json = try Self.filterParamsJSON(params)
        var error: UnsafeMutablePointer<CChar>?
        let scratch = dr_image_filter_op_scratch_bytes(ptr, op, json, &error)
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not plan \(op).")) }
        return scratch
    }
    /// The params object both filter-op calls send.
    private static func filterParamsJSON(_ params: [String: Any]) throws -> String {
        guard JSONSerialization.isValidJSONObject(params),
              let data = try? JSONSerialization.data(withJSONObject: params, options: [.sortedKeys]),
              let json = String(data: data, encoding: .utf8)
        else { throw DoctoredCoreError(message: "Filter parameters must be finite JSON values.") }
        return json
    }
    func hueRotated(degrees: Double) -> RasterImage? {
        wrap(dr_image_hue_rotate(ptr, Float(degrees)))
    }

    func thresholded(level: Double) -> RasterImage? {
        wrap(dr_image_threshold(ptr, Float(level)))
    }

    func posterized(levels: Int) -> RasterImage? {
        guard levels >= 2, levels <= 64 else { return nil }
        return wrap(dr_image_posterize(ptr, UInt32(levels)))
    }

    func pixelated(block: Int) -> RasterImage? {
        guard block >= 1, block <= 1024 else { return nil }
        return wrap(dr_image_pixelate(ptr, UInt32(block)))
    }

    func noised(amount: Double, seed: UInt64) -> RasterImage? {
        wrap(dr_image_noise(ptr, Float(amount), seed))
    }

    func edgeDetected() -> RasterImage? { wrap(dr_image_edge_detect(ptr)) }
    func embossed() -> RasterImage? { wrap(dr_image_emboss(ptr)) }
}

extension RasterImage {
    /// The straight (non-premultiplied) RGBA8 buffer IN PLACE, for a core op
    /// that takes a canvas-sized plate beside its overlay. Nothing is copied
    /// and nothing is converted; the pointer is valid for the call.
    /// nil for an empty handle.
    func withPlateBytes<T>(_ body: (UnsafePointer<UInt8>) throws -> T) rethrows -> T? {
        guard width > 0, height > 0, let pixels = dr_image_pixels_rgba(ptr) else { return nil }
        return try body(pixels)
    }
}

/// Owning Swift wrapper around a `DrDocument *` handle from the Rust core:
/// a canvas size plus a bottom-first layer stack. Handles are immutable;
/// every operation returns a new handle. Handles share unchanged layer pixel
/// buffers (copy-on-write), so clones and per-layer edits are cheap and the
/// undo stack can keep whole-document snapshots.
final class RasterDocument {
    /// Exactly one native owner; typed documents never carry a byte surrogate.
    let ptr: OpaquePointer?
    let precision: RasterPrecisionDocument?
    let precisionMetadata: PrecisionFacadeMetadata?
    var isHighDepth: Bool { precision != nil }
    var precisionInfo: [String: Any]? { precisionMetadata?.raw }

    /// The typed open's own profile refusal: the header report's
    /// sentence for `url` (`flatOpenProfileRefusal(at:)`) when this typed
    /// document's `import_warnings` carry it word for word — the typed Gray
    /// conversion words its warning through the same core sentence — else
    /// nil, so a sentence the typed open did not say is never shown.
    func precisionProfileRefusal(openedFrom url: URL) -> String? {
        if isCmyk { return CmykDocument.assignedSentence(precisionInfo) } // #1
        guard let warnings = precisionInfo?["import_warnings"] as? [String], !warnings.isEmpty,
              let sentence = RasterDocument.flatOpenProfileRefusal(at: url),
              warnings.contains(sentence)
        else { return nil }
        return sentence
    }

    /// `iccProfile`'s memo: the outer optional is "not read yet", the inner
    /// one the read's own answer. Guarded by `iccLock` — a handle is read
    /// from the main thread and from `PreviewRenderer`'s queue, and while
    /// the CORE is happy with that (its ops are pure reads), a Swift
    /// property written from both is a data race.
    var cachedICCProfile: Data??

    /// Serializes the memo above. One shared lock rather than one per
    /// document: it is held for a pointer copy, and the contended case is
    /// two threads asking about the same document anyway.
    static let iccLock = NSLock()

    init(owning pointer: OpaquePointer) {
        self.ptr = pointer
        self.precision = nil
        self.precisionMetadata = nil
        RasterResources.shared.register(self)
    }

    init(precision: RasterPrecisionDocument) throws {
        let metadata = try PrecisionFacadeMetadata.capture(precision)
        self.ptr = nil
        self.precision = precision
        self.precisionMetadata = metadata
        RasterResources.shared.register(self)
    }

    private init(precision: RasterPrecisionDocument, metadata: PrecisionFacadeMetadata) {
        self.ptr = nil
        self.precision = precision
        self.precisionMetadata = metadata
        RasterResources.shared.register(self)
    }

    deinit { if let ptr { dr_doc_free(ptr) } }

    static func open(url: URL, control: RasterJobControl? = nil) throws -> RasterDocument {
        if try isPrecisionNative(at: url) {
            let budget = try PrecisionWorkContext()
            return try RasterDocument(precision: RasterPrecisionDocument.openPreserving(
                url: url, workingLimit: budget.limit, otherRetained: budget.otherRetained,
                control: control))
        }
        var err: UnsafeMutablePointer<CChar>? = nil
        guard let handle = dr_doc_open(url.path, control?.ptr, &err) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not open \(url.lastPathComponent)."))
        }
        return RasterDocument(owning: handle)
    }

    /// True when opening `url` would preserve whatever EXIF, XMP and IPTC
    /// the file holds: a JPEG, a PNG or a native `.rz`. False for every
    /// other container — a TIFF, WebP, GIF, BMP or PSD arrives as pixels and
    /// drops its capture data, which such a file really can carry — and for
    /// a file that cannot be read.
    static func metadataWalked(at url: URL) -> Bool {
        dr_path_metadata_walked(url.path)
    }

    /// Wraps an image as a single-"Background"-layer document.
    static func from(image: RasterImage) -> RasterDocument? {
        dr_doc_from_image(image.ptr).map { RasterDocument(owning: $0) }
    }

    /// An 8-bit document of one canvas-sized layer filled with `rgba`
    /// (straight, row 0 = top) — File ▸ New…'s byte half. Composed from the
    /// two calls above rather than a new export, because a blank canvas is a
    /// buffer of constant pixels and nothing the core needs to know about.
    /// nil for a size past the pixel cap.
    static func blank(width: Int, height: Int, rgba: (UInt8, UInt8, UInt8, UInt8)) -> RasterDocument? {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else { return nil }
        let pixels = [UInt8](unsafeUninitializedCapacity: width * height * 4) { buffer, count in
            for index in stride(from: 0, to: width * height * 4, by: 4) {
                buffer[index] = rgba.0
                buffer[index + 1] = rgba.1
                buffer[index + 2] = rgba.2
                buffer[index + 3] = rgba.3
            }
            count = width * height * 4
        }
        guard let image = RasterImage.from(rgba: pixels, width: width, height: height) else { return nil }
        return from(image: image)
    }

    func clone() -> RasterDocument? {
        if let precision, let precisionMetadata { return RasterDocument(precision: precision, metadata: precisionMetadata) }
        return wrap(dr_doc_clone(ptr))
    }

    var width: Int { precisionMetadata?.width ?? Int(dr_doc_width(ptr)) }
    var height: Int { precisionMetadata?.height ?? Int(dr_doc_height(ptr)) }
    var canvasSize: NSSize { NSSize(width: width, height: height) }
    var layerCount: Int { precisionMetadata?.layers.count ?? Int(dr_doc_layer_count(ptr)) }

    /// Value snapshot of one ENTRY's metadata (index 0 = bottom entry). An
    /// entry is a raster layer or a GROUP; the structure fields are read here
    /// rather than one call at a time so the layers panel's per-row FFI count
    /// does not grow with the feature.
    struct LayerInfo {
        /// The core's stable per-layer identity (`dr_doc_layer_id`): unique
        /// while the app runs, kept through a rename, a reorder, an opacity
        /// change, a geometry op and undo/redo, fresh for every copy a
        /// duplicate makes, and gone when the layer is. It is what lets a
        /// recorded Action name the layer a user pointed at rather than the
        /// row number it happened to sit on — names are not unique and
        /// indices renumber on every insert, delete, group and merge.
        ///
        /// Persisted in `.rz` since 12E-1 (unlike `ChannelInfo.id`, which is
        /// session-scoped), so a reopened document hands the same ids back;
        /// an id from an earlier launch is honoured only when the recorded
        /// name also matches (`ActionIdentity`).
        let id: UInt64
        let name: String
        let opacity: Double
        let blendMode: DrBlendMode
        let visible: Bool
        /// The PIXEL BUFFER rect: a raster layer's offset and dimensions,
        /// and for a group — which has no buffer of its own — the union of
        /// its raster descendants' buffer rects. The CONTENT box (the opaque
        /// pixels) is `layerBounds`, which is a different rectangle and is
        /// asked for by name.
        let offsetX: Int
        let offsetY: Int
        let width: Int
        let height: Int
        let kind: DrLayerKind
        /// 0 at the top level, one more inside each enclosing group.
        let depth: Int
        /// A `DrLockFlags` bitmask; see `LayerLocks.swift`.
        let locks: UInt32
        /// Link-group id; 0 when unlinked.
        let link: UInt32
        /// Whether a GROUP is shown expanded. Always true on a raster entry.
        let open: Bool

        var isGroup: Bool { kind == DR_LAYER_GROUP }
    }

    func isValidIndex(_ idx: Int) -> Bool {
        idx >= 0 && idx < layerCount
    }

    func wrap(_ result: OpaquePointer?) -> RasterDocument? {
        result.map { RasterDocument(owning: $0) }
    }

    func wrapImage(_ result: OpaquePointer?) -> RasterImage? {
        result.map { RasterImage(owning: $0) }
    }

    func layerInfo(_ idx: Int) -> LayerInfo? {
        if let precisionMetadata { return precisionMetadata.layer(idx)?.info }
        guard isValidIndex(idx) else { return nil }
        let i = idx
        var name = ""
        if let cName = dr_doc_layer_name(ptr, i) {
            name = String(cString: cName)
            dr_string_free(cName)
        }
        return LayerInfo(
            id: dr_doc_layer_id(ptr, i),
            name: name,
            opacity: Double(dr_doc_layer_opacity(ptr, i)),
            blendMode: dr_doc_layer_blend_mode(ptr, i),
            visible: dr_doc_layer_visible(ptr, i),
            offsetX: Int(dr_doc_layer_offset_x(ptr, i)),
            offsetY: Int(dr_doc_layer_offset_y(ptr, i)),
            width: Int(dr_doc_layer_width(ptr, i)),
            height: Int(dr_doc_layer_height(ptr, i)),
            kind: dr_doc_layer_is_group(ptr, i) ? DR_LAYER_GROUP : DR_LAYER_RASTER,
            depth: Int(dr_doc_layer_depth(ptr, i)),
            locks: dr_doc_layer_locks(ptr, i),
            link: dr_doc_layer_link(ptr, i),
            open: dr_doc_layer_open(ptr, i))
    }

    /// Entry `idx` as a RECORDABLE reference: its display name and its stable
    /// id together.
    ///
    /// The pair is the whole identity design in one value — an Action's
    /// `{"$layer": {"name": …, "id": …}}` symbol resolves by id when it can
    /// and falls back to the name when it cannot (a batch's next file, a
    /// document reopened since the recording), so a step aims at the layer the
    /// user pointed at without ever failing merely because the session ended.
    func layerRef(_ idx: Int) -> LayerRef? {
        guard let info = layerInfo(idx) else { return nil }
        return LayerRef(name: info.name, id: info.id)
    }

    /// Channel `i` as the same recordable pair. Channels already carried a
    /// stable id (`dr_doc_channel_id`); this is only the spelling the Action
    /// symbol takes.
    func channelRef(_ i: Int) -> ChannelRef? {
        guard let info = channelInfo(i) else { return nil }
        return ChannelRef(name: info.name, id: info.id)
    }

    /// The index the layer with this identity currently sits at, or nil when
    /// no entry carries it — it was deleted, merged away, or the id was minted
    /// in an earlier launch.
    ///
    /// A linear walk rather than a core call: the reverse lookup is not
    /// exported (see `ffi_identity`'s module doc), the stacks this runs over
    /// are tens of entries, and it is asked once per symbol per step.
    func layerIndex(id: UInt64) -> Int? {
        guard id != 0 else { return nil }
        for index in 0..<layerCount where layerInfo(index)?.id == id { return index }
        return nil
    }

    /// Copy of a layer's pixels at the layer's own size.
    func layerImage(_ idx: Int) -> RasterImage? {
        guard isValidIndex(idx) else { return nil }
        return wrapImage(dr_doc_layer_image(ptr, idx))
    }

    /// Layer `idx`'s own straight RGBA8 bytes (width*height*4, row 0 = top),
    /// read back verbatim through dr_doc_layer_image — the pixel twin of
    /// `layerMaskCoverage`, for the byte-exact refits described layers make
    /// (no CoreGraphics round trip, so no premultiply loses low bits). nil
    /// for an out-of-range index or an empty layer.
    func layerPixels(_ idx: Int) -> [UInt8]? {
        guard isValidIndex(idx), let image = dr_doc_layer_image(ptr, idx) else {
            return nil
        }
        defer { dr_image_free(image) }
        let width = Int(dr_image_width(image))
        let height = Int(dr_image_height(image))
        guard width > 0, height > 0, let pixels = dr_image_pixels_rgba(image) else {
            return nil
        }
        return Array(UnsafeBufferPointer(start: pixels, count: width * height * 4))
    }

    /// A layer's own pixels on a transparent canvas-sized image, placed at
    /// its offset — `flattened()` for a single layer, and what Copy puts on
    /// the clipboard. Opacity, blend mode, visibility and the mask are
    /// ignored: they say how the layer composites, not what its pixels are.
    func layerCanvasImage(_ idx: Int) -> RasterImage? {
        guard isValidIndex(idx) else { return nil }
        return wrapImage(dr_doc_layer_canvas_image(ptr, idx))
    }

    /// Aspect-fit thumbnail of a layer, longest side == maxSide.
    func layerThumbnail(_ idx: Int, maxSide: Int) -> RasterImage? {
        guard isValidIndex(idx), maxSide > 0 else { return nil }
        if let precision, isCmyk, let context = try? PrecisionWorkContext(source: precision) {
            return try? precision.cmykLayerThumbnail(idx, maxSide: maxSide, workingLimit: context.limit,
                                                     otherRetained: context.otherRetained) // #1
        }
        if let precision {
            guard let source = precision.layerSource(idx) else { return nil }
            do {
                let context = try PrecisionWorkContext(image: source)
                // A float source has no direct RGBA8 form: its thumbnail goes
                // through the document's own tone map (Phase 11F), fitted in
                // the core before any whole-layer copy.
                if precision.isFloat {
                    return try precision.displayThumbnailRGBA8(
                        of: source, maxSide: maxSide, workingLimit: context.limit,
                        otherRetained: context.otherRetained)
                }
                if isDuotone {
                    return try DuotoneDocument.thumbnail(
                        of: source, precision: precision, maxSide: maxSide, context: context)
                }
                if isLab { return try LabDocument.thumbnail(of: source, maxSide: maxSide, context: context) }
                // The core fits a large layer before any whole-layer copy;
                // a small one keeps the preview-then-fit bytes.
                return try source.thumbnailRGBA8(maxSide: maxSide, workingLimit: context.limit,
                                                 otherRetained: context.otherRetained)
            } catch { return nil }
        }
        return wrapImage(dr_doc_layer_thumbnail(ptr, idx, UInt32(maxSide)))
    }

    /// Opaque identity of layer `idx`'s pixel storage; 0 when there is none.
    func layerStorageIdentity(_ idx: Int) -> UInt64 {
        guard isValidIndex(idx) else { return 0 }
        if let precision, isCmyk { return precision.cmykLayerIdentity(idx) } // #1
        if let precision { return precision.layerSource(idx)?.storageIdentity ?? 0 }
        return dr_doc_layer_storage_identity(ptr, idx)
    }

    /// Opaque identity of layer `idx`'s raw mask storage; 0 for no
    /// mask, a group or a bad index.
    func layerMaskStorageIdentity(_ idx: Int) -> UInt64 {
        guard isValidIndex(idx) else { return 0 }
        if let precision {
            return dr_precision_doc_layer_mask_storage_identity(precision.ptr, idx)
        }
        return dr_doc_layer_mask_storage_identity(ptr, idx)
    }

    /// Canvas-sized projection of the visible layers — the preview/readout
    /// convenience, which answers nil without a reason. A path that must say
    /// why a projection failed calls `render()` instead.
    func flattened() -> RasterImage? { try? render() }

    /// Complete projection or an actionable error; no partial pixels cross FFI.
    /// A reduced budget supports qualification with small fixtures.
    func render(groupBufferPixels: UInt64? = nil, control: RasterJobControl? = nil) throws -> RasterImage {
        guard !isHighDepth else { throw DoctoredCoreError(message: "High-depth pixels require an authoritative typed projection.") }
        // argv-style defaults keep qualification limits local to a test instance.
        let budget = groupBufferPixels ?? UInt64(max(0,
            UserDefaults.standard.integer(forKey: "RenderGroupPixelBudget")))
        var err: UnsafeMutablePointer<CChar>?
        guard let handle = dr_doc_flattened(ptr, budget, control?.ptr, &err) else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Could not render document."))
        }
        return RasterImage(owning: handle)
    }

    /// The 8-bit regional render: the projection's window sample for sample.
    /// A divisor above 1 is a preview only.
    func renderRegion(_ request: RegionRenderRequest, maxOutputBytes: UInt64,
                      control: RasterJobControl? = nil) throws -> RasterImage {
        guard !isHighDepth else { throw DoctoredCoreError(message: "High-depth pixels require an authoritative typed projection.") }
        try request.validate()
        var err: UnsafeMutablePointer<CChar>?
        guard let handle = dr_doc_render_region(ptr,
            UInt32(request.x), UInt32(request.y), UInt32(request.width), UInt32(request.height),
            UInt32(request.previewDivisor), maxOutputBytes, control?.ptr, &err) else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Could not render the region."))
        }
        return RasterImage(owning: handle)
    }
}

extension Int {
    func clamped(_ low: Int, _ high: Int) -> Int {
        Swift.min(Swift.max(self, low), high)
    }
}

/// Process-wide core state the host sets once at launch.
enum DoctoredCoreProcess {
    /// The working limit core uses wherever no caller limit is passed
    /// (internal constructors, plan defaults, the native reopen check, the
    /// PSD layered-import envelope). False for 0, which leaves it unchanged.
    @discardableResult
    static func setWorkingLimit(_ bytes: UInt64) -> Bool {
        dr_set_process_working_limit(bytes)
    }
}

/// Atomic cancellation/progress may be queried on main while Rust runs on the worker.
/// The operation closure retains this owner until its final checkpoint returns.
final class RasterJobControl {
    let ptr: OpaquePointer
    init() throws {
        guard let ptr = dr_job_control_create() else {
            throw DoctoredCoreError(message: "Could not allocate job control.")
        }
        // The whole working limit lets core `reserve` say when closing other
        // documents cannot help. A fresh token cannot refuse it.
        _ = dr_job_control_set_working_limit(ptr, RasterResources.shared.workingLimit)
        self.ptr = ptr
    }
    deinit { dr_job_control_free(ptr) }
    private let childLock = NSLock()
    private var activeChild: RasterJobControl?
    private var configuredLimit: UInt64?
    private var progressFloor: Double = 0
    private var childRange: ClosedRange<Double> = 0...1
    func cancel() {
        dr_job_control_cancel(ptr)
        childLock.lock(); let child = activeChild; childLock.unlock()
        child?.cancel()
    }
    var progress: Double {
        childLock.lock(); let child = activeChild; let range = childRange
        let floor = progressFloor; childLock.unlock()
        let value = child.map { range.lowerBound + $0.progress * (range.upperBound - range.lowerBound) }
            ?? dr_job_control_progress(ptr)
        childLock.lock(); progressFloor = max(progressFloor, value); let result = max(floor, progressFloor); childLock.unlock()
        return result
    }
    var stage: UInt32 {
        childLock.lock(); let child = activeChild; childLock.unlock()
        return child?.stage ?? dr_job_control_stage(ptr)
    }
    /// Consume the parent at the core boundary before dispatching child stages.
    /// Reusing it for another preparation or direct core call must still refuse.
    func beginCompoundOperation() throws {
        var error: UnsafeMutablePointer<CChar>?
        guard dr_job_control_begin_compound(ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error,
                fallback: "This operation requires a fresh job control."))
        }
    }
    /// Sequential compound work retains a distinct single-use child per core
    /// call. The UI cancels this parent; forwarding is synchronized with workers.
    func withChildOperation<T>(from start: Double = 0, to end: Double = 1,
                               _ body: (RasterJobControl) throws -> T) throws -> T {
        let child = try RasterJobControl()
        childLock.lock()
        let limit = configuredLimit
        activeChild = child
        childRange = start...end
        progressFloor = max(progressFloor, start)
        childLock.unlock()
        defer {
            childLock.lock(); activeChild = nil; progressFloor = max(progressFloor, end); childLock.unlock()
        }
        if let limit, !child.setScratchLimit(limit) {
            throw DoctoredCoreError(message: "Could not configure operation stage memory.")
        }
        if isCancelled { child.cancel() }
        let result = try body(child)
        guard !isCancelled else { throw JobCancelled() }
        return result
    }
    var isCancelled: Bool { dr_job_control_cancelled(ptr) }
    var scratchBytes: UInt64 {
        childLock.lock(); let child = activeChild; childLock.unlock()
        return max(child?.scratchBytes ?? 0, dr_job_control_scratch_bytes(ptr))
    }
    func setScratchLimit(_ bytes: UInt64) -> Bool {
        guard dr_job_control_set_scratch_limit(ptr, bytes) else { return false }
        childLock.lock(); configuredLimit = bytes; childLock.unlock()
        return true
    }
}
