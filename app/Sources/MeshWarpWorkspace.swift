import AppKit

// Edit ▸ Puppet Warp's and Edit ▸ Perspective Warp's workspace: what both on-canvas
// sessions capture when they open. The layer's pixels as an 8-bit PROXY (at most 2,048 px on
// its longest side, Liquify's rule), the stack below and above it, with a selection its canvas
// u16 coverage and that coverage at the proxy's size (the float form's hole), and the memory
// lease the preview holds. The sessions are `PuppetWarpSession` and `PerspectiveWarpSession`.

struct MeshWarpWorkspace {
    static let proxyMaxSide = 2048

    /// The document the session opened on, and the one it reads and commits
    /// from: the same handle, or the layer rasterized
    /// (`withLayerMeta(layer, nil)`) when it carried a description.
    let base: RasterDocument
    let source: RasterDocument
    let layer: Int
    /// The layer's canvas rect and opacity.
    let layerRect: CGRect
    let opacity: CGFloat
    let colorSpace: CGColorSpace
    /// The layer's pixels at 8 bits, reduced to `proxyMaxSide`.
    let proxy: RasterImage
    let proxyImage: CGImage?
    /// The stack below and above the layer (`transformStackComposites`).
    let below: CGImage?
    let above: CGImage?
    /// The float form: the selection's canvas u16 coverage, its rect, its
    /// coverage at the proxy's size and that as a gray image (the hole).
    let coverage: [UInt16]?
    let selectionBounds: CGRect?
    let proxyCoverage: [UInt8]?
    let coverageMask: CGImage?
    let selectionHistory: SelectionHistoryState
    /// The core's own working memory the session's previews need, which the
    /// lease covers on top of the buffers (`workspaceBytes`).
    let coreBytes: UInt64
    /// The preview's memory lease; dropped while the commit runs.
    var reservation: SelectionWorkBudget.Reservation?

    var isFloat: Bool { coverage != nil }

    /// Opens on layer `layer` of `source`; `base` is the document's handle
    /// and `plates` its stack around the layer; `name` the feature the
    /// refusals name. Throws when the layer has no pixels, the selection
    /// cannot be mapped onto it, or the workspace does not fit.
    init(document: ImageDocument, base: RasterDocument, source: RasterDocument, layer: Int,
         plates: (below: CGImage?, above: CGImage?), name: String, coreBytes: UInt64) throws {
        guard let placement = source.layerPlacement(layer), placement.width > 0,
              placement.height > 0, let info = source.layerInfo(layer) else {
            throw DoctoredCoreError(message: "\(name) needs a layer with pixels.")
        }
        let longest = max(placement.width, placement.height)
        let image = !source.isHighDepth && longest <= Self.proxyMaxSide
            ? source.layerImage(layer)
            : source.layerThumbnail(layer, maxSide: min(Self.proxyMaxSide, longest))
        guard let image, image.width > 0, image.height > 0 else {
            throw DoctoredCoreError(message: "\(name) could not read the layer's pixels.")
        }
        var coverage: [UInt16]?
        var proxyCoverage: [UInt8]?
        let selection = document.canvasSelection
        if let selection {
            coverage = selection.maskSamples16()
            _ = try RasterResources.shared.admit(
                scratch: UInt64(placement.width) * UInt64(placement.height))
            guard let plane = placement.layerPlane(from: selection.maskBytes()) else {
                throw DoctoredCoreError(message: "\(name) could not map the selection onto the layer.")
            }
            proxyCoverage = image.width == placement.width && image.height == placement.height
                ? plane
                : try RasterImage.resizingSelection(
                    plane, width: placement.width, height: placement.height,
                    toWidth: image.width, toHeight: image.height)
        }
        reservation = try SelectionWorkBudget.reservePreview(
            bytes: Self.workspaceBytes(canvas: (base.width, base.height),
                                       proxy: (image.width, image.height), core: coreBytes),
            outputBytes: 0)
        self.coreBytes = coreBytes
        self.base = base
        self.source = source
        self.layer = layer
        layerRect = CGRect(x: placement.x, y: placement.y, width: placement.width,
                           height: placement.height)
        opacity = CGFloat(info.opacity)
        colorSpace = base.colorSpace
        proxy = image
        proxyImage = image.makeCGImage(in: base.colorSpace)
        below = plates.below
        above = plates.above
        self.coverage = coverage
        selectionBounds = selection?.bounds
        self.proxyCoverage = proxyCoverage
        coverageMask = proxyCoverage.flatMap {
            Self.grayImage($0, width: image.width, height: image.height)
        }
        selectionHistory = document.selectionHistory
    }

    /// What a session holds at once, as an arithmetic bound over buffers of
    /// known size: six proxy-sized RGBA8 images (the proxy, its CGImage, the
    /// preview, the one being rendered and its CGImage, the coverage and its
    /// mask in the margin), the two canvas plates at 4 bytes a pixel and the
    /// core's `core` bytes, with a quarter over and 1 MiB.
    static func workspaceBytes(canvas: (Int, Int), proxy: (Int, Int), core: UInt64) -> UInt64 {
        let buffers = 6 * UInt64(proxy.0) * UInt64(proxy.1) * 4
            + 2 * UInt64(canvas.0) * UInt64(canvas.1) * 4 + core
        return buffers + buffers / 4 + (1 << 20)
    }

    /// The lease back after a refused or cancelled commit.
    mutating func renewReservation() {
        reservation = try? SelectionWorkBudget.reservePreview(
            bytes: Self.workspaceBytes(canvas: (base.width, base.height),
                                       proxy: (proxy.width, proxy.height), core: coreBytes),
            outputBytes: 0)
    }

    /// The coverage as an 8-bit gray image (white = selected) for the hole.
    private static func grayImage(_ bytes: [UInt8], width: Int, height: Int) -> CGImage? {
        guard bytes.count == width * height,
              let provider = CGDataProvider(data: Data(bytes) as CFData) else { return nil }
        return CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 8,
                       bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(),
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: true,
                       intent: .defaultIntent)
    }
}
