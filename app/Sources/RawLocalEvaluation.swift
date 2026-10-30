import AppKit

// Ordered encoded-RGB float blending precedes calibrated vignette and one final u16 quantization,
// then existing affine/fallback/retouch.

/// Worker-only immutable evaluation. Camera stages run sequentially through the
/// existing job queue; Core Image is noncancellable within one camera rendering.
/// All mask/native stages use fresh children of the same cancellation owner.
enum RawLocalEvaluation {
    static func cameraDescriptor(_ source: RawSourceDescriptor) -> RawSourceDescriptor {
        var result = source
        result.recipe.enhancements = nil
        result.recipe.sourceGeometry = nil
        result.recipe.decoderFrame = nil
        result.recipe.vignetteReference = nil
        // The camera-only render never carries the Lensfun stage (v1-lensfun), and the toggle
        // that switches it is not a camera setting.
        if result.recipe.lensProfile != nil {
            result.recipe.lensProfile = nil
            result.recipe.values.lensCorrection = result.recipe.capabilities.defaults.lensCorrection
        }
        result.recipe.version = result.recipe.cameraOnlyVersion
        return result
    }

    static func develop(snapshot: RawSourceSnapshot, requested: RawSourceDescriptor,
                        previous: RawSourceDescriptor, previousSource: RasterDocument?, layer: Int,
                        masks: [RasterRawLocalMask], cache: RawLocalVariantCache,
                        maximumScratch: UInt64, control: RasterJobControl,
                        migrating: Bool = false) throws -> (RasterDocument, RawSourceDescriptor) {
        let stages = try developStages(snapshot: snapshot, requested: requested,
            previous: previous, previousSource: previousSource, layer: layer, masks: masks,
            cache: cache, maximumScratch: maximumScratch, control: control, migrating: migrating)
        return (stages.final, stages.descriptor)
    }

    /// The key a base development is cached and memoized under: `cameraDescriptor(requested).json()`
    /// plus `lensfunKeySuffix(requested.recipe)`.
    static func baseKey(_ requested: RawSourceDescriptor) throws -> String {
        try cameraDescriptor(requested).json() + lensfunKeySuffix(requested.recipe)
    }

    /// ";lensfun=" + `recipe.lensProfile!.profile` when `recipe.lensfunApplies`, else "".
    private static func lensfunKeySuffix(_ recipe: RawDevelopmentRecipe) -> String {
        guard recipe.lensfunApplies, let profile = recipe.lensProfile else { return "" }
        return ";lensfun=" + profile.profile
    }

    /// `developed` through `recipe.lensProfile` (`applyingRawLensfun`, under
    /// `control.withChildOperation`) when `recipe.lensfunApplies`; `developed` itself otherwise.
    private static func lensfunCorrected(_ developed: RasterDocument, recipe: RawDevelopmentRecipe,
                                         control: RasterJobControl) throws -> RasterDocument {
        guard recipe.lensfunApplies else { return developed }
        let profile = recipe.lensProfile
        return try control.withChildOperation { child in
            try developed.applyingRawLensfun(profile, control: child)
        }
    }

    /// The base development of `requested`: the camera-only global render before any local,
    /// vignette, optics, Presence and grain (`cameraDescriptor`), from the cache, from the previous source
    /// when that is already exactly it, or decoded, and then Lensfun-corrected when the recipe's
    /// profile applies (v1-lensfun), as a decoder lens profile is inside the render. It is what the
    /// locals are mixed over, what a range reads and what a generated mask (`RawLocalGenerated`)
    /// is made from, so all of them are in the corrected frame. The descriptor returned is the
    /// camera-only one.
    static func baseDevelopment(snapshot: RawSourceSnapshot, requested: RawSourceDescriptor,
                                previous: RawSourceDescriptor, previousSource: RasterDocument?,
                                layer: Int, cache: RawLocalVariantCache, maximumScratch: UInt64,
                                control: RasterJobControl, migrating: Bool = false) throws
        -> (RasterDocument, RawSourceDescriptor) {
        func check() throws {
            guard !control.isCancelled else { throw JobCancelled() }
        }
        try check()
        let cacheTicket = cache.ticket()
        let baseRequest = cameraDescriptor(requested)
        let previousCamera = cameraDescriptor(previous)
        let key = try baseKey(requested)
        if let cached = cache.value(for: key) { return cached }
        let camera: (RasterDocument, RawSourceDescriptor)
        if let previousSource, baseRequest == previousCamera,
           previous.recipe.sourceGeometry == nil,
           previous.recipe.enhancements?.locals.isEmpty != false,
           (previous.recipe.enhancements?.vignette.edgeExposure ?? 0) == 0,
           previous.recipe.enhancements?.grain == nil,
           previous.recipe.enhancements?.presence == nil,
           !previous.recipe.lensfunApplies {
            camera = (try previousSource.rawLocalBaseSource(layer: layer), baseRequest)
        } else {
            let wbChanged = baseRequest.recipe.values.customWhiteBalance
                && (migrating || baseRequest.recipe.values.temperature != previous.recipe.values.temperature
                    || baseRequest.recipe.values.tint != previous.recipe.values.tint)
            try check()
            camera = try RetainedRawDecoder.develop(url: snapshot.url, descriptor: baseRequest,
                maximumScratch: maximumScratch, whiteBalanceChanged: wbChanged, cameraOnly: true)
        }
        try check()
        let base = (try lensfunCorrected(camera.0, recipe: requested.recipe, control: control), camera.1)
        try check()
        try cache.store(base.0, descriptor: base.1, key: key, ticket: cacheTicket)
        return base
    }

    /// `supplied` is this request's base development when the caller already made it
    /// (`baseDevelopment`, for a generated mask); it is developed here otherwise. The base and
    /// each decoded camera variant are Lensfun-corrected before the locals mix (v1-lensfun), so
    /// local coverage and range reads are in the corrected frame.
    static func developStages(snapshot: RawSourceSnapshot, requested: RawSourceDescriptor,
                        previous: RawSourceDescriptor, previousSource: RasterDocument?, layer: Int,
                        masks: [RasterRawLocalMask], cache: RawLocalVariantCache,
                        maximumScratch: UInt64, control: RasterJobControl,
                        migrating: Bool = false,
                        base supplied: (RasterDocument, RawSourceDescriptor)? = nil) throws ->
                        (final: RasterDocument, editing: RasterDocument, base: RasterDocument,
                         descriptor: RawSourceDescriptor) {
        func check() throws {
            guard !control.isCancelled else { throw JobCancelled() }
        }
        try check()
        let cacheTicket = cache.ticket()
        let base = try supplied ?? baseDevelopment(snapshot: snapshot, requested: requested,
            previous: previous, previousSource: previousSource, layer: layer, cache: cache,
            maximumScratch: maximumScratch, control: control, migrating: migrating)
        try check()
        var effective = requested
        effective.recipe.values = base.1.recipe.values
        // The camera-only base carries no Lensfun toggle (`cameraDescriptor`); keep the request's.
        if requested.recipe.lensProfile != nil, !requested.recipe.capabilities.lensCorrection {
            effective.recipe.values.lensCorrection = requested.recipe.values.lensCorrection
        }
        effective.recipe.chromaticityX = base.1.recipe.chromaticityX
        effective.recipe.chromaticityY = base.1.recipe.chromaticityY
        var nodes = requested.recipe.enhancements?.locals ?? []
        guard masks.count == nodes.count else { throw DoctoredCoreError(message: "RAW local masks and recipes have different counts.") }
        // A range colour named by a source point becomes a Lab colour here, read from the base
        // development the range itself is evaluated on; the saved recipe holds only the colour.
        for index in nodes.indices {
            guard let point = nodes[index].range?.color?.sample else { continue }
            try check()
            guard point.count == 2 else { throw DoctoredCoreError(message: RawLocalRange.refusal) }
            nodes[index].range?.color?.lab = try base.0.rawRangeSample(x: point[0], y: point[1])
            nodes[index].range?.color?.sample = nil
        }
        // In a version-3 recipe only a camera local (white balance or camera exposure) mixes its
        // own variant; every other local is evaluated by the core on the development itself, and
        // a local with no change at all is skipped. Below version 3 every local passes its
        // variant, which is the base for a local with no camera change, as it always did.
        let version = RawEnhancementRecipe.version(for: nodes)
        guard nodes.filter({ $0.developsCameraVariant(version: version) }).count
                <= RawEnhancementRecipe.maximumCameraLocals else {
            throw DoctoredCoreError(message: RawEnhancementRecipe.cameraLocalRefusal)
        }
        var variants: [RasterDocument?] = []
        for index in nodes.indices {
            try check()
            let node = nodes[index]
            var variant = base.1
            variant.recipe.values.exposure = Double(Float(base.1.recipe.values.exposure + node.exposureOffset))
            var wbChanged = false
            switch node.whiteBalance {
            case .inherit: break
            case .asShot:
                variant.recipe.values.customWhiteBalance = false
                variant.recipe.values.temperature = variant.recipe.capabilities.defaults.temperature
                variant.recipe.values.tint = variant.recipe.capabilities.defaults.tint
                variant.recipe.chromaticityX = variant.recipe.asShotChromaticityX
                variant.recipe.chromaticityY = variant.recipe.asShotChromaticityY
            case .custom:
                guard let temperature = node.temperature, let tint = node.tint else {
                    throw DoctoredCoreError(message: "Custom local white balance is incomplete.")
                }
                variant.recipe.values.customWhiteBalance = true
                if !migrating, let old = previous.recipe.enhancements?.locals.first(where: { $0.id == node.id }),
                   old.whiteBalance == .custom, old.temperature == temperature, old.tint == tint,
                   let saved = old.effective {
                    variant.recipe.values.temperature = saved.values.temperature
                    variant.recipe.values.tint = saved.values.tint
                    variant.recipe.chromaticityX = saved.chromaticityX
                    variant.recipe.chromaticityY = saved.chromaticityY
                } else {
                    variant.recipe.values.temperature = temperature
                    variant.recipe.values.tint = tint
                    wbChanged = true
                }
            }
            try variant.recipe.validate()
            let key = try variant.json() + (wbChanged ? ";resolve-local-white-balance" : "")
                + lensfunKeySuffix(requested.recipe)
            let decoded: (RasterDocument, RawSourceDescriptor)
            if variant == base.1 && !wbChanged {
                decoded = base
            } else if !node.enabled || node.density == 0 {
                try check()
                let resolved = try RetainedRawDecoder.canonicalDescriptor(url: snapshot.url,
                    descriptor: variant, whiteBalanceChanged: wbChanged)
                decoded = (base.0, resolved)
            } else if let cached = cache.value(for: key) {
                decoded = cached
            } else {
                try check()
                let camera = try RetainedRawDecoder.develop(url: snapshot.url, descriptor: variant,
                    maximumScratch: maximumScratch, whiteBalanceChanged: wbChanged, cameraOnly: true)
                try check()
                // Each variant is corrected as the base is, so the locals mix in one frame.
                decoded = (try lensfunCorrected(camera.0, recipe: requested.recipe, control: control),
                           camera.1)
                try check()
                try cache.store(decoded.0, descriptor: decoded.1, key: key, ticket: cacheTicket)
            }
            try check()
            guard decoded.0.width == base.0.width, decoded.0.height == base.0.height,
                  decoded.0.iccProfile == base.0.iccProfile else {
                throw DoctoredCoreError(message: "A local RAW variant changed geometry or colour encoding. The saved rendering remains intact.")
            }
            nodes[index].effective = RawLocalEffective(values: decoded.1.recipe.values,
                chromaticityX: decoded.1.recipe.chromaticityX, chromaticityY: decoded.1.recipe.chromaticityY)
            variants.append(node.developsCameraVariant(version: version) ? decoded.0 : nil)
        }
        effective.recipe.enhancements?.locals = nodes
        try effective.recipe.validate()
        try effective.recipe.validateLocalEffectiveState()
        let controls = try nodes.map { node -> [String: Any] in
            var fields: [String: Any] = ["enabled": node.enabled, "density": node.density, "feather": node.feather]
            if let range = node.range { fields["range"] = try range.coreJSON() }
            if let tone = node.tone { fields["tone"] = tone.json }
            return fields
        }
        let vignette = effective.recipe.enhancements?.vignette ?? .neutral
        let mixed = try control.withChildOperation { child in
            try base.0.developLocals(variants: variants, masks: masks,
                settings: RawLocalJSON.string(controls),
                vignette: vignette.coreJSON(width: base.0.width, height: base.0.height,
                    reference: effective.recipe.vignetteReference), control: child)
        }
        try check()
        var final = mixed
        if let geometry = effective.recipe.sourceGeometry {
            let corrected = final
            final = try control.withChildOperation { child in
                try corrected.applyingRawSourceGeometry(geometry.map().json(), control: child)
            }
            try check()
        }
        if let presence = effective.recipe.enhancements?.presence {
            let mapped = final
            final = try control.withChildOperation { child in
                try mapped.applyingRawPresence(presence, control: child)
            }
            try check()
        }
        if let grain = effective.recipe.enhancements?.grain {
            let mapped = final
            final = try control.withChildOperation { child in
                try mapped.applyingRawGrain(grain, contentSHA256: effective.content_sha256, control: child)
            }
            try check()
        }
        return (final, mixed, base.0, effective)
    }

    static func replacing(source: RasterDocument, layer: Int, asset: RasterDocument,
                          descriptor: RawSourceDescriptor, masks: [RasterRawLocalMask],
                          control: RasterJobControl,
                          sourceCoordinateMap: [Double]? = nil) throws -> RasterDocument {
        // A linked RAW layer stays linked, as in `RawSourceWork.replacing`.
        let link = try source.retainedSourceInfo(layer)?["link"] as? [String: Any]
        var fields: [String: Any] = ["op": "replace", "layer": layer,
            "revision_id": UUID().uuidString, "depth_conversion": "convert_fallback",
            "link": link.map { $0 as Any } ?? NSNull()]
        if let sourceCoordinateMap { fields["source_coordinate_map"] = sourceCoordinateMap }
        let command = try PrecisionPixelCommand(fields)
        let replaced = try control.withChildOperation { child -> RasterDocument in
            guard let result = try source.editingRetainedSource(asset: asset, request: command.request, control: child) else {
                throw DoctoredCoreError(message: "The local RAW source could not be replaced.")
            }
            return result
        }
        let retained = try control.withChildOperation { child in
            try replaced.attachingRawSource(layer, descriptor: descriptor.json(), original: source,
                originalLayer: layer, control: child)
        }
        let attachment = try RawLocalWork.attachment(nodes: descriptor.recipe.enhancements?.locals ?? [],
            masks: masks, width: asset.width, height: asset.height, control: control)
        return try control.withChildOperation { child in
            try retained.attachingRawLocalMasks(layer: layer, descriptor: attachment, masks: masks, control: child)
        }
    }
}
