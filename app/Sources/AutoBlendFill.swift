import AppKit

// Auto-Blend Layers ▸ Panorama's Content-Aware Fill Transparent Areas: after the
// blend, in the same job and undo step, the union canvas's transparent area — Generative Expand's
// coverage, `genExpandCoverage` (65535 − the composite's alpha) — is filled by the one
// Content-Aware Fill (`ContentAwareFillWork`, with the core's `fill_transparent` pad so the fill
// shows where the layer was transparent). take2's F is F-MERGED: the fill adds ONE layer
// above the frames, opaque over the whole canvas, named "<top frame> (merged)", and leaves the
// frames and their masks as they were. So the visible picture is stamped above the top frame
// under that name and filled in place. Runs on the job's worker (`AutoBlendWork.wait`).

/// The fill stage of Auto-Blend's Panorama.
enum AutoBlendFill {
    /// Where the fill lands: the stamped picture filled in place (`merged`, take2's F-MERGED),
    /// or the stamp masked to the filled area (`fillOnly`, F-NEW's row).
    enum Placement { case merged, fillOnly }

    /// take2's F: F-MERGED.
    static let placement: Placement = .merged

    /// take2's N: the layer is named after the top frame ("right (merged)", and
    /// "b (merged)" in its selftest).
    static func layerName(top: String) -> String {
        "\(top) (merged)"
    }

    /// The whole call's refusal when the fill cannot run: the core's own sentence, then the way
    /// out (the sentences of the fill's caps advise selecting less, which a panorama cannot do).
    static func refusalMessage(_ core: String) -> String {
        "Content-Aware Fill Transparent Areas could not fill the transparent area: \(core) "
            + "Turn it off to blend without the fill."
    }

    /// The core's refusal when the picture has no transparent pixel (`doc_gen_expand.rs`
    /// `NO_TRANSPARENCY`): the fill has nothing to do.
    static let noTransparency = "the picture has no transparent area"

    struct Filled {
        /// The filled layer's index in the result; nil when nothing was transparent.
        let layer: Int?
        /// The coverage's pixels above 0.
        let coveredPixels: Int
    }

    /// Fills `doc`'s transparent area (worker thread; `doc` is the job's own blended document).
    /// The target is `mergedLayer` when there is one, else the visible picture stamped above
    /// `top`. Each stage sets its own scratch limit: the coverage the working limit (the op
    /// admits its own plan against `budget`), the fill its own plan once admitted. A cancel
    /// throws `JobCancelled`; any other failure throws `refusalMessage`.
    static func run(_ doc: RasterDocument, top: Int, mergedLayer: Int?,
                    budget: (limit: UInt64, other: UInt64), control: RasterJobControl) throws
        -> (document: RasterDocument, filled: Filled, planBytes: UInt64)
    {
        try control.beginCompoundOperation()
        let samples: [UInt16]
        do {
            samples = try control.withChildOperation(from: 0, to: 0.2) { child in
                guard child.setScratchLimit(budget.limit) else {
                    throw DoctoredCoreError(message: "Could not bound the fill's coverage.")
                }
                return try doc.genExpandCoverage(layer: top, limit: budget.limit,
                                                 other: budget.other, control: child)
            }
        } catch let error as DoctoredCoreError
            where error.message.hasPrefix(noTransparency) && !control.isCancelled {
            return (doc, Filled(layer: nil, coveredPixels: 0), 0)
        } catch {
            throw refusal(error, control)
        }
        do {
            let target: (document: RasterDocument, layer: Int)
            if let mergedLayer {
                target = (doc, mergedLayer)
            } else {
                let name = layerName(top: doc.layerInfo(top)?.name ?? "Layer \(top)")
                guard let stamped = try doc.stampVisible(above: top, name: name) else {
                    throw DoctoredCoreError(message: "the picture could not be stamped.")
                }
                target = (stamped, doc.insertionIndex(above: top))
            }
            let stamped = target.document
            let bytes = samples.map { UInt8((UInt32($0) + 128) / 257) }
            var settings = ContentAwareFillSettings()
            settings.fillTransparent = true
            let work = try ContentAwareFillWork(
                source: stamped, layer: target.layer, coverage: bytes, settings: settings,
                nativeCoverage: stamped.isHighDepth ? samples : nil, ownedWorker: true)
            // As `CropFillRun.startContentAware` admits it.
            _ = try RasterResources.shared.admit(scratch: work.scratch)
            let planBytes = work.scratch + (try stamped.ownRetainedBytes())
            let filled = try control.withChildOperation(from: 0.2, to: 1) { child in
                guard child.setScratchLimit(work.scratch) else {
                    throw DoctoredCoreError(message: "Could not bound the fill.")
                }
                return try work.compute(control: child)
            } ?? stamped
            let result = placement == .fillOnly && mergedLayer == nil
                ? try masked(filled, layer: target.layer, samples: samples, bytes: bytes)
                : filled
            let covered = samples.reduce(0) { $1 > 0 ? $0 + 1 : $0 }
            return (result, Filled(layer: target.layer, coveredPixels: covered), planBytes)
        } catch {
            throw refusal(error, control)
        }
    }

    /// F-NEW's placement: the stamp keeps only the filled area, through a layer mask from the
    /// coverage (8 bits from the bytes, 16 and 32 bits from the samples).
    private static func masked(_ doc: RasterDocument, layer: Int, samples: [UInt16],
                               bytes: [UInt8]) throws -> RasterDocument {
        if !doc.isHighDepth {
            guard let out = doc.addingLayerMask(layer, kind: DR_MASK_FROM_SELECTION,
                                                selection: bytes) else {
                throw DoctoredCoreError(message: "the filled layer could not be masked.")
            }
            return out
        }
        let input = try PrecisionScalarBuffer(samples: samples, width: doc.width,
                                              height: doc.height)
        let command = try PrecisionPixelCommand.setMask(layer: layer, width: doc.width,
                                                        height: doc.height)
        guard let out = try doc.performingPrecisionPixelEdit(command, input: input) else {
            throw DoctoredCoreError(message: "the filled layer could not be masked.")
        }
        return out
    }

    /// A cancel stays a cancel; anything else is the whole call's refusal.
    private static func refusal(_ error: Error, _ control: RasterJobControl) -> Error {
        if error is JobCancelled || control.isCancelled { return JobCancelled() }
        let core = (error as? DoctoredCoreError)?.message ?? error.localizedDescription
        return DoctoredCoreError(message: refusalMessage(core))
    }
}
