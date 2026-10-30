import AppKit

/// Main-thread capture of every editable artwork member a set transform
/// affects. Workers receive only immutable drawing commands and descriptions.
/// In a CMYK document (#547) the members are captured in ink and published
/// through `dr_precision_doc_cmyk_artwork` (CmykArtwork.swift's arm).
struct ArtworkGeometryWork {
    struct Member {
        let index: Int
        let name: String
        let description: String
        let evaluation: NativeArtworkEvaluation
    }

    let members: [Member]
    let profile: Data
    let perspectiveLoss: Bool

    init(source: RasterDocument, request: GeometrySetRequest,
         rasterizeArtwork: Bool) throws {
        assert(Thread.isMainThread)
        let affected = source.movingSet(request.layers).filter(source.hasArtworkDescription)
        guard !affected.isEmpty else {
            members = []; profile = Data(); perspectiveLoss = false
            return
        }
        guard let profile = source.isCmyk ? source.precision?.cmykProfileData : source.iccProfile else {
            throw DoctoredCoreError(message: "Artwork geometry needs the document ICC profile.")
        }
        let mapping: CGAffineTransform?
        let loss: Bool
        switch request {
        case .affine(_, let matrix, _):
            mapping = matrix; loss = false
        case .perspective(_, let rect, let quad, _):
            mapping = LayerTransform.parallelogramAffine(of: rect, onto: quad)
            loss = mapping == nil
        }
        if loss && !rasterizeArtwork {
            throw DoctoredCoreError(message:
                "True perspective will rasterize editable artwork. Confirm rasterization first.")
        }
        self.profile = profile
        perspectiveLoss = loss
        // A set transform leaves the canvas as it is.
        members = try Self.capture(source: source, indices: affected, mapping: mapping,
                                   canvas: (source.width, source.height))
    }

    /// Capture a whole-document affine map before a crop/rotation worker starts.
    /// The core applies this same old-canvas to new-canvas map to pixels and masks.
    /// `canvas` is the RESULT's canvas size: a fill layer is drawn to cover it
    /// (FillLayer.swift).
    init(source: RasterDocument, affineMap: CGAffineTransform,
         canvas: (width: Int, height: Int)) throws {
        assert(Thread.isMainThread)
        let affected = source.artworkIndices
        guard !affected.isEmpty else {
            members = []; profile = Data(); perspectiveLoss = false
            return
        }
        guard let profile = source.isCmyk ? source.precision?.cmykProfileData : source.iccProfile else {
            throw DoctoredCoreError(message: "Artwork geometry needs the document ICC profile.")
        }
        self.profile = profile
        perspectiveLoss = false
        members = try Self.capture(source: source, indices: affected, mapping: affineMap,
                                   canvas: canvas)
    }

    /// The FILL layers alone, drawn to cover `canvas`: what a typed
    /// crop with no straighten re-renders. Its window move keeps every text
    /// and shape raster as it is, but a fill layer must cover the new canvas.
    init(fillsOf source: RasterDocument, canvas: (width: Int, height: Int)) throws {
        assert(Thread.isMainThread)
        let affected = source.fillIndices
        guard !affected.isEmpty else {
            members = []; profile = Data(); perspectiveLoss = false
            return
        }
        guard let profile = source.isCmyk ? source.precision?.cmykProfileData : source.iccProfile else {
            throw DoctoredCoreError(message: "Artwork geometry needs the document ICC profile.")
        }
        self.profile = profile
        perspectiveLoss = false
        members = try Self.capture(source: source, indices: affected,
                                   mapping: .identity, canvas: canvas)
    }

    /// No members: geometry that regenerates no artwork. Perspective Crop
    /// maps the canvas through a homography, which no text or shape
    /// description can carry, so its described layers are rasterized instead.
    init(noArtworkFor source: RasterDocument) {
        members = []; profile = Data(); perspectiveLoss = false
    }

    /// One member rendered and published over its own record: the RGB render
    /// and `publishingArtwork`, or in a CMYK document the ink components and
    /// `publishingCmykArtwork`. nil when the record already holds it.
    private func publish(_ member: Member, into current: RasterDocument,
                         control: RasterJobControl) throws -> RasterDocument? {
        let evaluation = member.evaluation
        if evaluation.cmyk {
            guard let precision = current.precision else {
                throw DoctoredCoreError(message: "Ink artwork needs a CMYK document.")
            }
            let parts = try evaluation.components(control: control)
            return try control.withChildOperation { child in
                try precision.publishingCmykArtwork(
                    layer: member.index, replacing: true, position: .above,
                    width: evaluation.width, height: evaluation.height, components: parts.json,
                    samples: parts.samples, intrinsic: parts.intrinsic, x: evaluation.offsetX,
                    y: evaluation.offsetY, name: member.name, description: member.description,
                    control: child).map { try RasterDocument(precision: $0) }
            }
        }
        let image = try evaluation.compute(profile: profile, control: control)
        return try control.withChildOperation { child in
            try current.publishingArtwork(image, layer: member.index,
                replacing: true, x: evaluation.offsetX,
                y: evaluation.offsetY, name: member.name,
                description: member.description, control: child)
        }
    }

    private static func capture(source: RasterDocument, indices: [Int],
                                mapping: CGAffineTransform?,
                                canvas: (width: Int, height: Int)) throws -> [Member] {
        try indices.map { index in
            guard let description = source.layerDescription(index),
                  description.isRenderable,
                  let anchor = source.describedAnchor(index),
                  let info = source.layerInfo(index) else {
                throw DoctoredCoreError(message:
                    "The artwork description or required font is unavailable. Geometry was not applied.")
            }
            let renderDescription: LayerDescription
            let renderAnchor: CGPoint
            if let mapping {
                let transformed = description.transform.concatenating(mapping).snapped()
                guard transformed.isInvertible else {
                    throw DoctoredCoreError(message: "Artwork transform must be invertible.")
                }
                renderDescription = description.withTransform(transformed)
                renderAnchor = DescribedLayer.snappedAnchor(anchor.applying(mapping))
            } else {
                renderDescription = description
                renderAnchor = anchor
            }
            // A fill covers the result's canvas, whatever the map did to it.
            let prepared = renderDescription.fitted(toCanvas: canvas).withOriginFraction(
                DescribedLayer.anchorFraction(renderAnchor))
            // A CMYK document's member is drawn in its stored ink (an sRGB
            // description separated first, `cmykArtwork`).
            let inked = source.isCmyk ? try source.cmykArtwork(prepared, matching: []) : prepared
            guard let json = inked.json(), json.utf8.count <= NativeArtwork.maximumDescriptionBytes else {
                throw DoctoredCoreError(message: "Artwork description exceeds its bounded geometry form.")
            }
            let drawing = try NativeArtwork.drawing(inked, anchor: renderAnchor,
                                                    ink: source.artworkInk,
                                                    reference: source.layerReference(index),
                                                    alignBox: source.fillAlignBox(index))
            return Member(index: index, name: info.name, description: json,
                          evaluation: try NativeArtworkEvaluation(drawing, float: source.isFloat,
                                                                  cmyk: source.isCmyk))
        }
    }

    /// The members render one after another (`prepareBefore` /
    /// `prepareAfter`), each coverage call reserving the core's bookkeeping
    /// and releasing it on return, so the stage needs that once and each
    /// member's own planes beside it. Summing every member's whole
    /// `scratchBytes` charged the bookkeeping once per text or shape layer.
    var scratchBytes: UInt64 {
        guard !members.isEmpty else { return 0 }
        return members.reduce(NativeArtworkEvaluation.coverageBookkeepingBytes) {
            // A CMYK member adds its render's margin (`cmykNativeArtwork`'s plan).
            let ink = $1.evaluation.cmyk ? NativeArtworkEvaluation.cmykMarginPerPixel
                &* UInt64($1.evaluation.width * $1.evaluation.height) &+ UInt64(profile.count) &* 64 : 0
            return $0 &+ $1.evaluation.ownScratchBytes &+ ink
        }
    }

    /// True perspective begins from the original description at native depth,
    /// then the shared core homography samples that canonical raster once.
    func prepareBefore(_ source: RasterDocument,
                       control: RasterJobControl) throws -> RasterDocument {
        guard perspectiveLoss else { return source }
        var current = source
        for member in members {
            let published = try publish(member, into: current, control: control)
            // The canonical original may already be the stored fallback; an
            // unchanged publication is a successful no-op, not a refusal.
            if published == nil {
                guard current.layerMeta(member.index) == member.description,
                      let info = current.layerInfo(member.index),
                      info.offsetX == member.evaluation.offsetX,
                      info.offsetY == member.evaluation.offsetY else {
                    throw DoctoredCoreError(message:
                        "Artwork could not be prepared for perspective.")
                }
            }
            let next = published ?? current
            guard let bare = next.withLayerMeta(member.index, nil) else {
                throw DoctoredCoreError(message: "Artwork could not be prepared for perspective.")
            }
            current = bare
        }
        return current
    }

    /// `member` redrawn at `current`'s reference point for it when it is a
    /// pattern fill: the core mapped the layer's `fxrp` with the
    /// geometry, and a linked tile's origin follows the mapped point. An
    /// aligned gradient fill is redrawn over `current`'s align box:
    /// the core mapped the mask with the geometry. `FillLayer.drawing` is
    /// pure, so this needs no main-thread hop. Every other member as captured.
    private static func followingReference(_ member: Member, in current: RasterDocument) throws -> Member {
        guard let payload = FillLayerPayload.decode(member.description),
              payload.fill == FillLayerPayload.patternKind
                || payload.gradient?.alignWithLayer == true else { return member }
        let drawing = try FillLayer.drawing(payload, ink: current.artworkInk,
                                            reference: current.layerReference(member.index),
                                            alignBox: current.fillAlignBox(member.index))
        return Member(index: member.index, name: member.name, description: member.description,
                      evaluation: try NativeArtworkEvaluation(
                          drawing, float: member.evaluation.float, cmyk: member.evaluation.cmyk))
    }

    /// Affine geometry replaces each resampled fallback with a fresh native
    /// drawing and its composed editable description before publication.
    func prepareAfter(_ result: RasterDocument,
                      control: RasterJobControl) throws -> RasterDocument {
        guard !perspectiveLoss else { return result }
        var current = result
        for member in members {
            let published = try publish(
                Self.followingReference(member, in: current), into: current, control: control)
            current = published ?? current
            guard current.layerMeta(member.index) == member.description else {
                throw DoctoredCoreError(message: "Artwork could not be regenerated after geometry.")
            }
        }
        return current
    }
}
