import Foundation

// Edit ▸ Auto-Align Layers… ▸ Lens Correction ▸ Vignette Removal, measured
// against take2's V row (`~/devel/doctored-fixtures/photoshop/panorama-564/take2`, re-read by
// `scripts/photoshop-probe/panorama_lens_reread.py`): Photoshop's Perspective + Vignette Removal
// came back with exactly Spherical's bounds, divided one falloff shared by all three frames out of
// each, and scaled every frame by about its mean falloff. Vignette Removal reprojects: Auto and
// Perspective switch to Spherical (Auto behaves as Perspective, AutoAlignProjection.swift; Auto
// with Vignette Removal itself was not measured), Cylindrical and Spherical keep their own
// surface. Collage and Reposition keep their planar maps and run without Vignette Removal (take3,
// `scripts/photoshop-probe/auto_align_lens_take.py`, measured the same output;
// `Registration.lens`, AutoAlignDistortion.swift, holds the table for both lenses).
//
// THE STEP runs inside `AutoAlignWork.reprojectedDocument` (AutoAlignReprojection.swift), after
// the registration and before the warps: one `remove_vignette` request over the reference (the
// identity, first, so its exposure is the one the fit pins) and every registered layer's map,
// planned on the source before any pixel work and run as its own admitted child operation. The
// maps are always the Vision-registered ones: the core pairs RAW pixels through a lens-free
// homography (`align_vignette.rs`), so Geometric Distortion's corrected maps never reach it. The
// core fits the falloff and divides it out (`align_vignette.rs`); the warps then reproject the
// result, or the source when the core changed no sample.

extension AutoAlign {
    /// The falloff the core fitted (`remove_vignette`'s plan): V(ρ) =
    /// 1 + coefficient·ρ², every layer multiplied by gain / V.
    struct VignetteFit {
        let coefficient: Double
        let gain: Double
        let samples: Int
        let layers: [Int]
    }

    /// The surface a job reprojects onto: Cylindrical and Spherical as
    /// chosen; Auto and Perspective Spherical when a lens correction the
    /// table says `applies` is on (take2's Perspective rows; take3 measured
    /// Auto the same); otherwise nil, the plane.
    static func surface(for registration: Registration, vignette: Bool,
                        distortion: Bool = false) -> Registration? {
        switch registration {
        case .cylindrical, .spherical: return registration
        case .auto, .perspective:
            let lens = (vignette && registration.lens(.vignette) == .applies)
                || (distortion && registration.lens(.distortion) == .applies)
            return lens ? .spherical : nil
        case .collage, .translation: return nil
        }
    }

    /// The core's `remove_vignette` request over `maps` (each a layer's
    /// canvas → the reference's canvas), in order: the reference first.
    static func vignetteRequest(_ maps: [(layer: Int, map: Homography)]) throws -> String {
        let fields: [String: Any] = [
            "op": "remove_vignette",
            "layers": maps.map { ["layer": $0.layer, "map": $0.map.m] as [String: Any] },
        ]
        let data = try JSONSerialization.data(withJSONObject: fields)
        return String(decoding: data, as: UTF8.self)
    }

    /// The fit a `remove_vignette` plan answers.
    static func vignetteFit(_ plan: [String: Any]) throws -> VignetteFit {
        guard let coefficient = (plan["coefficient"] as? NSNumber)?.doubleValue,
              let gain = (plan["gain"] as? NSNumber)?.doubleValue,
              let samples = (plan["samples"] as? NSNumber)?.intValue,
              let layers = (plan["layers"] as? [Any])?.compactMap({ ($0 as? NSNumber)?.intValue })
        else {
            throw DoctoredCoreError(message: "\(title) could not plan Vignette Removal.")
        }
        return VignetteFit(coefficient: coefficient, gain: gain, samples: samples, layers: layers)
    }
}

extension AutoAlignWork {
    /// Vignette Removal's plan over the reference and `maps` (each aligned
    /// layer's Vision-registered map, canvas → reference canvas), on the
    /// source: the request, its scratch and output bytes, and the fit.
    /// Throws the core's refusal (too little overlap, a layer it cannot
    /// change) before any pixel work.
    func vignettePlan(maps: [(layer: Int, homography: Homography)],
                      budget: (limit: UInt64, other: UInt64))
        throws -> (request: String, scratch: UInt64, output: UInt64, fit: AutoAlign.VignetteFit)
    {
        let identity = Homography(m: [1, 0, 0, 0, 1, 0, 0, 0, 1])
        let command = try AutoAlign.vignetteRequest(
            [(request.reference, identity)] + maps.map { ($0.layer, $0.homography) })
        let plan = try source.removeVignettePlan(request: command, budget: budget)
        return (command,
                try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes"),
                try PrecisionDocumentDescriptor.integer(plan, "output_bytes"),
                try AutoAlign.vignetteFit(plan))
    }

    /// The step: the planned `request` admitted at `scratch` and run on the
    /// source as one child operation over `progress`; nil when the core
    /// changed no sample.
    func removeVignette(request: String, scratch: UInt64,
                        budget: (limit: UInt64, other: UInt64), control: RasterJobControl,
                        progress: ClosedRange<Double>) throws -> RasterDocument? {
        guard !control.isCancelled else { throw JobCancelled() }
        _ = try RasterResources.shared.admit(scratch: scratch)
        let source = self.source
        return try control.withChildOperation(from: progress.lowerBound, to: progress.upperBound) {
            child in
            guard child.setScratchLimit(scratch) else {
                throw DoctoredCoreError(message: "Could not bound alignment work.")
            }
            return try source.editingRemoveVignette(request: request, budget: budget,
                                                    control: child)
        }
    }
}
