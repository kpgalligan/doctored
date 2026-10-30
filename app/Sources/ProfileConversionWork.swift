import AppKit

// Convert to Profile, `convert_profile` and the artwork conversion admit, run on `BackgroundWork`
// with Cancel, and throw a refusal or cancel rather than swallowing it. Its `preflight`
// is the Convert sheet's, `convert_profile`'s and the high-depth Convert's preflight: a
// preserve-appearance check that can bake runs, at 8 and 16 bits, as a cancellable worker wait
// under a job control while the attached sheet stays up; every other case — and an occupied
// worker, which must never become the refusal — runs inline.

/// The one 8-bit options conversion worker: Image ▸ Convert to
/// Profile…, MCP `convert_profile` and the artwork conversion all run a plain
/// byte `convertingToProfile(_:options:)` here — admitted against the working
/// limit, on the bounded worker, with progress and Cancel — instead of on
/// the main thread. The high-depth conversion owns its own captured worker;
/// its report comes from `preflight`, so a baking check is cancellable there
/// too.
enum ProfileConversionWork {
    /// The retained-byte conversion estimate, plus the flattened
    /// projection and one RGBA8 canvas per adjustment layer when a
    /// preserve-appearance conversion may bake.
    static func scratch(_ source: RasterDocument, profile: Data,
                        options: ConversionOptions) throws -> UInt64 {
        let bakeBound = bakeCandidates(source, options: options)
        var scratch = try RetainedByteProfileWork.scratchEstimate(source)
        if bakeBound > 0 {
            scratch = try PrecisionDocumentDescriptor.sum(scratch, source.projectionScratchEstimate())
            scratch = try PrecisionDocumentDescriptor.sum(
                scratch, UInt64(bakeBound) * UInt64(source.width) * UInt64(source.height) * 4)
        }
        return scratch
    }

    /// How many adjustment layers a preserve-appearance conversion may bake:
    /// zero under any other policy.
    static func bakeCandidates(_ source: RasterDocument, options: ConversionOptions) -> Int {
        options.adjustments == .preserveAppearance
            ? (0..<source.layerCount).filter { source.layerIsAdjustment($0) }.count : 0
    }

    /// The conversion's preflight report. A preflight that can
    /// bake renders one projection per adjustment layer, so at 8 and 16 bits
    /// it runs as a worker wait with Cancel under a job control, admitted
    /// against the projection; closing the document cancels it. Every other
    /// case runs inline as before — and so does a busy worker, which must
    /// never become the conversion's refusal. Throws the core's sentence — a
    /// destination whose matrix/TRC curves fail the precision check among
    /// them, worded as its `convert_refusal` — an admission refusal,
    /// or `JobCancelled`. Main thread only.
    ///
    /// Record (the gate's Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB,
    /// a fresh instance per run on 50xx, `MallocLargeCache=0`, E =
    /// `ri_interval_max_phys_footprint` − pre around one MCP `convert_profile`
    /// `preserve_appearance` into Display P3, 3 runs plus a baseline twin ×3).
    /// Fixture: a 6000×4000 white document, three Curves, then a clipped
    /// Curves on top, so the preflight renders three bakes and refuses at the
    /// clipped one before any conversion; the twin's clipped Curves is the
    /// bottom one, so it refuses before any render. Rule: the plan
    /// (`projectionScratchEstimate()`) ≥ ⌈1.25 × max E⌉.
    /// - 8-bit: E 865,043,056 / 864,879,216 / 865,010,288; twin 606,208 /
    ///   573,440 / 606,208. Plan: core 1,728,000,000 + n×4 96,000,000 =
    ///   1,824,000,000 structural, charged (m8 = 7) 2,622,000,000 ≥
    ///   1,081,303,820 (3.031×).
    /// - 16-bit: E 576,487,832 / 576,422,296 / 576,684,440; twin 81,920 /
    ///   212,992 / 229,376. Plan: max(render 865,048,576, output 192,000,000 +
    ///   display 97,048,576) + overhead 1,700,339 = 866,748,915 structural,
    ///   charged (m16 = 6) 1,191,779,759 ≥ 720,855,550 (2.067×).
    /// Outcome: both depths pass; the projection estimate stays the plan.
    static func preflight(_ source: RasterDocument, profile: Data, options: ConversionOptions,
                          title: String) throws -> ConversionReport {
        assert(Thread.isMainThread)
        if bakeCandidates(source, options: options) == 0 || BackgroundWork.occupied {
            return try source.conversionPreflight(profile, options: options)
        }
        // Bakes render one at a time, so the projection is the figure.
        let estimate = try source.projectionScratchEstimate()
        _ = try RasterResources.shared.admit(scratch: estimate)
        let control = try RasterJobControl()
        guard control.setScratchLimit(estimate) else {
            throw DoctoredCoreError(message: "Could not configure the profile conversion check.")
        }
        let observer = NotificationCenter.default.addObserver(forName: .imageDocumentDidClose,
            object: nil, queue: nil) { notification in
                if let owner = notification.object as? ImageDocument, owner.doc === source {
                    control.cancel()
                }
            }
        defer { NotificationCenter.default.removeObserver(observer) }
        // The typed budget (`PrecisionWorkContext`) is built inside the
        // worker, by `conversionPreflight`, as the captured edits build it.
        let task = try WorkerTask(scratchEstimate: estimate) {
            try source.conversionPreflight(profile, options: options, control: control)
        }
        let report = try BackgroundWork.wait(task, title: title, control: control)
        if control.isCancelled { throw JobCancelled() }
        return report
    }

    /// nil is the core's no-op (nothing would change). A throw is an
    /// admission refusal, the core's sentence or `JobCancelled`. Main
    /// thread only (`BackgroundWork.wait`).
    static func run(_ source: RasterDocument, profile: Data, options: ConversionOptions,
                    title: String) throws -> (document: RasterDocument, report: ConversionReport)? {
        if source.isHighDepth {
            // The report comes from `preflight` (a cancellable worker wait
            // when it can bake) and the document from the captured-edit path.
            let report = try preflight(source, profile: profile, options: options, title: title)
            guard let next = try source.performingPrecisionAdjustment(options.isDefault
                ? .convertProfile(profile) : .convertProfileWith(profile, options))
            else { return nil }
            return (next, report)
        }
        let estimate = try scratch(source, profile: profile, options: options)
        _ = try RasterResources.shared.admit(scratch: estimate)
        let control = try RasterJobControl()
        guard control.setScratchLimit(estimate) else {
            throw DoctoredCoreError(message: "Could not configure profile conversion.")
        }
        let observer = NotificationCenter.default.addObserver(forName: .imageDocumentDidClose,
            object: nil, queue: nil) { notification in
                if let owner = notification.object as? ImageDocument, owner.doc === source {
                    control.cancel()
                }
            }
        defer { NotificationCenter.default.removeObserver(observer) }
        let task = try WorkerTask(scratchEstimate: estimate) {
            try source.convertingToProfile(profile, options: options, control: control)
        }
        let converted = try BackgroundWork.wait(task, title: title, control: control)
        if control.isCancelled { throw JobCancelled() }
        return converted
    }
}
