import AppKit

extension RasterDocument {
    /// Presence is policy, so typed admission failures must not turn artwork
    /// into a plain raster. The facade already owns these bounded core fields.
    func hasArtworkDescription(_ index: Int) -> Bool {
        if isHighDepth {
            guard let rows = precisionInfo?["layers"] as? [[String: Any]], rows.indices.contains(index) else { return false }
            // A Generative Layer's tag describes where its pixels came from, not
            // artwork that re-renders.
            return (rows[index]["description_bytes"] as? NSNumber)?.intValue ?? 0 > 0
                && !GeneratedLayerTag.isTag(layerMeta(index))
        }
        switch LayerDescription.claimedKind(of: layerMeta(index)) {
        case .text, .shape, .fill: return true
        case nil: return false
        }
    }

    var artworkIndices: [Int] {
        (0..<layerCount).filter { hasArtworkDescription($0) }
    }

    func restoringArtworkDescription(_ description: String, layer: Int) throws -> RasterDocument {
        if layerMeta(layer) == description { return self }
        guard let restored = withLayerMeta(layer, description) else {
            throw NativeArtworkEvaluation.failure("The artwork description could not be preserved. No geometry was applied.")
        }
        return restored
    }

    func artworkAffine(_ index: Int, matrix: CGAffineTransform,
                       sampler: DrResizeFilter) throws -> RasterDocument? {
        if matrix.a == 1, matrix.b == 0, matrix.c == 0, matrix.d == 1,
           matrix.tx.isFinite, matrix.ty.isFinite,
           matrix.tx == matrix.tx.rounded(), matrix.ty == matrix.ty.rounded(),
           let info = layerInfo(index), let meta = layerMeta(index) {
            if isHighDepth {
                let raw = try performingPrecisionPixelEdit(.affine(layer: index, target: .pixels,
                    matrix: matrix, filter: .nearest))
                return try raw.map { try $0.restoringArtworkDescription(meta, layer: index) }
            }
            guard abs(matrix.tx) <= Double(Int32.max), abs(matrix.ty) <= Double(Int32.max) else {
                throw NativeArtworkEvaluation.failure("Artwork translation exceeds the supported coordinate range.")
            }
            return try withLayerOffset(index, info.offsetX + Int(matrix.tx), info.offsetY + Int(matrix.ty))
        }
        guard let description = layerDescription(index),
              description.isRenderable, let anchor = describedAnchor(index),
              let info = layerInfo(index) else {
            throw NativeArtworkEvaluation.failure("The artwork description or required font is unavailable. Its saved rendering remains intact.")
        }
        let map = description.transform.concatenating(matrix).snapped()
        guard map.isInvertible else { throw NativeArtworkEvaluation.failure("Artwork transform must be invertible.") }
        let movedAnchor = DescribedLayer.snappedAnchor(anchor.applying(matrix))
        let base: RasterDocument
        if layerHasMask(index) || layerHasStyle(index) {
            if isHighDepth {
                base = try performingPrecisionPixelEdit(.affine(layer: index, target: .pixels,
                    matrix: matrix, filter: Self.precisionFilter(sampler))) ?? self
            } else {
                guard let transformed = try transformingLayer(index, matrix, sampler: sampler) else {
                    throw NativeArtworkEvaluation.failure("Artwork mask/style transform was refused.")
                }
                base = transformed
            }
        } else { base = self }
        return try base.nativeArtwork(description.withTransform(map), anchor: movedAnchor,
            layer: index, replacing: true, name: info.name)
    }

    func artworkDocumentGeometry(_ operation: DocumentGeometry) throws -> RasterDocument? {
        let indices = artworkIndices
        let captures = try indices.map { index -> (Int, LayerDescription, CGPoint, String) in
            guard let description = layerDescription(index), description.isRenderable,
                  let anchor = describedAnchor(index), let info = layerInfo(index) else {
                throw NativeArtworkEvaluation.failure("A required artwork font or description is unavailable; geometry was not applied.")
            }
            return (index, description.resolved(rasterWidth: info.width), anchor, info.name)
        }
        let raw = try operation.performChecked(on: self)
        guard raw != nil || !captures.isEmpty else { return nil }
        var result = raw ?? self
        let map = operation.linearMap(canvasWidth: width, canvasHeight: height)
        for (index, description, anchor, name) in captures {
            let composed = description.withTransform(description.transform.concatenating(map).snapped(),
                turningTile: true)
            let moved = DescribedLayer.snappedAnchor(operation.apply(anchor, canvasWidth: width, canvasHeight: height))
            if let updated = try result.nativeArtwork(composed, anchor: moved, layer: index, replacing: true, name: name) { result = updated }
        }
        return try result.followingHostPlacement(of: captures.map(\.0), placedBy: raw ?? self)
    }

    /// Authored sRGB remains independent of destination storage/profile. Assign
    /// reinterprets ordinary raster codes; artwork regenerates authored colors.
    /// `options` is ignored when `convert` is false. The report is the
    /// conversion's own (empty for the default options and for Assign).
    func artworkProfile(_ profile: Data, convert: Bool, options: ConversionOptions = .default) throws
        -> (document: RasterDocument, report: ConversionReport)? {
        let captures = try artworkIndices.map { index -> (Int, LayerDescription, CGPoint, String) in
            guard let description = layerDescription(index), description.isRenderable,
                  let anchor = describedAnchor(index), let info = layerInfo(index) else {
                throw NativeArtworkEvaluation.failure("A required artwork font is unavailable; the profile edit was not applied.")
            }
            return (index, description, anchor, info.name)
        }
        let raw: RasterDocument?
        var report = ConversionReport()
        if isHighDepth {
            if convert, !options.isDefault {
                let converted = try convertingToProfile(profile, options: options)
                raw = converted?.document
                report = converted?.report ?? ConversionReport()
            } else {
                raw = try performingPrecisionAdjustment(convert ? .convertProfile(profile) : .assignProfile(profile))
            }
        } else if convert, !options.isDefault {
            let converted = try ProfileConversionWork.run(
                self, profile: profile, options: options,
                title: "Converting Captured Artwork Profile")
            raw = converted?.document
            report = converted?.report ?? ConversionReport()
        } else if convert {
            let scratch = try RetainedByteProfileWork.scratchEstimate(self)
            _ = try RasterResources.shared.admit(scratch: scratch)
            let task = try WorkerTask(scratchEstimate: scratch) { try self.convertingToProfile(profile) }
            raw = try BackgroundWork.wait(task, title: "Converting Captured Artwork Profile")
        } else { raw = try assigningProfile(profile) }
        guard var result = raw else { return nil }
        for (index, description, anchor, name) in captures {
            if let updated = try result.nativeArtwork(description, anchor: anchor, layer: index, replacing: true, name: name) { result = updated }
        }
        return (result, report)
    }
}

extension ImageDocument {
    /// Report of the edit that LANDED (doc handle changed); empty otherwise.
    @discardableResult
    func applyArtworkProfile(_ profile: Data, convert: Bool, options: ConversionOptions = .default,
                             record: [ActionStep]) -> ConversionReport {
        let before = doc
        var failure: Error?
        var report = ConversionReport()
        applyEdit(convert ? "Convert to Profile" : "Assign Profile", record: record) { source in
            do {
                guard let result = try source.artworkProfile(profile, convert: convert, options: options)
                else { return nil }
                report = result.report
                return result.document
            } catch { failure = error; return nil }
        }
        if let failure { presentError(failure) }
        return doc !== before ? report : ConversionReport()
    }
}
