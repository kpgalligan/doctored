import AppKit

// `RetouchPlate` is the canvas-sized plate the CORE builds from the document's own layers — no
// COMMITTED pixel comes from `canvas.image` any more, which is `ImageDocument.previewImage`, a
// display image that is an 8-bit reduction on a 16-bit document and deliberately the PREVIOUS
// revision's picture whenever a render failed. Native drag ghosts are derived from captured core
// plates after native interpolation. The plate is latched once per gesture (mouse-down for a
// stroke, region placement for the Patch tool) and used by BOTH the drag and the commit, so the two
// cannot sample different pixels. It is tagged per DESTINATION: `doc.drawingSpace` for a buffer
// drawn into an overlay, `doc.colorSpace` for one drawn on screen — the two differ only for a
// profile the app cannot draw into (a LUT/no-B2A profile, or a LUT the core cannot convert both
// ways; a LUT-mode profile, is its own drawing space), and there the wrong one silently
// converts the sampled texture.

/// ONE gesture's sampling plate: the canvas-sized straight-RGBA8 picture a
/// clone, heal, spot heal or patch reads from, built by the core from the
/// document's own layers.
///
/// # Why this type exists at all
///
/// Before Phase 9D the Clone Stamp and the Sample-All Healing Brush sampled
/// `canvas.image`, which is `ImageDocument.previewImage` — a DISPLAY image.
/// That is an 8-bit reduction on a 16-bit document, and on any revision whose
/// render failed or was deferred it is deliberately the PREVIOUS revision's
/// picture, kept so the canvas has something to draw. Sampling it meant a
/// stroke could clone pixels belonging to no document revision at all. The
/// phase's own rule — no display preview may become a sampling source — is
/// this type: every retouch gesture asks the core, and the core answers from
/// the layers.
///
/// # Latched once, used twice
///
/// A plate is built at mouse-down (or at the top of an agent call) and then
/// used by both the drag preview and the commit, so the two cannot sample
/// different pixels — the bug the Patch tool's own comments already name,
/// where the preview read the display image and the commit read the
/// projection. Nothing can swap it mid-gesture, for the same reason the
/// stroke's tool and offset are latched there.
final class RetouchPlate {
    let image: RasterImage
    let choice: RetouchSamplingChoice
    /// The plate the backdrop-matching half of a NEW-LAYER commit needs: the
    /// current-and-below picture the new layer will composite onto. Identical
    /// to `image` whenever the gesture already samples current-and-below, and
    /// nil when the gesture is not going to a new layer.
    let backdrop: RasterImage?

    init(image: RasterImage, choice: RetouchSamplingChoice, backdrop: RasterImage?) {
        self.image = image
        self.choice = choice
        self.backdrop = backdrop
    }

    var width: Int { image.width }
    var height: Int { image.height }

    /// The plate tagged for one destination. There is deliberately no
    /// default, for `makeCGImage`'s own reason: a sampled colour converts
    /// NOWHERE, so a plate drawn into a document-space CONTEXT must be tagged
    /// with that context's space (`doc.drawingSpace`) and a plate drawn on
    /// SCREEN with the pixels' own tag (`doc.colorSpace`). The two differ only
    /// for a profile the app cannot draw into (a LUT/no-B2A profile, or a LUT
    /// the core cannot convert both ways), which is exactly the document
    /// where getting it wrong silently converts the cloned texture.
    func cgImage(in space: CGColorSpace) -> CGImage? { image.makeCGImage(in: space) }
}

enum RetouchSource {
    /// The plate `choice` names for `layer`, or nil when the mode cannot be
    /// satisfied (a `.current` plate on an index with no raster pixels).
    /// Throws the core's message on a rendering or reservation refusal.
    ///
    /// `output` decides whether a backdrop plate is built beside it. A
    /// new-layer commit needs the current-and-below picture as the membrane's
    /// illumination reference whatever the gesture SAMPLES, so the two are
    /// the same buffer for a current-and-below gesture and two separate
    /// composites otherwise — the honest cost of asking to sample one thing
    /// and land on another.
    static func plate(
        _ doc: RasterDocument, layer: Int, choice: RetouchSamplingChoice,
        output: RetouchOutput = .layer
    ) throws -> RetouchPlate? {
        guard let image = try doc.retouchSource(
            layer, sample: choice.mode, ignoreAdjustments: choice.ignoreAdjustments)
        else { return nil }
        guard output == .newLayer else {
            return RetouchPlate(image: image, choice: choice, backdrop: nil)
        }
        if choice.mode == .currentAndBelow {
            return RetouchPlate(image: image, choice: choice, backdrop: image)
        }
        guard let backdrop = try doc.retouchSource(
            layer, sample: .currentAndBelow, ignoreAdjustments: choice.ignoreAdjustments)
        else { return nil }
        return RetouchPlate(image: image, choice: choice, backdrop: backdrop)
    }

    /// The message for a plate that could not be built at all, phrased for
    /// the gesture rather than for the renderer.
    static func missingPlate(_ mode: RetouchSampling) -> String {
        mode == .current
            ? "This layer has no pixels to sample from. Sample Current & Below or All Layers to "
                + "read the picture beneath it."
            : "Could not build the picture to sample from."
    }
}
