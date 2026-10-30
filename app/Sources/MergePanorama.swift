import AppKit

// File ▸ Automate ▸ Merge Panorama…, the one-dialog panorama. It composes the
// stages that already exist and reimplements none: Load Files into Stack (`StackLoad.build`, its own
// per-file jobs, as Merge to HDR's files form), then ONE cancellable job (`MergePanoramaWork`) that runs
// Auto-Align (`AutoAlignWork.alignedDocument`, the union canvas), Auto-Blend Panorama with Seamless
// Tones and Colors (`AutoBlendWork.blended`) and its Content-Aware Fill Transparent Areas
// (`AutoBlendFill.run`), and `ImageDocument.makeStack` for the new document. Each stage admits its
// own existing plan, as `AutoBlendWork.wait` does; there is no new memory plan or constant.
//
// The reference is the MIDDLE source. A frame that does not overlap it is unregistered (planar
// layouts register to the reference only): it is kept, hidden and named, and fewer than two
// registered frames refuses at Auto-Align. Auto is Perspective; the sampler is bicubic.
// Layers are named after their files, bottom-first in the dialog's list order (MCP: the caller's
// order). A refusal names its stage (`stageRefusal`).

/// Merge Panorama's options, refusals and the one entry point, `run`.
enum MergePanorama {
    static let title = "Merge Panorama"
    static let minimumFiles = 2

    /// The dialog's layout order: Auto, Perspective, Cylindrical, Spherical, Collage, Reposition.
    static let layouts: [AutoAlign.Registration] = [
        .auto, .perspective, .cylindrical, .spherical, .collage, .translation,
    ]

    /// The pipeline's stages, as a refusal names them.
    enum Stage: String {
        case load = "Load Files into Stack"
        case align = "Auto-Align"
        case blend = "Blend Images Together"
        case fill = "Content-Aware Fill Transparent Areas"
    }

    /// The dialog's choices; the defaults: Auto, Blend on, the rest off.
    struct Options: Equatable {
        var layout: AutoAlign.Registration = .auto
        var blend = true
        var vignette = false
        var distortion = false
        var fill = false
    }

    static let fillNeedsBlend =
        "Content-Aware Fill Transparent Areas needs Blend Images Together."

    /// Collage or Reposition with a lens box on: Doctored's rule is that those
    /// two layouts clear both lens boxes, and the dialog disables them.
    static func lensRefusal(_ layout: AutoAlign.Registration) -> String {
        "The \(layout.title) layout takes neither Vignette Removal nor Geometric Distortion "
            + "Correction in Merge Panorama. Turn both off, or choose Auto, Perspective, Cylindrical "
            + "or Spherical."
    }

    /// A refusal, prefixed with the stage that refused.
    static func stageRefusal(_ stage: Stage, _ reason: String) -> String {
        "\(title) stopped at \(stage.rawValue): \(reason)"
    }

    /// Why Auto-Align leaves fewer than two frames to merge: each frame it
    /// could not register, named beside its own reason.
    static func tooFewAligned(_ unregistered: [AutoAlign.Skipped]) -> String {
        let listed = unregistered.map { "“\($0.name)” (\(clause($0.reason)))" }
        let names = listed.isEmpty ? "The frames" : listed.joined(separator: ", ")
        return "\(names) could not be registered to the middle file, which leaves fewer than "
            + "two frames to merge. Each frame must overlap the middle one."
    }

    /// A skip reason without its closing period, for use inside parentheses.
    fileprivate static func clause(_ reason: String) -> String {
        reason.hasSuffix(".") ? String(reason.dropLast()) : reason
    }

    /// The reference: the middle source, the lower of the two middles for an
    /// even count.
    static func referenceIndex(count: Int) -> Int {
        max(count - 1, 0) / 2
    }

    /// The files of the open documents, for Add Open Files: every image
    /// document with a file on disk, native `.rz` documents excluded (Load
    /// Files into Stack cannot plan them, `StackLoad.probe`).
    static func openFileURLs() -> [URL] {
        NSDocumentController.shared.documents.compactMap { document -> URL? in
            guard let image = document as? ImageDocument, let url = image.fileURL,
                  !ImageDocument.isNativeDocumentType(image.fileType),
                  FileManager.default.fileExists(atPath: url.path),
                  !ImageDocument.hasNativeSignature(at: url) else { return nil }
            return url
        }
    }

    private static var untitledCount = 0

    /// "Untitled_Panorama<n>", n from 1 per app session: the name of a
    /// Merge Panorama result.
    static func untitledName() -> String {
        untitledCount += 1
        return "Untitled_Panorama\(untitledCount)"
    }

    /// What one run made.
    struct Outcome {
        let document: RasterDocument
        let load: StackLoad.Outcome
        let options: Options
        let aligned: AutoAlign.Outcome
        /// The blend's outcome; nil with Blend Images Together off.
        let blend: AutoBlend.Outcome?
        /// The largest plan any stage admitted.
        let planBytes: UInt64
        /// The hidden layers' sentences, then the load's notice; nil when
        /// there is neither.
        var notice: String? {
            var sentences = aligned.unregistered.map { entry -> String in
                "“\(entry.name)” is kept hidden: it could not be registered to the middle "
                    + "file (\(MergePanorama.clause(entry.reason)))."
            }
            if let loaded = load.notice { sentences.append(loaded) }
            return sentences.isEmpty ? nil : sentences.joined(separator: " ")
        }
    }

    /// An already-phrased refusal, passed through unchanged.
    struct Refusal: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }

    /// `error` as `stage`'s refusal: a cancel stays a cancel and an
    /// already-phrased refusal is passed through.
    static func refusal(_ stage: Stage, _ error: Error, _ control: RasterJobControl? = nil)
        -> Error
    {
        if error is JobCancelled || control?.isCancelled == true { return JobCancelled() }
        if let cocoa = error as? CocoaError, cocoa.code == .userCancelled { return cocoa }
        if error is Refusal { return error }
        let reason = (error as? DoctoredCoreError)?.message ?? error.localizedDescription
        return Refusal(message: stageRefusal(stage, reason))
    }

    /// Main thread. Validates `options`, loads `urls` as one stack
    /// (`StackLoad.build`, which plans and admits it first), then runs
    /// `MergePanoramaWork`. Nothing is published here. Throws `JobCancelled` /
    /// `CocoaError.userCancelled` unwrapped and every other error as
    /// `stageRefusal`.
    static func run(_ urls: [URL], options: Options) throws -> Outcome {
        assert(Thread.isMainThread)
        guard urls.count >= minimumFiles else {
            throw Refusal(message: "\(title) needs at least \(minimumFiles) source files.")
        }
        if options.fill && !options.blend { throw Refusal(message: fillNeedsBlend) }
        if options.vignette || options.distortion,
           options.layout == .collage || options.layout == .translation {
            throw Refusal(message: lensRefusal(options.layout))
        }
        let load: StackLoad.Outcome
        do {
            load = try StackLoad.build(urls)
        } catch {
            throw refusal(.load, error)
        }
        guard !load.document.isCmyk else {
            throw Refusal(message: stageRefusal(.align, CmykDocument.refusal(tool: title)))
        }
        let result: (document: RasterDocument, aligned: AutoAlign.Outcome,
                     blend: AutoBlend.Outcome?, planBytes: UInt64)
        do {
            result = try MergePanoramaWork(stack: load.document, options: options).run()
        } catch {
            throw refusal(.align, error)
        }
        return Outcome(document: result.document, load: load, options: options,
                       aligned: result.aligned, blend: result.blend,
                       planBytes: max(load.plan.bytes, result.planBytes))
    }
}

/// The ONE compound Merge Panorama job: Auto-Align, the hiding of unregistered
/// frames, the Panorama blend and its fill under one `RasterJobControl`, so
/// Cancel/Esc stops it inside any stage and a cancel publishes nothing. The
/// shape is `AutoBlendWork.wait`'s without the display preparation (the
/// result is a new document). The worker captures values only.
struct MergePanoramaWork {
    let stack: RasterDocument
    let options: MergePanorama.Options
    /// The alignment stage, captured on main.
    private let align: AutoAlignWork
    /// Each layer's name, read on main.
    private let names: [Int: String]

    /// Main thread: Auto-Align's request over every layer with the middle
    /// reference, its work, Auto-Blend's eligibility (with Blend on) and the
    /// layers' names. A refusal names its stage.
    init(stack: RasterDocument, options: MergePanorama.Options) throws {
        guard Thread.isMainThread else {
            throw DoctoredCoreError(message: "Capture the layers before Merge Panorama work starts.")
        }
        self.stack = stack
        self.options = options
        let all = Array(0..<stack.layerCount)
        do {
            let request = try AutoAlign.request(
                in: stack, layers: all, reference: MergePanorama.referenceIndex(count: all.count),
                registration: options.layout, sampler: DR_FILTER_CATMULL_ROM, growsCanvas: true,
                vignette: options.vignette, distortion: options.distortion)
            align = try AutoAlignWork(source: stack, request: request)
        } catch {
            throw MergePanorama.refusal(.align, error)
        }
        if options.blend {
            do {
                _ = try AutoBlend.request(in: stack, layers: all, method: .panorama, seamless: true,
                                          align: false, fill: options.fill)
            } catch {
                throw MergePanorama.refusal(.blend, error)
            }
        }
        var names: [Int: String] = [:]
        for index in all {
            names[index] = stack.layerInfo(index)?.name ?? "Layer \(index)"
        }
        self.names = names
    }

    /// Runs the job. Progress: Auto-Align 0…0.5 (0…1 with Blend off), the
    /// blend to 0.75 with the fill (to 1 without), the fill 0.75…1.
    /// Admission is per stage: the registration plan and the stack's blend
    /// plan on main (so a refusal starts no job), each warp's plan, the
    /// aligned document's blend plan and the fill's plans on the worker just
    /// before each runs. Throws `JobCancelled` on a cancel; every other
    /// failure as its stage's refusal.
    func run() throws -> (document: RasterDocument, aligned: AutoAlign.Outcome,
                          blend: AutoBlend.Outcome?, planBytes: UInt64)
    {
        assert(Thread.isMainThread)
        let stack = self.stack
        let align = self.align
        let names = self.names
        let options = self.options
        let budget: (limit: UInt64, other: UInt64)?
        let fillBudget: (limit: UInt64, other: UInt64)?
        let registration: UInt64
        var initial: UInt64
        do {
            budget = stack.isHighDepth ? nil : try stack.legacyPrecisionWorkContext()
            fillBudget = options.blend && options.fill ? try GenerativeFillRun.budget(stack) : nil
            registration = try align.registrationPlan()
            _ = try RasterResources.shared.admit(scratch: registration)
            initial = registration
        } catch {
            throw MergePanorama.refusal(.align, error)
        }
        if options.blend {
            do {
                // Read from the stack first: it answers every core refusal
                // before a job starts. The aligned document's own plan is
                // admitted on the worker before the blend runs.
                let command = try AutoBlend.command(AutoBlend.Request(
                    layers: Array(0..<stack.layerCount), method: .panorama, seamless: true,
                    align: nil, fill: options.fill))
                let planned = try PrecisionDocumentDescriptor.integer(
                    stack.autoBlendPlan(request: command), "scratch_bytes")
                _ = try RasterResources.shared.admit(scratch: planned)
                initial = max(initial, planned)
            } catch {
                throw MergePanorama.refusal(.blend, error)
            }
        }
        let first = initial
        let control = try RasterJobControl()
        guard control.setScratchLimit(first) else {
            throw DoctoredCoreError(message: "Could not bound Merge Panorama work.")
        }
        let alignEnd = options.blend ? 0.5 : 1.0
        let blendEnd = fillBudget == nil ? 1.0 : 0.75
        let result = try BackgroundWork.wait(
            title: MergePanorama.title, control: control, revealAfter: DocumentJobs.panelDelay
        ) {
            try WorkerTask(scratchEstimate: first) {
                () -> (document: RasterDocument, aligned: AutoAlign.Outcome,
                       blend: AutoBlend.Outcome?, planBytes: UInt64) in
                try control.beginCompoundOperation()
                let stage: (document: RasterDocument, outcome: AutoAlign.Outcome)
                do {
                    stage = try align.alignedDocument(
                        control: control, registration: registration, budget: budget,
                        share: 0...alignEnd)
                } catch {
                    throw MergePanorama.refusal(.align, error, control)
                }
                let aligned = stage.outcome
                var largest = max(first, aligned.planBytes)
                // Ascending, as `AutoBlend.Request.layers` requires, so the
                // last is the topmost registered frame.
                let registered = ([aligned.reference] + aligned.aligned.map(\.layer)
                    + aligned.unchanged.map(\.layer)).sorted()
                guard registered.count >= MergePanorama.minimumFiles else {
                    throw MergePanorama.Refusal(message: MergePanorama.stageRefusal(
                        .align, MergePanorama.tooFewAligned(aligned.unregistered)))
                }
                var running = stage.document
                do {
                    for skipped in aligned.unregistered {
                        running = try running.withLayerVisible(skipped.layer, false) ?? running
                    }
                } catch {
                    throw MergePanorama.refusal(.align, error, control)
                }
                guard !control.isCancelled else { throw JobCancelled() }
                guard options.blend else {
                    return (document: running, aligned: aligned, blend: nil, planBytes: largest)
                }
                let blended: (document: RasterDocument, report: [String: Any], planBytes: UInt64)
                do {
                    let command = try AutoBlend.command(AutoBlend.Request(
                        layers: registered, method: .panorama, seamless: true, align: nil,
                        fill: options.fill))
                    blended = try AutoBlendWork.blended(
                        running, command: command, budget: budget, control: control,
                        share: alignEnd...blendEnd)
                } catch {
                    throw MergePanorama.refusal(.blend, error, control)
                }
                largest = max(largest, blended.planBytes)
                running = blended.document
                var filled: AutoBlendFill.Filled?
                if let fillBudget, let top = registered.last {
                    // Panorama reports no merged layer, so the fill stamps the
                    // visible picture above the topmost registered frame and
                    // names it after that frame.
                    let merged = (blended.report["merged_layer"] as? NSNumber).map(\.intValue)
                    let input = running
                    do {
                        let stage = try control.withChildOperation(from: blendEnd, to: 1) { child in
                            try AutoBlendFill.run(input, top: top, mergedLayer: merged,
                                                  budget: fillBudget, control: child)
                        }
                        running = stage.document
                        filled = stage.filled
                        largest = max(largest, stage.planBytes)
                    } catch {
                        throw MergePanorama.refusal(.fill, error, control)
                    }
                }
                let outcome: AutoBlend.Outcome
                do {
                    outcome = try AutoBlend.outcome(
                        report: blended.report, names: names, aligned: nil, fill: filled,
                        planBytes: largest)
                } catch {
                    throw MergePanorama.refusal(.blend, error, control)
                }
                return (document: running, aligned: aligned, blend: outcome, planBytes: largest)
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        return result
    }
}

extension ImageDocument {
    /// The Merge Panorama result as a new document, shown: `makeStack` around the
    /// finished handle (its history "Initial State" with no undo step, its
    /// active layer the topmost), named `name` or "Untitled_Panorama<n>",
    /// then the window, as `openStack` shows one.
    @discardableResult
    static func openMergePanorama(_ outcome: MergePanorama.Outcome, name: String?) throws -> ImageDocument {
        let load = outcome.load
        let document = try makeStack(
            StackLoad.Outcome(document: outcome.document, plan: load.plan, layers: load.layers,
                              profileName: load.profileName),
            name: name ?? MergePanorama.untitledName())
        NSDocumentController.shared.addDocument(document)
        document.makeWindowControllers()
        document.showWindows()
        return document
    }
}
