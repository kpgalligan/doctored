import Foundation

// Layer ▸ Place Embedded…/Place Linked… of a camera RAW file lands a retained RAW layer of
// its development (measured against Photoshop, `float-raw-602` takes 1–2: 300 ppi at 100 %, R-SDR
// at 32 bits, the sidecar honoured; `float-host-602/take2`: a 300 ppi file placed at 24 % in a
// 72 ppi document).
// The u16 development is the master in an 8- or 16-bit document (`convert_fallback`, as the open's
// 8-bit choice keeps it), the camera file is embedded beside it (`attachingRawSource`), and a
// linked layer keeps its link through every later redevelopment (`RawSourceWork.replacing`,
// `RawLocalEvaluation.replacing`). In a 32-bit float document the place develops the 32-bit float
// (linear) master instead (`defaultEncoding`), unless the request names a control that only
// the 16-bit development has or asks for `sdr16`.

/// The camera-RAW branch of `LinkedImageWork.importImage`: one snapshot, one
/// develop, one source edit plus attach, one undo step.
///
/// # Measurement record
/// Harness `~/devel/doctored-fixtures/photoshop/place-raw-652/admission/measure.py`, the method
/// of `float-raw-602/admission` (`RawSourceWork.scratch`'s record): Release app, one instance per
/// measured `place_image`, `MallocLargeCache=0`, `DR_WORKING_LIMIT_BYTES` 8 GiB, an 8 s settle,
/// E = interval max phys_footprint − pre around the one call, 3 runs plus a `list_documents`
/// baseline twin. `d24.dng` (`synthetic_dng(path, 4000, 6000)`, orientation 6: develops 6000 ×
/// 4000, 48,001,004 B, bf77e22d…) placed with `raw: {}` into 6000 × 4000 transparent targets at
/// the file's reported 150 ppi (P8, P16, P32) and at 225 ppi (P32s, 150 %). Figures: decode =
/// `RawSourceWork.scratch` 2,655,165,380; render = the place plan's `scratch_bytes` (P8
/// 676,195,775, P16 580,194,304, P32 and P32s 2,165,830,692); scale (P32s) = the `transform`
/// plan's 3,907,288,365. Max E (charge ÷ max E): whole call P8 1,394,033,504 (decode 1.905),
/// P16 1,441,448,800 (1.842), P32 1,268,106,392 (decode 2.094, render 1.708), P32s 3,009,759,680
/// (scale 1.298). Stages the whole call exceeds, measured alone (`core_p`, the inputs held):
/// render P8 96,403,528 (7.014), P16 48,136,216 (12.053), P32s 386,875,680 (5.598); decode by the
/// 16-bit open of the same file, 1,395,131,184 (1.903). Every stage ≥ 1.25×; no constant moved.
/// Baseline 16,384 / 16,384 / 0 in the full run (a lone page), 0 / 0 / 0 re-run alone.
enum RawSourcePlacement {
    static func claims(_ url: URL) -> Bool { RawImage.isRawFile(url) }

    /// The encoding a place develops when the request names none.
    /// A 32-bit float
    /// document takes the float development, so the layer holds what the
    /// document can: highlights above white. A tone curve develops float (the
    /// decoder's curve under the extended range,
    /// `RawOutputEncoding`), and grain since #4 (on the display encoding,
    /// `RawGrain.swift`). The default is never written into the recorded
    /// step: a replay follows its own document's depth.
    static func defaultEncoding(documentIsFloat: Bool) -> RawOutputEncoding {
        documentIsFloat ? .linearFloat : .normalized16
    }

    /// Returns the sidecar's joined disclosures, or nil; nil also when nothing
    /// was placed (a cancelled dialog).
    @discardableResult
    static func importRaw(in document: ImageDocument, url: URL, name: String,
                          x: Int, y: Int, storage: ImageSourceStorage, raw: RawImportMode,
                          insertion: LayerInsertion?,
                          record: [ActionStep]) throws -> String? {
        let capture = try ImageSourceCapture(document)
        let source = capture.source
        guard let probe = RawImage.inspect(url: url) else {
            throw DoctoredCoreError(message: "Choose a camera RAW file Core Image can develop.")
        }
        if let reason = RawImage.oversizeReason(probe.pixelSize) {
            throw RawImportError(url: url, reason: reason)
        }
        let count = try RawSourceSnapshot.size(url)
        let budget = try RawSourceWork.scratch(width: Int(probe.pixelSize.width),
            height: Int(probe.pixelSize.height), encoded: count)
        _ = try RasterResources.shared.admit(scratch: budget)
        let snapshot = try BackgroundWork.wait(try WorkerTask(scratchEstimate: budget) {
            try RawSourceSnapshot.capture(url, maximumBytes: count)
        }, title: "Capturing Camera RAW")
        guard let captureProbe = RawImage.inspect(url: snapshot.url),
            captureProbe.pixelSize == probe.pixelSize else {
            throw RawImportError(url: url, reason: "the source changed while it was being captured")
        }
        let captured = try RetainedRawDecoder.capture(url: snapshot.url,
            filename: url.lastPathComponent, sha256: snapshot.sha256)

        let sidecar = RawSidecar.read(beside: url)
        let base = sidecar?.settings ?? RawDevelopSettings()
        let notes = sidecar?.disclosures ?? []
        let effective: RawDevelopSettings
        switch raw {
        case .headless(let patch):
            effective = base.overlaid(by: patch)
        case .ask:
            var seeded = captured
            if !base.isAsShot { seeded.recipe = try captured.recipe.updating(base) }
            guard let chosen = RawDevelopWindowController.runPlace(captureProbe, descriptor: seeded,
                snapshot: snapshot, verb: "Place",
                documentIsEightBit: !source.isHighDepth,
                documentIsFloat: source.isFloat) else { return nil }
            effective = base.overlaid(by: chosen)
        }
        // The default is computed after `effective` and kept out of it: the
        // recorded step below carries an encoding only when the request or
        // the window named one.
        var develop = effective
        if source.isFloat, develop.outputEncoding == nil {
            develop.outputEncoding = defaultEncoding(documentIsFloat: true)
        }
        try RawSourceWork.requireFloatDocument(develop, in: source)
        var desired = captured
        desired.recipe = try captured.recipe.updating(develop)
        try RawSourceWork.requireFloatDocument(desired.recipe, in: source)
        let request = desired
        // The capture was admitted at the 16-bit figure; the float development
        // is charged at its own.
        let developBudget = try RawSourceWork.scratch(width: Int(probe.pixelSize.width),
            height: Int(probe.pixelSize.height), encoded: count, float: desired.recipe.isLinearFloat)
        let whiteBalanceChanged = desired.recipe.values.customWhiteBalance
            && (desired.recipe.values.temperature != captured.recipe.values.temperature
                || desired.recipe.values.tint != captured.recipe.values.tint)
        _ = try RasterResources.shared.admit(scratch: developBudget)
        let developed = try BackgroundWork.wait(try WorkerTask(scratchEstimate: developBudget) {
            try RetainedRawDecoder.develop(url: snapshot.url, descriptor: request,
                                           maximumScratch: developBudget,
                                           whiteBalanceChanged: whiteBalanceChanged)
        }, title: "Developing Camera RAW")
        try capture.requireCurrent(document)

        let revisionID = UUID().uuidString
        let placement = insertion ?? (document.activeLayerIndex, LayerInsertPosition.above)
        let target = placement.position.landingIndex(in: source, anchor: placement.anchor)
        let command = try PrecisionPixelCommand(["op": "place", "revision_id": revisionID,
            "depth_conversion": ImageSourceDepthConversion.convertFallback.rawValue,
            "link": storage == .linked
                ? ["path": url.standardizedFileURL.path, "sha256": snapshot.sha256]
                : NSNull(),
            "source_id": revisionID, "name": name, "x": x, "y": y,
            "anchor": placement.anchor, "position": placement.position.rawValue])
        // The resolution scale lands as Free Transform's commit does: a
        // non-interactive `transform` about (x, y), whose sampling the job
        // control bounds. A place request's own
        // `source_transform` was once held to the interactive sampling ceiling
        // (`MAX_WORK_UNITS`, about 13 MP of Lanczos3 output), which refused a
        // 24 MP file drawn at 150 %; every source edit but a ⌘T preview now
        // takes its job's cap, and the two-step shape is kept. The composed
        // affine is the place's own, [s, 0, 0, s, x, y].
        var scaling: PrecisionPixelCommand?
        if let scale = placementScale(document: source, file: snapshot.url) {
            scaling = try PrecisionPixelCommand(["op": "transform", "layer": target,
                "matrix": [scale, 0, 0, scale, Double(x) * (1 - scale), Double(y) * (1 - scale)],
                "filter": "lanczos3"])
        }
        let asset = developed.0
        let descriptor = try developed.1.json()
        let plan = try source.retainedSourcePlan(asset: asset, request: command.request)
        let scratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        let title = LinkedImageWork.placeTitle(storage)
        let placed = try PrecisionCapturedWork.run(scratchEstimate: scratch, title: title) { control in
            try withExtendedLifetime(snapshot) {
                let placed = try control.withChildOperation { child -> RasterDocument in
                    guard let edited = try source.editingRetainedSource(asset: asset, request: command.request,
                                                                        control: child) else {
                        throw DoctoredCoreError(message: "The camera RAW layer could not be placed.")
                    }
                    return edited
                }
                return try control.withChildOperation { child in
                    try placed.attachingRawSource(target, bytes: snapshot.bytes, descriptor: descriptor,
                                                  control: child)
                }
            }
        }
        var result = placed
        if let scaling {
            let scalePlan = try placed.retainedSourcePlan(request: scaling.request)
            let scaleScratch = try PrecisionDocumentDescriptor.integer(scalePlan, "scratch_bytes")
            result = try PrecisionCapturedWork.run(scratchEstimate: scaleScratch,
                                                   title: title) { control -> RasterDocument in
                guard let scaled = try placed.editingRetainedSource(request: scaling.request,
                                                                    control: control) else {
                    throw DoctoredCoreError(message: "The placed camera RAW layer could not be scaled.")
                }
                return scaled
            }
        }
        // The recorded step pins the exposure it developed with, so a sidecar
        // that appears before a replay cannot change what the replay develops.
        var recorded = effective
        recorded.exposure = effective.exposure ?? captured.recipe.values.exposure
        try RawSourceWork.publish(result, in: document, capture: capture, title: title,
                                  record: recording(record, raw: recorded),
                                  landing: target)
        return notes.isEmpty ? nil : notes.joined(separator: " ")
    }

    /// A placed file lands at its own print size: the document's
    /// resolution over the file's, both axes alike (`float-host-602/take2`,
    /// a 300 ppi file in a 72 ppi document at 24 %). nil when the file states
    /// no resolution or the two agree.
    static func placementScale(document: RasterDocument, file: URL) -> Double? {
        guard let dpi = Bitmap.imageDPI(file), dpi.0.isFinite, dpi.0 > 0 else { return nil }
        let ppi = document.resolution.x
        guard ppi.isFinite, ppi > 0 else { return nil }
        let scale = ppi / dpi.0
        return abs(scale - 1) < 1e-9 ? nil : scale
    }

    /// The place step with the development it ran, so a replay
    /// develops the same way whatever sidecar sits beside the file then.
    static func recording(_ record: [ActionStep], raw: RawDevelopSettings) -> [ActionStep] {
        record.map { step in
            guard step.tool == "place_image" else { return step }
            var step = step
            step.arguments["raw"] = raw.jsonObject()
            return step
        }
    }
}
