import AppKit

// Edit ▸ Auto-Align Layers… ▸ Collage or Reposition with Geometric Distortion, measured
// against take3's availability matrix (`~/devel/doctored-fixtures/photoshop/
// panorama-564/take3`): Photoshop APPLIED the lens on both, and each layer kept its own frame
// size (Reposition's D rows are 600 × 400, the frames' own). The correction happens within
// each layer's frame and the projection's map then places it. Collage's map is the similarity, Reposition's the
// translation, each reduced from the map the core's `fit_distortion` refines between the CORRECTED
// frames.
//
// THE PATH. Each floating layer is registered against the reference as Perspective (phase 3a:
// no translation or similarity of the distorted frames follows the lens); the core fits ONE radial coefficient over the reference
// and every registered layer, all anchored to the reference (`align_distortion.rs`); each refined
// map is reduced (`planarMap`); and every layer, the reference too (the identity), is corrected
// and placed in ONE core resample (`project_layer`'s `"projection":"plane"`,
// `align_projection.rs`). The plans, the union check, the admission and the canvas are the
// surfaces' (`projectionCharges` and `projectedDocument`, AutoAlignReprojection.swift).
//
// A stack where no layer moves (a twin) says nothing about the lens, so it is refused before the
// fit (`planarStillMessage`) and nothing changes.

extension AutoAlign {
    /// Thrown when no registered layer moves a rect corner past
    /// `unchangedThreshold`.
    static let planarStillMessage = "Geometric Distortion needs a layer that moves against "
        + "the reference; none does here, so the overlaps say nothing about the lens. Turn "
        + "off Geometric Distortion to align them. Nothing was changed."

    /// The map the projection places a corrected layer with: `.collage` the
    /// similarity closest to `map` over `rect` (`similarityFit(over:)`),
    /// `.translation` the translation (`translationFit(over:)`); any other
    /// projection, nil.
    static func planarMap(_ map: Homography, registration: Registration, over rect: CGRect)
        -> Homography?
    {
        switch registration {
        case .collage: return map.similarityFit(over: rect)
        case .translation: return map.translationFit(over: rect)
        case .auto, .perspective, .cylindrical, .spherical: return nil
        }
    }
}

extension AutoAlignWork {
    /// `alignedDocument` for Collage and Reposition with Geometric
    /// Distortion (the file header). Runs on the worker with the caller's
    /// compound `control`; reads only captured values.
    ///
    /// 1. Each floating layer, ascending, is registered to the reference
    ///    as Perspective with Geometric Distortion on (`register`); a layer
    ///    with no pixels, no answer or no quad is `unregistered`. None
    ///    registered answers `source`.
    /// 2. No registered layer moving a corner past `unchangedThreshold`
    ///    throws `planarStillMessage`; nothing changes.
    /// 3. `fit_distortion` over the reference (the identity) and every
    ///    registered layer anchored to it, ascending, under its own admitted
    ///    scratch; each refined map is reduced by `planarMap` (nil:
    ///    `unregistered`).
    /// 4. The reference (the identity) and every aligned layer are planned
    ///    and projected onto the plane with the coefficient, and the canvas
    ///    becomes their union (`projectionCharges`, `projectedDocument`).
    ///
    /// Registration takes 40% of `share`, the fit 10%; the warps share the
    /// rest evenly.
    func planarLensDocument(control: RasterJobControl, registration: UInt64,
                            share: ClosedRange<Double>) throws
        -> (document: RasterDocument, outcome: AutoAlign.Outcome)
    {
        let source = self.source
        let request = self.request
        let layers = self.layers
        let space = self.space
        guard let budget = projectionBudget, layers[request.reference] != nil else {
            throw DoctoredCoreError(message: "Capture the layers before alignment work starts.")
        }
        let analysis = AutoAlign.analysis(canvasWidth: source.width, canvasHeight: source.height)
        let referenceName = layers[request.reference]?.name ?? "The reference"
        let span = share.upperBound - share.lowerBound
        let registering = share.lowerBound...(share.lowerBound + 0.4 * span)
        let distorting = registering.upperBound...(registering.upperBound + 0.1 * span)
        func frame(_ layer: Int) throws -> CGImage? {
            try autoreleasepool {
                try AutoAlign.analysisFrame(of: source, layer: layer, scale: analysis.scale,
                                            space: space)
            }
        }
        var registered: [Int: (homography: Homography, method: AutoAlign.Method)] = [:]
        /// What step 3's reduction (`planarMap`) makes of every map.
        let planarMethod: AutoAlign.Method =
            request.registration == .collage ? .similarity : .translation
        var unregistered: [AutoAlign.Skipped] = []
        func skip(_ layer: Int, _ reason: String) {
            unregistered.append(.init(layer: layer, name: layers[layer]?.name ?? "Layer \(layer)",
                                      reason: reason))
        }
        // Step 1, against the reference. Its frame lives only in this pass.
        try {
            guard let referenceFrame = try frame(request.reference) else {
                throw DoctoredCoreError(message: AutoAlign.emptyReferenceMessage(referenceName))
            }
            let count = Double(max(request.floating.count, 1))
            let width = registering.upperBound - registering.lowerBound
            for (position, layer) in request.floating.enumerated() {
                guard !control.isCancelled else { throw JobCancelled() }
                guard let rect = layers[layer]?.rect else { continue }
                let found = try control.withChildOperation(
                    from: registering.lowerBound + width * Double(position) / count,
                    to: registering.lowerBound + width * Double(position + 1) / count
                ) { _ in
                    try autoreleasepool { () throws -> (Bool, (Homography, AutoAlign.Method)?) in
                        guard let floating = try frame(layer) else { return (false, nil) }
                        // Registered as Perspective: the lens leaves a residual no
                        // translation or similarity of the DISTORTED frames follows,
                        // and step 3 reduces the refined map after the fit.
                        return (true, AutoAlign.register(
                            floating: floating, reference: referenceFrame,
                            registration: .perspective, distortion: true))
                    }
                }
                let map = found.1.map { $0.0.rescaled(fromAnalysisScale: analysis.scale) }
                if !found.0 {
                    skip(layer, "the layer has no pixels")
                } else if let map, map.quad(of: rect) != nil {
                    registered[layer] = (map, planarMethod)
                } else {
                    skip(layer, AutoAlign.unregisteredReason)
                }
            }
        }()
        guard !control.isCancelled else { throw JobCancelled() }
        // Step 1's end: nothing registered changes nothing.
        guard !registered.isEmpty else {
            let outcome = AutoAlign.Outcome(
                reference: request.reference, aligned: [], unchanged: [],
                unregistered: unregistered.sorted { $0.layer < $1.layer },
                registrationBytes: registration, planBytes: registration, canvas: nil)
            return (source, outcome)
        }
        // Step 2: a stack no layer moves in says nothing about the lens.
        let ordered = registered.keys.sorted()
        let moves = ordered.contains { layer in
            guard let rect = layers[layer]?.rect,
                  let quad = registered[layer]?.homography.quad(of: rect) else { return false }
            return zip(quad, Homography.corners(of: rect)).contains {
                hypot($0.x - $1.x, $0.y - $1.y) > AutoAlign.unchangedThreshold
            }
        }
        guard moves else { throw DoctoredCoreError(message: AutoAlign.planarStillMessage) }
        // Step 3: the fit, every layer anchored to the reference.
        let identity = Homography(m: [1, 0, 0, 0, 1, 0, 0, 0, 1])
        var entries: [(layer: Int, anchor: Int, map: Homography)] = [
            (request.reference, request.reference, identity),
        ]
        for layer in ordered {
            guard let map = registered[layer]?.homography else { continue }
            entries.append((layer, request.reference, map))
        }
        let planned = try distortionPlan(entries, budget: budget)
        let fitted = try fitDistortion(request: planned.request, scratch: planned.scratch,
                                       budget: budget, control: control, progress: distorting)
        let coefficient = fitted.fit.coefficient
        func command(_ layer: Int, _ map: Homography) throws -> String {
            let fields: [String: Any] = [
                "op": "project_layer", "layer": layer, "projection": "plane", "map": map.m,
                "distortion": coefficient,
            ]
            let data = try JSONSerialization.data(withJSONObject: fields)
            return String(decoding: data, as: UTF8.self)
        }
        var jobs: [(layer: Int, request: String)] = [
            (request.reference, try command(request.reference, identity)),
        ]
        var aligned: [AutoAlign.Aligned] = []
        for layer in ordered {
            guard let (name, rect) = layers[layer], let method = registered[layer]?.method,
                  let refined = fitted.maps[layer],
                  let map = AutoAlign.planarMap(refined, registration: request.registration,
                                                over: rect),
                  let quad = map.quad(of: rect)
            else {
                skip(layer, AutoAlign.unregisteredReason)
                continue
            }
            jobs.append((layer, try command(layer, map)))
            aligned.append(.init(layer: layer, name: name, method: method, homography: map,
                                 quad: quad))
        }
        // Step 4: every plan and the union before any warp, then the warps
        // and the canvas.
        let (charges, held) = try projectionCharges(jobs, held: 0, budget: budget)
        let projected = try projectedDocument(
            jobs, charges: charges, held: held, from: source,
            layers: [request.reference] + aligned.map(\.layer), budget: budget, control: control,
            progress: distorting.upperBound...share.upperBound)
        guard !control.isCancelled else { throw JobCancelled() }
        let outcome = AutoAlign.Outcome(
            reference: request.reference, aligned: aligned, unchanged: [],
            unregistered: unregistered.sorted { $0.layer < $1.layer },
            registrationBytes: registration,
            planBytes: max(registration, planned.scratch, charges.max() ?? 0,
                           projected.canvasCharge),
            canvas: projected.canvas, distortion: fitted.fit)
        return (projected.document, outcome)
    }
}
