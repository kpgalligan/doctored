import AppKit

// Edit ▸ Auto-Align Layers…'s Cylindrical and Spherical projections, built
// against take2's C rows (`~/devel/doctored-fixtures/photoshop/panorama-564/take2`: three pinhole
// views 600 × 400, f = 600, yaw −20/0/+20°; Cylindrical 974 × 399, Spherical 974 × 385, the layers
// at x 0, 209 and 419, the bottom one the reference). Photoshop's Spherical on take1's pure shift
// gave 1205 × 809; a pure shift here refuses with `focalRefusal`, as Cylindrical did there.
// Take2's rows are checked through the core alone (`geometry_align_projection.rs`): its 20° steps
// shift a frame about 36% of its width, past `register`'s reach (about 10–15° at f 600), so the
// end-to-end drive uses ±10° and the 20° frames wait on phase 3's reach.
//
// THE MODEL. Every layer is a pinhole view from one point, its principal point at its rect's
// centre, and every layer shares one focal length f. Registration is `AutoAlign.register`'s
// Perspective answer, on the ORIGINAL frames. The planar map N between centred coordinates of a
// layer and of the reference is then K·R·K⁻¹ with K = diag(f, f, 1) and R the rotation from the
// layer's rays to the reference's, so `AutoAlignSphere` finds f as the focal that makes
// K⁻¹·N·K closest to a rotation, and R as that matrix's polar factor. The core reprojects each
// layer, the reference too, onto the reference camera's cylinder or sphere (`project_layer`,
// `align_projection.rs`), and the canvas becomes the union of the projected layers.

/// The focal length and rotations of a stack of pinhole views, from the
/// planar maps `AutoAlign.register` measured.
enum AutoAlignSphere {
    /// The focal lengths tried, as multiples of the reference's diagonal:
    /// from a fisheye's 0.2 to a long lens's 20 (a 600 px focal on take2's
    /// 721 px diagonal is 0.83).
    static let focalRange: ClosedRange<Double> = 0.2...20
    /// Log-spaced samples over `focalRange`: 401 puts neighbours 2.3% apart,
    /// so the golden-section search starts inside a bracket of under 5%.
    static let focalSamples = 401

    /// N = T(−c₀)·H·T(cᵢ): `map` (floating canvas → reference canvas)
    /// between coordinates centred on each layer's own centre; nil when it
    /// is not a usable 3×3 or its determinant is not positive.
    private static func centred(_ map: Homography, centre: CGPoint, referenceCentre: CGPoint)
        -> [Double]?
    {
        guard map.m.count == 9, map.m.allSatisfy(\.isFinite) else { return nil }
        let into: [Double] = [1, 0, Double(centre.x), 0, 1, Double(centre.y), 0, 0, 1]
        let back: [Double] = [1, 0, -Double(referenceCentre.x), 0, 1, -Double(referenceCentre.y),
                              0, 0, 1]
        let n = Homography.product(back, Homography.product(map.m, into))
        return determinant(n) > 0 ? n : nil
    }

    /// M(g) = diag(1/g, 1/g, 1)·N·diag(g, g, 1), scaled to determinant 1.
    private static func normalised(_ n: [Double], focal g: Double) -> [Double] {
        let m: [Double] = [
            n[0], n[1], n[2] / g,
            n[3], n[4], n[5] / g,
            n[6] * g, n[7] * g, n[8],
        ]
        let scale = pow(determinant(m), -1.0 / 3.0)
        return m.map { $0 * scale }
    }

    private static func determinant(_ m: [Double]) -> Double {
        m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
            + m[2] * (m[3] * m[7] - m[4] * m[6])
    }

    private static func transposed(_ m: [Double]) -> [Double] {
        [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]
    }

    /// ‖M·Mᵀ − I‖²_F: how far M is from a rotation.
    private static func departure(_ m: [Double]) -> Double {
        let p = Homography.product(m, transposed(m))
        var sum = 0.0
        for i in 0..<3 {
            for j in 0..<3 {
                let d = p[i * 3 + j] - (i == j ? 1 : 0)
                sum += d * d
            }
        }
        return sum
    }

    /// The one focal (canvas pixels) that makes every `maps` entry closest
    /// to a rotation: E(g) = Σ‖M(g)·M(g)ᵀ − I‖²_F, sampled at `focalSamples`
    /// log-spaced values of `focalRange` × `diagonal`, then a 60-step
    /// golden-section search between the best sample's neighbours. nil when
    /// no map is usable, when the best sample is the first or the last (no
    /// minimum inside the range), or when E does not vary by more than 1e-12
    /// (no turn: a pure shift, or identical layers).
    static func focal(maps: [(map: Homography, centre: CGPoint)], referenceCentre: CGPoint,
                      diagonal: Double) -> Double? {
        let ns = maps.compactMap {
            centred($0.map, centre: $0.centre, referenceCentre: referenceCentre)
        }
        guard !ns.isEmpty, diagonal.isFinite, diagonal > 0, focalSamples >= 3 else { return nil }
        func energy(_ g: Double) -> Double {
            ns.reduce(0) { $0 + departure(normalised($1, focal: g)) }
        }
        let low = log(focalRange.lowerBound * diagonal)
        let high = log(focalRange.upperBound * diagonal)
        let samples = (0..<focalSamples).map { k -> (g: Double, e: Double) in
            let g = exp(low + (high - low) * Double(k) / Double(focalSamples - 1))
            return (g, energy(g))
        }
        guard samples.allSatisfy({ $0.e.isFinite }),
              let best = samples.indices.min(by: { samples[$0].e < samples[$1].e }),
              let most = samples.map(\.e).max() else { return nil }
        guard best > 0, best < focalSamples - 1, most - samples[best].e > 1e-12 else { return nil }
        var a = samples[best - 1].g, b = samples[best + 1].g
        let ratio = (5.0.squareRoot() - 1) / 2
        var c = b - ratio * (b - a), d = a + ratio * (b - a)
        var ec = energy(c), ed = energy(d)
        for _ in 0..<60 {
            if ec < ed {
                b = d; d = c; ed = ec
                c = b - ratio * (b - a); ec = energy(c)
            } else {
                a = c; c = d; ec = ed
                d = a + ratio * (b - a); ed = energy(d)
            }
        }
        let focal = (a + b) / 2
        return focal.isFinite && focal > 0 ? focal : nil
    }

    /// The rotation from the layer's rays to the reference's: the polar
    /// factor of M(`focal`), by X ← (X + X⁻ᵀ)/2 until no entry changes by
    /// more than 1e-12 (at most 50 steps), row-major. nil when the map is not
    /// usable or the iteration does not settle.
    static func rotation(map: Homography, centre: CGPoint, referenceCentre: CGPoint,
                         focal: Double) -> [Double]? {
        guard focal.isFinite, focal > 0,
              let n = centred(map, centre: centre, referenceCentre: referenceCentre) else {
            return nil
        }
        var x = normalised(n, focal: focal)
        for _ in 0..<50 {
            let det = determinant(x)
            guard det.isFinite, det != 0 else { return nil }
            // X⁻ᵀ is the cofactor matrix over the determinant.
            let cofactor: [Double] = [
                x[4] * x[8] - x[5] * x[7], x[5] * x[6] - x[3] * x[8], x[3] * x[7] - x[4] * x[6],
                x[2] * x[7] - x[1] * x[8], x[0] * x[8] - x[2] * x[6], x[1] * x[6] - x[0] * x[7],
                x[1] * x[5] - x[2] * x[4], x[2] * x[3] - x[0] * x[5], x[0] * x[4] - x[1] * x[3],
            ]
            let next = zip(x, cofactor).map { ($0 + $1 / det) / 2 }
            let change = zip(next, x).map { abs($0 - $1) }.max() ?? 0
            x = next
            if change <= 1e-12 { return x.allSatisfy(\.isFinite) ? x : nil }
        }
        return nil
    }
}

extension AutoAlign {
    /// The refusal when no layer registers as a homography or the maps give
    /// no focal: the refusal for Cylindrical or Spherical on a pure shift,
    /// plus the other cause, frames that overlap too little or too widely
    /// for `register`.
    /// When the surface came from a lens correction (Auto or Perspective,
    /// `surface(for:vignette:distortion:)`), it names the sphere and the way
    /// back to Perspective: turning off the lenses that are on.
    static func focalRefusal(_ projection: Registration, vignette: Bool = false,
                             distortion: Bool = false) -> String {
        let chosen = surface(for: projection, vignette: vignette, distortion: distortion)
            ?? projection
        let shape = chosen == .spherical ? "sphere" : "cylinder"
        let lenses = [(vignette, LensCorrection.vignette), (distortion, .distortion)]
            .filter { $0.0 && projection.lens($0.1) == .applies }.map(\.1.title)
        let advice = !lenses.isEmpty && !projection.reprojects
            ? "Turn off \(lenses.joined(separator: " and ")) to align them with Perspective."
            : "Use Perspective or Reposition."
        return "The layers do not turn about one point, or they overlap too little or too "
            + "widely to align, so there is no lens to project onto a \(shape). \(advice) "
            + "Nothing was changed."
    }

    /// The refusal when no layer registers because every one the lens test
    /// judged carries lens distortion while Geometric Distortion is off
    /// (`lensDistortionReason`).
    static let lensRefusal = "No layer could be aligned: "
        + "\(lensDistortionReason). Nothing was changed."

    /// `planar`: Geometric Distortion on Collage or Reposition
    /// (AutoAlignPlanarLens.swift), which corrects the reference too.
    static func reprojectionLockMessage(_ names: String, planar: Bool = false) -> String {
        planar
            ? "\(names) cannot move because of a position lock. Geometric Distortion corrects "
                + "every layer, the reference too: unlock it first."
            : "\(names) cannot move because of a position lock. Cylindrical and Spherical move "
                + "every layer, the reference too: unlock it first."
    }

    static func placedSourceMessage(_ name: String, planar: Bool = false) -> String {
        planar
            ? "\(name) is a placed image. Geometric Distortion corrects its pixels: rasterize it "
                + "first."
            : "\(name) is a placed image. Cylindrical and Spherical reproject its pixels: "
                + "rasterize it first."
    }

    /// The union of the content bounds (`layerBounds`) of `layers` only, the
    /// canvas left out, in the canvas's coordinates; nil when nothing is
    /// opaque or the union is the canvas.
    static func projectedCanvas(of doc: RasterDocument, layers: [Int]) -> CGRect? {
        var union: CGRect?
        for index in layers {
            guard let bounds = doc.layerBounds(index), bounds.width > 0, bounds.height > 0 else {
                continue
            }
            let rect = CGRect(x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height)
            union = union.map { $0.union(rect) } ?? rect
        }
        guard let union, union != CGRect(x: 0, y: 0, width: doc.width, height: doc.height) else {
            return nil
        }
        return union
    }
}

extension AutoAlignWork {
    /// `alignedDocument` for Cylindrical and Spherical. Runs on the worker
    /// with the caller's compound `control`; reads only captured values.
    ///
    /// 1. Each floating layer, ascending, is registered to the reference
    ///    (`register`, Perspective); only a homography answer counts. A layer
    ///    answered nil or a translation is tried once against the
    ///    homography-registered layer nearest in index (ties to the lower)
    ///    and the two maps composed; otherwise it is `unregistered` and never
    ///    reprojected.
    /// 2. No homography, or no focal (`AutoAlignSphere.focal`), refuses
    ///    `focalRefusal`; nothing changes.
    /// 3. The reference (R = I) and every registered layer are planned; the
    ///    union of the projected rects must pass 100 MP and the channel
    ///    budget (`tooBigMessage`), and every plan must pass, before any warp.
    /// 4. Each layer is reprojected under its own admitted charge, and the
    ///    canvas becomes `projectedCanvas` (Reveal All's `canvasResized`).
    ///    The job holds every layer it has reprojected while it projects the
    ///    next, so a warp's charge is its own `scratch_bytes` plus the
    ///    earlier plans' `output_bytes`, and the canvas step's is every
    ///    output plus its own plan's scratch; `planBytes` is the largest.
    ///
    /// With Geometric Distortion (AutoAlignDistortion.swift) the surface is
    /// `request.surface`; right after step 1 the core's `fit_distortion` runs
    /// on the source under its own admitted scratch over the reference and
    /// every registered layer, each after the layer it was registered
    /// against (the reference, or the chained neighbour), with the Vision
    /// maps. Its maps, between the CORRECTED frames, replace those before
    /// step 2, so the focal, the rotations and the `aligned` rows read them,
    /// and every `project_layer` carries its coefficient as `distortion`.
    ///
    /// With Vignette Removal (AutoAlignVignette.swift) the surface is
    /// `request.surface`; after step 2 the core's `remove_vignette` is
    /// planned on the source (its refusals before any pixel work) with each
    /// aligned layer's VISION map (it pairs raw pixels through a lens-free
    /// homography), step 3's held bytes start at its `output_bytes`, and step
    /// 4 first runs it under its own admitted scratch, then reprojects its
    /// result (the source when it changed no sample).
    ///
    /// Registration takes 40% of `share`, Geometric Distortion 10%, Vignette
    /// Removal 10%; the warps share the rest evenly.
    func reprojectedDocument(control: RasterJobControl, registration: UInt64,
                             share: ClosedRange<Double>) throws
        -> (document: RasterDocument, outcome: AutoAlign.Outcome)
    {
        let source = self.source
        let request = self.request
        let layers = self.layers
        let space = self.space
        guard let budget = projectionBudget,
              let (_, referenceRect) = layers[request.reference] else {
            throw DoctoredCoreError(message: "Capture the layers before alignment work starts.")
        }
        let analysis = AutoAlign.analysis(canvasWidth: source.width, canvasHeight: source.height)
        let referenceName = layers[request.reference]?.name ?? "The reference"
        let span = share.upperBound - share.lowerBound
        let registering = share.lowerBound...(share.lowerBound + 0.4 * span)
        let distorting = registering.upperBound...(registering.upperBound
            + (request.distortion ? 0.1 * span : 0))
        let vignetting = distorting.upperBound...(distorting.upperBound
            + (request.vignette ? 0.1 * span : 0))
        func frame(_ layer: Int) throws -> CGImage? {
            try autoreleasepool {
                try AutoAlign.analysisFrame(of: source, layer: layer, scale: analysis.scale,
                                            space: space)
            }
        }
        /// The canvas map of a homography answer, else nil.
        func planar(_ found: (Homography, AutoAlign.Method)?) -> Homography? {
            guard let (map, method) = found, method == .homography else { return nil }
            return map.rescaled(fromAnalysisScale: analysis.scale)
        }
        var registered: [Int: Homography] = [:]
        /// The layer each registered layer was registered against.
        var anchors: [Int: Int] = [:]
        var pending: [Int] = []
        /// The layers the lens test refused with Geometric Distortion off.
        var lensRefused: Set<Int> = []
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
                let found = try control.withChildOperation(
                    from: registering.lowerBound + width * Double(position) / count,
                    to: registering.lowerBound + width * Double(position + 1) / count
                ) { _ in
                    try autoreleasepool { () throws -> (Bool, AutoAlign.Registered?) in
                        guard let floating = try frame(layer) else { return (false, nil) }
                        return (true, AutoAlign.registered(
                            floating: floating, reference: referenceFrame,
                            registration: .perspective, distortion: request.distortion))
                    }
                }
                if case let .aligned(map, method)? = found.1, let map = planar((map, method)) {
                    registered[layer] = map
                    anchors[layer] = request.reference
                } else if !found.0 {
                    skip(layer, "the layer has no pixels")
                } else {
                    if case let .refused(reason)? = found.1,
                       reason == AutoAlign.lensDistortionReason {
                        lensRefused.insert(layer)
                    }
                    pending.append(layer)
                }
            }
        }()
        // The chained retry, against the nearest layer the first pass registered.
        let first = registered
        for layer in pending {
            guard !control.isCancelled else { throw JobCancelled() }
            let nearest = first.keys.sorted().min {
                (abs($0 - layer), $0) < (abs($1 - layer), $1)
            }
            let chained: Homography? = try nearest.flatMap { neighbour in
                guard let toReference = first[neighbour] else { return nil }
                return try autoreleasepool { () throws -> Homography? in
                    guard let floating = try frame(layer), let anchor = try frame(neighbour),
                          let toNeighbour = planar(AutoAlign.register(
                              floating: floating, reference: anchor, registration: .perspective,
                              distortion: request.distortion))
                    else { return nil }
                    return Homography(m: Homography.product(toReference.m, toNeighbour.m))
                }
            }
            if let chained, let nearest {
                registered[layer] = chained
                anchors[layer] = nearest
            } else {
                skip(layer, lensRefused.contains(layer)
                    ? AutoAlign.lensDistortionReason : AutoAlign.unregisteredReason)
            }
        }
        // Nothing registered because the lens test refused: name the lens.
        if registered.isEmpty, !lensRefused.isEmpty {
            throw DoctoredCoreError(message: AutoAlign.lensRefusal)
        }
        guard !control.isCancelled else { throw JobCancelled() }
        // Geometric Distortion: the reference first, then the layers
        // registered against it, then the chained ones (each after its
        // anchor). The Vision maps are kept for Vignette Removal.
        let vision = registered
        var distortion: (fit: AutoAlign.DistortionFit, scratch: UInt64)?
        if request.distortion, !registered.isEmpty {
            let identity = Homography(m: [1, 0, 0, 0, 1, 0, 0, 0, 1])
            let direct = registered.keys.filter { anchors[$0] == request.reference }.sorted()
            let chained = registered.keys.filter { anchors[$0] != request.reference }.sorted()
            var entries: [(layer: Int, anchor: Int, map: Homography)] = [
                (request.reference, request.reference, identity),
            ]
            for layer in direct + chained {
                guard let map = registered[layer], let anchor = anchors[layer] else { continue }
                entries.append((layer, anchor, map))
            }
            let planned = try distortionPlan(entries, budget: budget)
            let fitted = try fitDistortion(request: planned.request, scratch: planned.scratch,
                                           budget: budget, control: control,
                                           progress: distorting)
            for (layer, map) in fitted.maps where layer != request.reference {
                registered[layer] = map
            }
            distortion = (fitted.fit, planned.scratch)
        }
        // Step 2.
        let referenceCentre = CGPoint(x: referenceRect.midX, y: referenceRect.midY)
        let ordered = registered.keys.sorted()
        guard !ordered.isEmpty,
              let focal = AutoAlignSphere.focal(
                  maps: ordered.compactMap { layer in
                      guard let map = registered[layer], let rect = layers[layer]?.rect else {
                          return nil
                      }
                      return (map, CGPoint(x: rect.midX, y: rect.midY))
                  },
                  referenceCentre: referenceCentre,
                  diagonal: hypot(Double(referenceRect.width), Double(referenceRect.height)))
        else {
            throw DoctoredCoreError(message: AutoAlign.focalRefusal(
                request.registration, vignette: request.vignette,
                distortion: request.distortion))
        }
        // The requests: the reference first (R = I), then each registered layer.
        let projection = request.surface == .spherical ? "spherical" : "cylindrical"
        let coefficient = distortion?.fit.coefficient
        // The reference's axis on the pixel centre nearest its rect's centre:
        // the index-space rows the takes measured (`align_projection.rs`, phase 2b-5).
        let origin = [floor(Double(referenceRect.midX)) + 0.5,
                      floor(Double(referenceRect.midY)) + 0.5]
        func command(_ layer: Int, _ rotation: [Double], centre: CGPoint) throws -> String {
            var fields: [String: Any] = [
                "op": "project_layer", "layer": layer, "projection": projection, "focal": focal,
                "rotation": rotation, "centre": [Double(centre.x), Double(centre.y)],
                "origin": origin,
            ]
            if let coefficient { fields["distortion"] = coefficient }
            let data = try JSONSerialization.data(withJSONObject: fields)
            return String(decoding: data, as: UTF8.self)
        }
        var jobs: [(layer: Int, request: String)] = [
            (request.reference, try command(request.reference, [1, 0, 0, 0, 1, 0, 0, 0, 1],
                                            centre: referenceCentre)),
        ]
        var aligned: [AutoAlign.Aligned] = []
        for layer in ordered {
            guard let map = registered[layer], let (name, rect) = layers[layer] else { continue }
            let centre = CGPoint(x: rect.midX, y: rect.midY)
            guard let quad = map.quad(of: rect),
                  let polar = AutoAlignSphere.rotation(
                      map: map, centre: centre, referenceCentre: referenceCentre, focal: focal)
            else {
                skip(layer, AutoAlign.unregisteredReason)
                continue
            }
            // The polar factor maps the layer's rays to the reference's; the
            // core takes the reverse.
            let rotation = [polar[0], polar[3], polar[6], polar[1], polar[4], polar[7],
                            polar[2], polar[5], polar[8]]
            jobs.append((layer, try command(layer, rotation, centre: centre)))
            aligned.append(.init(layer: layer, name: name, method: .homography, homography: map,
                                 quad: quad, rotation: rotation))
        }
        // Vignette Removal's plan, on the source, before any pixel work.
        let vignette = request.vignette
            ? try vignettePlan(maps: aligned.compactMap { row in
                vision[row.layer].map { (row.layer, $0) }
            }, budget: budget)
            : nil
        // Step 3: every plan, and the union of their rects, before any warp.
        let (charges, held) = try projectionCharges(jobs, held: vignette?.output ?? 0,
                                                    budget: budget)
        // Step 4: Vignette Removal, then the warps.
        var running = source
        if let vignette {
            running = try removeVignette(request: vignette.request, scratch: vignette.scratch,
                                         budget: budget, control: control,
                                         progress: vignetting) ?? source
        }
        let projected = try projectedDocument(
            jobs, charges: charges, held: held, from: running,
            layers: [request.reference] + aligned.map(\.layer), budget: budget, control: control,
            progress: vignetting.upperBound...share.upperBound)
        running = projected.document
        let canvas = projected.canvas, canvasCharge = projected.canvasCharge
        guard !control.isCancelled else { throw JobCancelled() }
        let outcome = AutoAlign.Outcome(
            reference: request.reference, aligned: aligned, unchanged: [],
            unregistered: unregistered.sorted { $0.layer < $1.layer },
            registrationBytes: registration,
            planBytes: max(registration, distortion?.scratch ?? 0, vignette?.scratch ?? 0,
                           charges.max() ?? 0, canvasCharge),
            canvas: canvas, focal: focal, surface: request.surface, vignette: vignette?.fit,
            distortion: distortion?.fit)
        return (running, outcome)
    }

    /// Step 3: each job's plan on `source`; its charge is `held` plus its
    /// own `scratch_bytes`, and `held` grows by its `output_bytes`. The union
    /// of the plans' rects is refused past 100 MP (`tooBigMessage`) or the
    /// channel budget, before any warp. Answers the charges and the final
    /// `held`.
    func projectionCharges(_ jobs: [(layer: Int, request: String)], held: UInt64,
                           budget: (limit: UInt64, other: UInt64)) throws
        -> (charges: [UInt64], held: UInt64)
    {
        let source = self.source
        var charges: [UInt64] = []
        var held = held
        var union: CGRect?
        for job in jobs {
            let plan = try source.projectLayerPlan(request: job.request, budget: budget)
            charges.append(try PrecisionDocumentDescriptor.sum(
                held, PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")))
            held = try PrecisionDocumentDescriptor.sum(
                held, PrecisionDocumentDescriptor.integer(plan, "output_bytes"))
            let values = (plan["rect"] as? [Any] ?? []).compactMap { ($0 as? NSNumber)?.doubleValue }
            guard values.count == 4 else {
                throw DoctoredCoreError(message: "\(AutoAlign.title) could not plan the projection.")
            }
            let placed = CGRect(x: values[0], y: values[1], width: values[2], height: values[3])
            union = union.map { $0.union(placed) } ?? placed
        }
        if let union {
            let width = Int(union.width), height = Int(union.height)
            guard width <= RasterImage.maxResizePixels / max(height, 1) else {
                throw DoctoredCoreError(message: AutoAlign.tooBigMessage(width: width, height: height))
            }
            if let reason = source.channelBudgetRefusal(width: width, height: height) {
                throw DoctoredCoreError(message: reason)
            }
        }
        return (charges, held)
    }

    /// Step 4: each job admitted at its charge and run on `running` as a
    /// child operation, the jobs sharing `progress` evenly; then the canvas
    /// is set to `projectedCanvas` of `layers` (Reveal All's
    /// `canvasResized`), charged at `held` plus its own plan's bookkeeping
    /// scratch at high depth. Answers the document, the new canvas (nil when
    /// it did not change) and the canvas step's charge.
    func projectedDocument(_ jobs: [(layer: Int, request: String)], charges: [UInt64],
                           held: UInt64, from running: RasterDocument, layers: [Int],
                           budget: (limit: UInt64, other: UInt64), control: RasterJobControl,
                           progress: ClosedRange<Double>) throws
        -> (document: RasterDocument, canvas: CGRect?, canvasCharge: UInt64)
    {
        var running = running
        let slots = Double(jobs.count)
        let width = progress.upperBound - progress.lowerBound
        for (position, job) in jobs.enumerated() {
            guard !control.isCancelled else { throw JobCancelled() }
            let scratch = charges[position]
            _ = try RasterResources.shared.admit(scratch: scratch)
            let input = running
            running = try control.withChildOperation(
                from: progress.lowerBound + width * Double(position) / slots,
                to: progress.lowerBound + width * Double(position + 1) / slots
            ) { child in
                guard child.setScratchLimit(scratch) else {
                    throw DoctoredCoreError(message: "Could not bound alignment work.")
                }
                return try input.editingProjectLayer(request: job.request, budget: budget,
                                                     control: child)
            }
        }
        guard let projected = AutoAlign.projectedCanvas(of: running, layers: layers) else {
            return (running, nil, 0)
        }
        let canvasWidth = Int(projected.width), canvasHeight = Int(projected.height)
        guard canvasWidth <= RasterImage.maxResizePixels / max(canvasHeight, 1) else {
            throw DoctoredCoreError(message: AutoAlign.tooBigMessage(width: canvasWidth,
                                                                   height: canvasHeight))
        }
        // Canvas Size moves offsets and copies no sample; at high depth its
        // plan's bookkeeping scratch rides on the held outputs.
        var step: UInt64 = 0
        if let precision = running.precision {
            let resize = try PrecisionPixelCommand.canvasSize(
                width: canvasWidth, height: canvasHeight, x: Int(-projected.minX),
                y: Int(-projected.minY))
            step = try PrecisionDocumentDescriptor.integer(
                precision.pixelEditPlan(request: resize.request, workingLimit: budget.limit,
                                        otherRetained: budget.other), "scratch_bytes")
        }
        let canvasCharge = try PrecisionDocumentDescriptor.sum(held, step)
        _ = try RasterResources.shared.admit(scratch: canvasCharge)
        running = try running.canvasResized(
            w: canvasWidth, h: canvasHeight, originX: Int(-projected.minX),
            originY: Int(-projected.minY)) ?? running
        return (running, projected, canvasCharge)
    }
}
