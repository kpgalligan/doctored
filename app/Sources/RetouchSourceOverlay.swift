import AppKit

/// A display-only hover cache. Pixel commits always recapture native source.
final class RetouchSourceOverlay {
    weak var source: RasterDocument?
    let layer: Int
    let choice: RetouchSamplingChoice
    let image: CGImage?

    init(source: RasterDocument, layer: Int, choice: RetouchSamplingChoice, image: CGImage?) {
        self.source = source; self.layer = layer; self.choice = choice; self.image = image
    }
}

extension EditorViewController {
    func retouchSourceOverlayImage() -> CGImage? {
        guard let document, let source = document.doc, !document.isLiveEditing,
              !BackgroundWork.occupied, let options = Self.retouchOptions(for: currentTool),
              options.sourceOverlay != false else { return nil }
        let layer = document.activeLayerIndex
        let choice = RetouchSamplingChoice(mode: options.sample, ignoreAdjustments: options.ignoreAdjustments)
        if let cached = retouchSourceOverlay, cached.source === source,
           cached.layer == layer, cached.choice == choice { return cached.image }
        let image: CGImage?
        do {
            let preview: RasterImage
            if let precision = source.precision {
                let plate = try precision.retouchSource(layer: layer, sample: choice.mode,
                    ignoreAdjustments: choice.ignoreAdjustments)
                let budget = try PrecisionWorkContext(image: plate)
                preview = try source.display(of: plate, context: budget)
            } else {
                guard let plate = try RetouchSource.plate(source, layer: layer, choice: choice) else { return nil }
                preview = plate.image
            }
            image = preview.makeCGImage(in: source.colorSpace)
        } catch { image = nil }
        retouchSourceOverlay = RetouchSourceOverlay(source: source, layer: layer, choice: choice, image: image)
        return image
    }
}
