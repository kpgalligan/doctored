import Foundation

// Edit ▸ Auto-Align Layers… ▸ Lens Correction ▸ Geometric Distortion, measured
// against take2's D row (`~/devel/doctored-fixtures/photoshop/panorama-564/take2`, re-read by
// `scripts/photoshop-probe/panorama_lens_reread.py`: Photoshop's Perspective + Geometric
// Distortion came back as the Spherical of the frames with one lens correction undone, the same
// on all three) and take3's availability matrix (`scripts/photoshop-probe/auto_align_lens_take.py`,
// `panorama-564/take3`).
//
// THE TABLE (take3, each lens's on case against its off twin, per projection): Vignette Removal
// APPLIED on Auto, Perspective, Cylindrical and Spherical and was IGNORED on Collage and
// Reposition (bounds equal, no pixel apart); Geometric Distortion APPLIED on all six. Applied on
// Auto and Perspective reprojects onto the sphere (take2: Perspective with a lens came back with
// Spherical's bounds); ignored runs the projection without the lens; Geometric Distortion on
// Collage and Reposition is the planar lens path (phase 2b-5, AutoAlignPlanarLens.swift): each
// layer is corrected within its own frame, which keeps its size (take3's D rows), then warped by
// the similarity or translation.
//
// THE STEP runs inside `AutoAlignWork.reprojectedDocument` (AutoAlignReprojection.swift) or
// `planarLensDocument` (AutoAlignPlanarLens.swift), right after the registration: one
// `fit_distortion` request over the reference (its own anchor, the identity, first) and every
// registered layer after its anchor, with the Vision maps, planned on the source and run as its
// own admitted child operation. The core fits ONE radial coefficient for
// the stack (`align_distortion.rs`) and answers refined maps between the CORRECTED frames, which
// replace the Vision maps for the focal fit and the rotations (on the plane, the maps the
// similarity or translation is reduced from); every `project_layer` then carries the coefficient,
// so the reprojection undoes the lens.

extension AutoAlign {
    /// The dialog's Lens Correction choices.
    enum LensCorrection: CaseIterable {
        case vignette, distortion

        var title: String {
            switch self {
            case .vignette: return "Vignette Removal"
            case .distortion: return "Geometric Distortion"
            }
        }
    }

    /// What a projection does with a lens correction (THE TABLE).
    enum LensAvailability { case applies, ignored }

    /// The coefficient the core fitted (`fit_distortion`): a layer pixel p is
    /// the source of the corrected point u, p = c + R·q·(1 + coefficient·|q|²).
    struct DistortionFit {
        let coefficient: Double
        let matches: Int
        let residual: Double
        let layers: [Int]
    }

    /// The core's `fit_distortion` request over `entries` (each a layer, the
    /// layer it was registered against, and its map, canvas → reference
    /// canvas), in order: the reference first, every layer after its anchor.
    static func distortionRequest(_ entries: [(layer: Int, anchor: Int, map: Homography)])
        throws -> String
    {
        let fields: [String: Any] = [
            "op": "fit_distortion",
            "layers": entries.map {
                ["layer": $0.layer, "anchor": $0.anchor, "map": $0.map.m] as [String: Any]
            },
        ]
        let data = try JSONSerialization.data(withJSONObject: fields)
        return String(decoding: data, as: UTF8.self)
    }

    /// The fit and each listed layer's corrected map a `fit_distortion`
    /// answer carries.
    static func distortionFit(_ answer: [String: Any]) throws
        -> (fit: DistortionFit, maps: [Int: Homography])
    {
        guard let coefficient = (answer["coefficient"] as? NSNumber)?.doubleValue,
              let matches = (answer["matches"] as? NSNumber)?.intValue,
              let residual = (answer["residual"] as? NSNumber)?.doubleValue,
              let layers = (answer["layers"] as? [Any])?.compactMap({ ($0 as? NSNumber)?.intValue }),
              let rows = answer["maps"] as? [Any], rows.count == layers.count
        else {
            throw DoctoredCoreError(message: "\(title) could not fit Geometric Distortion.")
        }
        var maps: [Int: Homography] = [:]
        for (layer, row) in zip(layers, rows) {
            let m = (row as? [Any] ?? []).compactMap { ($0 as? NSNumber)?.doubleValue }
            guard m.count == 9, m.allSatisfy(\.isFinite) else {
                throw DoctoredCoreError(message: "\(title) could not fit Geometric Distortion.")
            }
            maps[layer] = Homography(m: m)
        }
        return (DistortionFit(coefficient: coefficient, matches: matches, residual: residual,
                              layers: layers), maps)
    }
}

extension AutoAlign.Registration {
    /// THE TABLE: what this projection does with `correction` (take3).
    func lens(_ correction: AutoAlign.LensCorrection) -> AutoAlign.LensAvailability {
        switch (self, correction) {
        case (.auto, _), (.perspective, _), (.cylindrical, _), (.spherical, _): return .applies
        case (.collage, .vignette), (.translation, .vignette): return .ignored
        case (.collage, .distortion), (.translation, .distortion): return .applies
        }
    }
}

extension AutoAlignWork {
    /// Geometric Distortion's plan over `entries` (`distortionRequest`'s), on
    /// the source: the request and its scratch. Throws the core's refusal (a
    /// layer it cannot read) before any pixel work.
    func distortionPlan(_ entries: [(layer: Int, anchor: Int, map: Homography)],
                        budget: (limit: UInt64, other: UInt64))
        throws -> (request: String, scratch: UInt64)
    {
        let command = try AutoAlign.distortionRequest(entries)
        let plan = try source.fitDistortionPlan(request: command, budget: budget)
        return (command, try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes"))
    }

    /// The step: the planned `request` admitted at `scratch` and fitted on
    /// the source as one child operation over `progress`. Changes no
    /// document; throws the core's refusal (too little overlap) or a cancel.
    func fitDistortion(request: String, scratch: UInt64, budget: (limit: UInt64, other: UInt64),
                       control: RasterJobControl, progress: ClosedRange<Double>)
        throws -> (fit: AutoAlign.DistortionFit, maps: [Int: Homography])
    {
        guard !control.isCancelled else { throw JobCancelled() }
        _ = try RasterResources.shared.admit(scratch: scratch)
        let source = self.source
        let answer = try control.withChildOperation(from: progress.lowerBound,
                                                    to: progress.upperBound) { child in
            guard child.setScratchLimit(scratch) else {
                throw DoctoredCoreError(message: "Could not bound alignment work.")
            }
            return try source.fittingDistortion(request: request, budget: budget, control: child)
        }
        return try AutoAlign.distortionFit(answer)
    }
}
