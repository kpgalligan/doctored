import AppKit
import ImageIO

// File ▸ Merge to HDR… and Layer ▸ Merge Layers to HDR…: bracketed exposures of one scene
// become ONE new 32-bit float document holding their exposure-weighted scene-linear merge as a
// single raster layer. The merge, the exposure estimate and the ghost test are the core's
// (`merge_hdr.rs`, through `DoctoredCore+MergeHDR.swift`); registration is Auto-Align's
// (`AutoAlignWork.alignedDocument`); the files form's stack is Load Files into Stack's
// (`StackLoad.build`). This file decides each frame's EV, picks the reference, plans the memory
// and runs the one cancellable job. The source document of the layers form is never edited: no
// `applyEdit`, no history step.
//
// EVs, in order of trust: values the caller gives; EXIF (`exifEV`) when every file has one; the
// core's estimate from the pixels. When the estimate cannot pair two frames `prepare` still
// answers — every frame `unknown`, the reason in `notice` — so the sheet can ask for the values
// and an MCP call that passes `evs` never needs the estimate.
//
// A camera RAW among the files is the 16-bit development with the tone curve and the shadow boost
// off (`rawSettings`), so its numbers are as close to linear as the decoder gives; it is still
// clipped at the decoder's white. A frame is never the 32-bit float (linear) RAW development
// (`RawOutputEncoding.linearFloat`): the merge takes 8- or 16-bit exposures.

/// The frames of one merge, their exposures and the one job that merges them.
enum MergeHDR {
    static let title = "Merge to HDR"
    static let minimumFrames = 2
    /// The core's `merge_hdr::MAX_FRAMES`, checked here before any file is decoded.
    static let maximumFrames = 16
    /// The core's request cap (`doc_merge_hdr::MAX_REQUEST_BYTES`).
    static let requestMaximumBytes = 64 * 1024
    /// Two frames whose EVs are this close are taken for the same exposure
    /// and refused, unless the values were entered: a tenth of a stop is under
    /// the smallest bracket step a camera offers (a third), and above what an
    /// estimate or EXIF rounding moves a value.
    static let identicalEVTolerance = 0.1
    /// A camera RAW frame's development: the tone curve and the shadow boost
    /// off, so the 16-bit numbers follow the sensor as closely as the decoder
    /// allows.
    static let rawSettings = RawDevelopSettings(toneCurve: 0, shadows: 0)
    /// What a reply says about a RAW frame's development.
    static let rawDevelopmentNote =
        "tone curve and shadow boost off; a 16-bit development clipped at the decoder's white"
    static let remedy = "Merge fewer or smaller frames."

    /// The core's exposure estimate.
    enum Estimate {
        case evs([Double])
        case unknown(reason: String)
    }

    /// Where a frame's EV came from.
    enum EVSource: String { case exif, estimated, given, unknown }

    /// What alignment did with a frame.
    enum Alignment: String { case reference, aligned, unchanged, unregistered }

    struct Frame {
        /// The frame's layer in the stack.
        let layer: Int
        let name: String
        /// The file it came from; nil in the layers form.
        let path: String?
        /// Stops, larger = more light; nil exactly when `source` is `unknown`.
        var ev: Double?
        var source: EVSource
        /// nil when the merge did not align.
        var alignment: Alignment?
    }

    struct Options {
        /// One EV per frame, none missing.
        var evs: [Double]
        /// The values were entered by the user or the caller, so two equal
        /// ones are deliberate.
        var evsEntered: Bool
        var align: Bool
        var deghost: Bool
    }

    struct Prepared {
        let stack: RasterDocument
        let frames: [Frame]
        /// The files form's stack plan (`StackLoad.Plan.bytes`); 0 for layers.
        let stackPlanBytes: UInt64
        /// Why the EVs are unknown, when they are; else what building the
        /// stack converted or disclosed, if anything.
        let notice: String?
        /// What building the files form's stack converted or disclosed, kept
        /// apart from the estimate's reason so a merge whose EVs were then
        /// entered still reports it (`Outcome.notice`). nil for layers.
        let stackNotice: String?
    }

    struct LeftOut {
        let name: String
        let reason: String
    }

    struct Outcome {
        /// The new 32-bit float document's handle, not yet shown.
        let document: RasterDocument
        let frames: [Frame]
        /// The reference frame's layer.
        let reference: Int
        /// The frames alignment left out, in frame order.
        let leftOut: [LeftOut]
        /// The core's report.
        let report: [String: Any]
        /// The stack's plan plus the merge's own (`Job.mergeBytes`).
        let planBytes: UInt64
        let notice: String?
    }

    // MARK: - Exposure values

    /// The file's exposure value from its EXIF: log2(t · ISO / (100 · N²)),
    /// stops of light relative to 1 s at f/1 and ISO 100. nil unless the
    /// exposure time, the f-number and the ISO are all present and positive.
    static func exifEV(_ url: URL) -> Double? {
        let options = [kCGImageSourceShouldCache: false] as CFDictionary
        guard let source = CGImageSourceCreateWithURL(url as CFURL, options),
              let info = CGImageSourceCopyPropertiesAtIndex(source, 0, options)
                as? [CFString: Any],
              let exif = info[kCGImagePropertyExifDictionary] as? [CFString: Any],
              let time = (exif[kCGImagePropertyExifExposureTime] as? NSNumber)?.doubleValue,
              let aperture = (exif[kCGImagePropertyExifFNumber] as? NSNumber)?.doubleValue
        else { return nil }
        let rating = exif[kCGImagePropertyExifISOSpeedRatings]
        guard let iso = ((rating as? [NSNumber])?.first ?? rating as? NSNumber)?.doubleValue,
              time.isFinite, aperture.isFinite, iso.isFinite, time > 0, aperture > 0, iso > 0
        else { return nil }
        let ev = log2(time * iso / (100 * aperture * aperture))
        return ev.isFinite ? ev : nil
    }

    /// The frame the result's scale is taken from: the frames ascending by
    /// EV (ties in their own order), element (n − 1) / 2 — the middle
    /// exposure, the lower of the two for an even count.
    static func referenceFrame(_ evs: [Double]) -> Int {
        let order = evs.indices.sorted { (evs[$0], $0) < (evs[$1], $1) }
        return order.isEmpty ? 0 : order[(order.count - 1) / 2]
    }

    /// The core's request JSON; `evs` nil is the exposures request.
    static func requestJSON(layers: [Int], evs: [Double]? = nil, reference: Int = 0,
                            deghost: Bool = false) throws -> String {
        var request: [String: Any] = ["version": 1, "layers": layers]
        if let evs {
            request["evs"] = evs
            request["reference"] = reference
            request["deghost"] = deghost
        }
        let data = try JSONSerialization.data(withJSONObject: request)
        return String(decoding: data, as: UTF8.self)
    }

    private static func checkCount(_ count: Int, _ what: String) throws {
        guard count >= minimumFrames, count <= maximumFrames else {
            throw DoctoredCoreError(
                message: "\(title) takes \(minimumFrames) to \(maximumFrames) \(what); "
                    + "\(count) \(count == 1 ? "was" : "were") given.")
        }
    }

    private static func checkGiven(_ evs: [Double]?, frames: Int) throws {
        guard let evs else { return }
        guard evs.count == frames, evs.allSatisfy(\.isFinite) else {
            throw DoctoredCoreError(
                message: "evs must hold one finite number per frame: \(frames) frames, "
                    + "\(evs.count) values.")
        }
    }

    /// Writes each frame's EV and its source; answers the estimate's reason
    /// when the EVs stay unknown. `files` is the files form's URLs, one per
    /// frame, whose EXIF is read when every one has it.
    private static func assignExposures(_ frames: inout [Frame], in stack: RasterDocument,
                                        given: [Double]?, files: [URL]?) throws -> String? {
        func assign(_ evs: [Double], _ source: EVSource) {
            for index in frames.indices {
                frames[index].ev = evs[index]
                frames[index].source = source
            }
        }
        if let given {
            assign(given, .given)
            return nil
        }
        if let files, files.count == frames.count {
            let exif = files.compactMap(exifEV)
            if exif.count == frames.count {
                assign(exif, .exif)
                return nil
            }
        }
        switch try stack.mergeHDRExposures(layers: frames.map(\.layer)) {
        case .evs(let evs):
            assign(evs, .estimated)
            return nil
        case .unknown(let reason):
            return reason
        }
    }

    // MARK: - Prepare

    /// The files form: the files as one stack (`StackLoad.build`, a camera
    /// RAW developed with `rawSettings`) and each frame's EV. `evs` given:
    /// every frame is `given` and neither EXIF nor the estimate is read. A
    /// first file at 32 bits is refused before anything is decoded. Main
    /// thread.
    static func prepare(files urls: [URL], evs: [Double]? = nil) throws -> Prepared {
        assert(Thread.isMainThread)
        try checkCount(urls.count, "files")
        try checkGiven(evs, frames: urls.count)
        let plan = try StackLoad.plan(urls)
        if plan.depth == 32, let first = plan.probes.first {
            throw DoctoredCoreError(
                message: "“\(first.url.lastPathComponent)” is a 32-bit float image; \(title) "
                    + "takes 8- or 16-bit exposures.")
        }
        let built = try StackLoad.build(urls, raw: rawSettings)
        guard !built.document.isFloat else {
            throw DoctoredCoreError(
                message: "“\(urls[0].lastPathComponent)” opened as a 32-bit float image; "
                    + "\(title) takes 8- or 16-bit exposures.")
        }
        // #547 Lab phase 6b-i: a Lab stack merges through 16-bit ProPhoto RGB (no 32-bit Lab).
        let stack = built.document.isLab ? try LabDocument.mergeStack(built.document) : built.document
        var frames = built.layers.map {
            Frame(layer: $0.index, name: $0.name, path: $0.path, ev: nil, source: .unknown,
                  alignment: nil)
        }
        let unknown = try assignExposures(&frames, in: stack, given: evs, files: urls)
        return Prepared(stack: stack, frames: frames, stackPlanBytes: built.plan.bytes,
                        notice: unknown ?? built.notice, stackNotice: built.notice)
    }

    /// The layers form: `layers` of `doc`, in the caller's order, each a
    /// raster layer of an 8- or 16-bit document — a float document, a group,
    /// an adjustment layer and a text, shape or fill layer are refused, the
    /// layer named. EVs as the files form's, without EXIF. Main thread.
    static func prepare(layers: [Int], in doc: RasterDocument, evs: [Double]? = nil) throws
        -> Prepared
    {
        assert(Thread.isMainThread)
        try checkCount(layers.count, "layers")
        try checkGiven(evs, frames: layers.count)
        guard !doc.isFloat else {
            throw DoctoredCoreError(
                message: "This document is already 32-bit float; \(title) takes 8- or 16-bit "
                    + "exposures.")
        }
        guard Set(layers).count == layers.count else {
            throw DoctoredCoreError(message: "A layer is listed twice.")
        }
        var frames: [Frame] = []
        for index in layers {
            guard let info = doc.layerInfo(index) else {
                throw DoctoredCoreError(message: "Layer \(index) is out of range.")
            }
            let name = "“\(info.name)”"
            guard !info.isGroup else {
                throw DoctoredCoreError(
                    message: "\(name) is a group. \(title) takes raster layers; select the "
                        + "layers inside it instead.")
            }
            guard !doc.layerIsAdjustment(index) else {
                throw DoctoredCoreError(
                    message: "\(name) is an adjustment layer. \(title) takes raster layers.")
            }
            guard doc.describedEntries(in: [index]).isEmpty else {
                throw DoctoredCoreError(
                    message: "\(name) is a text, shape or fill layer. Rasterize it before "
                        + "merging it.")
            }
            frames.append(Frame(layer: index, name: info.name, path: nil, ev: nil,
                                source: .unknown, alignment: nil))
        }
        // #547 Lab phase 6b-i: a Lab document merges through a 16-bit ProPhoto RGB copy (no
        // 32-bit Lab); the document itself stays Lab.
        let stack = doc.isLab ? try LabDocument.mergeStack(doc) : doc
        let unknown = try assignExposures(&frames, in: stack, given: evs, files: nil)
        return Prepared(stack: stack, frames: frames, stackPlanBytes: 0, notice: unknown,
                        stackNotice: nil)
    }

    // MARK: - Merge

    /// What the worker answers.
    private struct Worked {
        let document: RasterDocument
        let frames: [Frame]
        let leftOut: [LeftOut]
        let report: [String: Any]
    }

    /// The one job's state. A class so the worker can DROP the unaligned
    /// stack once the aligned one exists: after `init` this object holds the
    /// only references the merge keeps to it (`stack`, and `work`'s source).
    private final class Job {
        private var stack: RasterDocument?
        private var work: AutoAlignWork?
        private let aligns: Bool
        private var frames: [Frame]
        private var leftOut: [LeftOut] = []
        private let deghost: Bool
        /// The reference frame's layer.
        let referenceLayer: Int
        private let registration: UInt64
        private let budget: (limit: UInt64, other: UInt64)?
        /// The core's `output_bytes + scratch_bytes` for every frame.
        private let coreBytes: UInt64
        /// The merge's plan beside the stack's own: the core's output and
        /// scratch, the result's open (canvas pixels ×
        /// `ImageSourceFile.scratchPerPixel32`), and with alignment the
        /// registration plan and the bytes of the non-reference layers at the
        /// stack's depth — the warped copies stand beside the originals until
        /// the unaligned stack is dropped.
        ///
        /// MEASUREMENT (`scripts/stack-admission/measure.py`, gate
        /// M-merge; a Release app, a fresh process per case at an 8 GiB
        /// working limit, `MallocLargeCache=0`, three runs; five 6000 × 4000
        /// frames of one picture at five shifts, `evs` given, Align on, so
        /// four layers are warped inside the interval; E is the interval's
        /// peak phys_footprint over its start; every baseline twin read
        /// E ≤ 65,536). The reply's plan is this figure plus the stack's, and
        /// the gate holds EACH of the two to the whole interval's peak:
        ///   `merge_to_hdr`, jpeg8 × 5: this plan 2,729,428,644 (the stack's
        ///     2,760,747,679; the reply 5,490,176,323); E 1,053,377,328 /
        ///     1,053,262,664 / 1,053,115,208; 2.591 × (the stack's 2.621 ×)
        ///   `merge_to_hdr`, png16 × 5: this plan 3,569,428,644 (the stack's
        ///     5,502,236,119; the reply 9,071,664,763); E 2,123,990,600 /
        ///     2,122,810,976 / 2,123,187,808; 1.681 × (the stack's 2.591 ×)
        ///   `merge_layers_to_hdr`, png16 × 5 (the stack already open, so the
        ///     plan is this figure alone): plan 3,569,428,644; E 1,527,923,840 /
        ///     1,527,956,608 / 1,527,956,608; 2.336 ×
        /// The 16-bit files case reports more than the 8 GiB limit and is
        /// admitted, because the stack's plan and this one are admitted one
        /// after the other, each before its own stage.
        let mergeBytes: UInt64

        /// Main thread: writes the EVs into the frames, refuses identical
        /// exposures, picks the reference, plans and admits the merge.
        init(prepared: consuming Prepared, options: Options) throws {
            assert(Thread.isMainThread)
            var frames = prepared.frames
            guard options.evs.count == frames.count, options.evs.allSatisfy(\.isFinite) else {
                throw DoctoredCoreError(message: "\(title) needs one EV for every frame.")
            }
            for index in frames.indices where frames[index].ev != options.evs[index] {
                frames[index].ev = options.evs[index]
                frames[index].source = .given
            }
            if !options.evsEntered {
                for first in frames.indices {
                    for second in frames.indices where second > first
                        && abs(options.evs[first] - options.evs[second]) <= identicalEVTolerance
                    {
                        throw DoctoredCoreError(
                            message: "“\(frames[first].name)” and “\(frames[second].name)” have "
                                + "the same exposure (EV \(evText(options.evs[first])) and "
                                + "\(evText(options.evs[second]))). \(title) needs frames taken "
                                + "at different exposures; if these differ, give each frame's EV.")
                    }
                }
            }
            let stackPlanBytes = prepared.stackPlanBytes
            let stack = (consume prepared).stack
            let reference = referenceFrame(options.evs)
            let referenceLayer = frames[reference].layer
            let layers = frames.map(\.layer)
            // The whole request, so every refusal the core has for these
            // layers and EVs arrives before the job starts.
            let plan = try stack.mergeHDRPlan(request: requestJSON(
                layers: layers, evs: options.evs, reference: referenceLayer,
                deghost: options.deghost))
            let coreBytes = try PrecisionDocumentDescriptor.sum(
                PrecisionDocumentDescriptor.integer(plan, "output_bytes"),
                PrecisionDocumentDescriptor.integer(plan, "scratch_bytes"))
            let (pixels, pixelsOverflow) = UInt64(max(stack.width, 0)).multipliedReportingOverflow(
                by: UInt64(max(stack.height, 0)))
            let (open, openOverflow) = pixels.multipliedReportingOverflow(
                by: ImageSourceFile.scratchPerPixel32)
            guard !pixelsOverflow, !openOverflow else {
                throw DoctoredCoreError(message: "Working-memory calculation overflow.")
            }
            var mergeBytes = try PrecisionDocumentDescriptor.sum(coreBytes, open)
            var registration: UInt64 = 0
            var work: AutoAlignWork?
            if options.align {
                let request = try AutoAlign.request(
                    in: stack, layers: layers, reference: referenceLayer, registration: .auto,
                    sampler: DR_FILTER_CATMULL_ROM)
                let aligning = try AutoAlignWork(source: stack, request: request)
                registration = try aligning.registrationPlan()
                let bytesPerPixel = StackLoad.bytesPerPixel(stack.isHighDepth ? 16 : 8)
                var copies: UInt64 = 0
                for layer in request.floating {
                    let info = stack.layerInfo(layer)
                    let (area, areaOverflow) = UInt64(max(info?.width ?? 0, 0))
                        .multipliedReportingOverflow(by: UInt64(max(info?.height ?? 0, 0)))
                    let (bytes, bytesOverflow) = area.multipliedReportingOverflow(by: bytesPerPixel)
                    guard !areaOverflow, !bytesOverflow else {
                        throw DoctoredCoreError(message: "Working-memory calculation overflow.")
                    }
                    copies = try PrecisionDocumentDescriptor.sum(copies, bytes)
                }
                mergeBytes = try PrecisionDocumentDescriptor.sum(
                    mergeBytes, PrecisionDocumentDescriptor.sum(registration, copies))
                work = aligning
            }
            // The reply's figure is this plan plus the stack's; it must exist.
            _ = try PrecisionDocumentDescriptor.sum(stackPlanBytes, mergeBytes)
            // The stack is this job's own input in both forms (the files
            // form's built stack, as `makeStack` passes it, and the layers
            // form's source document), so it is passed as `own`: a merge
            // that cannot fit beside it is refused as too large on its own,
            // not with advice to close documents. Its bytes are in the
            // report's retained total either way, as every live wrapper's
            // are.
            let own = try stack.ownRetainedBytes()
            _ = try RasterResources.shared.admit(scratch: mergeBytes, own: own, remedy: remedy)
            let budget: (limit: UInt64, other: UInt64)? =
                stack.isHighDepth ? nil : try stack.legacyPrecisionWorkContext()
            self.budget = budget
            self.stack = stack
            self.work = work
            self.aligns = options.align
            self.frames = frames
            self.deghost = options.deghost
            self.referenceLayer = referenceLayer
            self.registration = registration
            self.coreBytes = coreBytes
            self.mergeBytes = mergeBytes
        }

        /// Worker: the stack to merge — the aligned one when alignment was
        /// asked for, with each frame's `alignment` written — after which
        /// this object no longer holds the unaligned stack.
        private func aligned(control: RasterJobControl) throws -> RasterDocument {
            guard let stack else {
                throw DoctoredCoreError(message: "\(title) has already run.")
            }
            guard let work else {
                self.stack = nil
                return stack
            }
            let (document, outcome) = try work.alignedDocument(
                control: control, registration: registration, budget: budget, share: 0...0.6)
            let moved = Set(outcome.aligned.map(\.layer))
            var reasons: [Int: String] = [:]
            for skipped in outcome.unregistered { reasons[skipped.layer] = skipped.reason }
            for index in frames.indices {
                let layer = frames[index].layer
                if layer == referenceLayer {
                    frames[index].alignment = .reference
                } else if moved.contains(layer) {
                    frames[index].alignment = .aligned
                } else if let reason = reasons[layer] {
                    frames[index].alignment = .unregistered
                    leftOut.append(LeftOut(name: frames[index].name, reason: reason))
                } else {
                    frames[index].alignment = .unchanged
                }
            }
            self.work = nil
            self.stack = nil
            return document
        }

        /// Worker, after `control.beginCompoundOperation()`: alignment over
        /// 0…0.6 of the progress, the merge over the rest.
        func run(control: RasterJobControl) throws -> Worked {
            let source = try aligned(control: control)
            let used = frames.filter { $0.alignment != .unregistered }
            guard used.count >= minimumFrames else {
                let lost = leftOut.map { "“\($0.name)” (\($0.reason))" }.joined(separator: ", ")
                throw DoctoredCoreError(
                    message: "\(title) needs at least \(minimumFrames) frames, and alignment "
                        + "left out \(lost). Turn Align off to merge the frames as they are.")
            }
            guard !control.isCancelled else { throw JobCancelled() }
            let request = try requestJSON(
                layers: used.map(\.layer), evs: used.map { $0.ev ?? 0 },
                reference: referenceLayer, deghost: deghost)
            let limit = coreBytes
            let budget = self.budget
            // A cancel inside the merge leaves here as the core's "operation
            // cancelled" error, not as `JobCancelled`: `BackgroundWork.wait`
            // turns any failed task under a cancelled control into
            // `JobCancelled` (BackgroundWork.swift), so `merge` still throws
            // the type both menu actions catch.
            let merged = try control.withChildOperation(from: aligns ? 0.6 : 0, to: 1) {
                child -> (document: RasterDocument, report: [String: Any]) in
                guard child.setScratchLimit(limit) else {
                    throw DoctoredCoreError(message: "Could not bound the merge.")
                }
                return try source.mergingHDR(request: request, budget: budget, control: child)
            }
            return Worked(document: merged.document, frames: frames, leftOut: leftOut,
                          report: merged.report)
        }
    }

    private static func evText(_ ev: Double) -> String {
        String(format: "%+.2f", ev)
    }

    /// The ONE cancellable job: with `options.align`, Auto-Align's
    /// registration and warps of every frame onto the reference
    /// (`referenceFrame`), then the core's merge of the frames alignment
    /// kept. An `unregistered` frame is left out with its reason; fewer than
    /// two usable frames refuses. EVs within `identicalEVTolerance` of each
    /// other refuse, naming both frames, unless `options.evsEntered`. Throws
    /// `JobCancelled` on a cancel; nothing is published here. Main thread.
    ///
    /// `prepared` is CONSUMED: pass `consume prepared`, so the unaligned
    /// stack of the files form can be freed inside the job once the aligned
    /// one exists. The stack's own plan was admitted before it was decoded
    /// (`StackLoad.plan`); the merge's plan is admitted here, with the stack
    /// resident, and `Outcome.planBytes` is their sum.
    static func merge(_ prepared: consuming Prepared, options: Options) throws -> Outcome {
        assert(Thread.isMainThread)
        let stackPlanBytes = prepared.stackPlanBytes
        let notice = prepared.stackNotice
        let job = try Job(prepared: consume prepared, options: options)
        let mergeBytes = job.mergeBytes
        let control = try RasterJobControl()
        guard control.setScratchLimit(mergeBytes) else {
            throw DoctoredCoreError(message: "Could not bound the merge.")
        }
        let worked = try BackgroundWork.wait(
            title: title, control: control, revealAfter: DocumentJobs.panelDelay
        ) {
            try WorkerTask(scratchEstimate: mergeBytes) { () -> Worked in
                try control.beginCompoundOperation()
                return try job.run(control: control)
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        return Outcome(
            document: worked.document, frames: worked.frames, reference: job.referenceLayer,
            leftOut: worked.leftOut, report: worked.report,
            planBytes: try PrecisionDocumentDescriptor.sum(stackPlanBytes, mergeBytes),
            notice: notice)
    }
}

extension ImageDocument {
    /// The merged document, shown: `makeStack`'s shape around the float
    /// handle — native `fileType`, dirty through `countEditChange`, its
    /// history "Initial State" with no undo step — then the window, as
    /// `openStack` shows one. The source document of the layers form is not
    /// touched.
    @discardableResult
    static func openMergedHDR(_ outcome: MergeHDR.Outcome, name: String?) throws -> ImageDocument {
        guard outcome.document.isFloat else {
            throw DoctoredCoreError(message: "\(MergeHDR.title) did not produce a 32-bit document.")
        }
        let document = ImageDocument()
        document.doc = outcome.document
        document.activeLayerIndex = 0
        document.refreshProjection()
        document.fileType = Self.nativeTypeIdentifier
        if let name = name?.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty {
            document.displayName = name
        }
        document.countEditChange(.changeDone)
        NSDocumentController.shared.addDocument(document)
        document.makeWindowControllers()
        document.showWindows()
        return document
    }
}
