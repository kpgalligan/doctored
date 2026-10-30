import AppKit

// The colour sampler list is CORE document state at every depth (`RasterDocument.colorSamplers` /
// `withColorSamplers`, saved in `.rz` and PSD 1073, moved by every geometry op), and
// `ImageDocument.colorSamplers` (`ColorSamplerSet`, at most 10) is its LIVE mirror, resynced by the
// `doc` didSet (`syncColorSamplersFromDocument`) and different only during a marker drag. Every
// change — add, ⌥-click remove, Clear, a drag's release (`onColorSamplerMoveEnd`) and MCP
// `set_color_samplers` — goes through `commitColorSamplers`, `applyGroupExpanded`'s
// untracked-mutation shape (dirty, no undo step, no Action step, projection re-pointed,
// `history.replaceUntrackedCurrentState()`), refused with `SilentEditRefusal` during a live
// gesture, a publication or a job on the document (`DocumentJobs.refusesGesture`). `EditingState`
// holds no list: `applyEdit` still marks a canvas-geometry transition
// `DocumentHistory.Entry.changesGeometry` (ORed on a merge), whose
// undo/redo restores the destination's own list; any other transition carries the current handle's
// list onto its destination (`DocumentHistory.replaceDocument(at:with:)` /
// `EditingState.replacingDocument`, the destination handle kept when the lists match), while a
// snapshot restore brings the list the snapshot was taken with and is itself marked
// `changesGeometry`, so undoing it brings the pre-restore list back. `ColorSamplerReader` reads the
// composite once per call at the document's depth
// (`PixelReadout`/`PrecisionReadout`/`FloatReadout.at(_:projection:…)`); while a tone sheet is
// attached the canvas's `previewImage.didSet` drives "after" readings — 8 bits from the preview
// image, 16 bits from `previewPrecisionSource` (the projection `PrecisionAdjustmentSession` hands
// the canvas beside its preview), float none.

/// The Color Sampler: up to ten points pinned per document and read live in
/// the Info panel's Color Samplers section, on the canvas and over MCP
/// (`get_color_samplers` / `set_color_samplers`).
///
/// The list is DOCUMENT state: the core document holds it at every
/// depth (`RasterDocument.colorSamplers`), saves it in `.rz` and as PSD image
/// resource 1073, and moves it with every canvas-geometry op — a sampler that
/// lands off the canvas is dropped.
/// `ImageDocument.colorSamplers` is the live mirror the canvas, the Info
/// panel and MCP read; it differs from the handle only during a marker drag.
/// A change commits through `ImageDocument.commitColorSamplers` as an
/// untracked mutation: it dirties the document but adds no undo step and no
/// Action step. Ordinary undo and redo keep the current list (it is carried
/// onto the destination entry); a canvas-geometry transition restores the
/// destination's own list (`DocumentHistory.Entry.changesGeometry`).
/// Samplers read the COMPOSITE only, at the document's own depth.

/// One pinned point, in canvas pixels; `sampleSize` is 1, 3 or 5.
struct ColorSampler: Equatable {
    var x: Int
    var y: Int
    var sampleSize: Int
}

/// The ordered list, at most `limit` long. Every mutation is a pure value
/// that answers nil when it cannot apply, so a caller can beep.
struct ColorSamplerSet: Equatable {
    /// The core's `doc_sampler::MAX_COLOR_SAMPLERS`, which refuses an
    /// eleventh sampler in `dr_doc_with_color_samplers`.
    static let limit = 10
    private(set) var items: [ColorSampler] = []

    /// The caller validates the count (`set_color_samplers` refuses an
    /// eleventh sampler by name).
    init(items: [ColorSampler] = []) {
        self.items = items
    }

    /// nil at the limit.
    func adding(_ sampler: ColorSampler) -> ColorSamplerSet? {
        guard items.count < Self.limit else { return nil }
        return ColorSamplerSet(items: items + [sampler])
    }

    /// nil on a bad index.
    func moving(_ index: Int, toX x: Int, y: Int) -> ColorSamplerSet? {
        guard items.indices.contains(index) else { return nil }
        var next = items
        next[index].x = x
        next[index].y = y
        return ColorSamplerSet(items: next)
    }

    /// nil on a bad index.
    func removing(_ index: Int) -> ColorSamplerSet? {
        guard items.indices.contains(index) else { return nil }
        var next = items
        next.remove(at: index)
        return ColorSamplerSet(items: next)
    }
}

/// One sampler's value at the document's depth; `unavailable` carries the
/// read's sentence when there is one.
enum ColorSamplerReading {
    case byte(PixelReadout)
    case precision(PrecisionReadout)
    /// A Lab Color document's codes (#547 phase 2d): L*, a*, b* and alpha.
    case lab(PrecisionReadout)
    case float(FloatReadout)
    case unavailable(String?)
}

/// The Info section's readout popup. Persisted as
/// `ViewToolOptions.colorSamplerReadoutIndex`.
enum ColorSamplerReadoutMode: Int, CaseIterable {
    case rgb
    case hsb
    case lab
    case codes16

    var title: String {
        switch self {
        case .rgb: return "RGB"
        case .hsb: return "HSB"
        case .lab: return "Lab"
        case .codes16: return "16-bit codes"
        }
    }
}

extension ColorSamplerReading {
    /// The row text for `mode`. 8 bits print the panel's own spellings; 16
    /// bits print RGB as 0…255 equivalents (codes / 257, one decimal) or the
    /// raw codes; 32-bit float prints linear components. HSB and Lab read
    /// "—" above 8 bits, and a mode with no meaning at a depth (codes at 8
    /// bits or float) falls back to its RGB. A Lab Color document reads its L*a*b* and its
    /// codes, and RGB and HSB "—".
    func text(_ mode: ColorSamplerReadoutMode) -> String {
        switch self {
        case .byte(let readout):
            switch mode {
            case .rgb, .codes16:
                return "\(readout.r), \(readout.g), \(readout.b)"
            case .hsb:
                let hsb = readout.hsbRounded
                return "\(hsb.h)°, \(hsb.s)%, \(hsb.b)%"
            case .lab:
                guard let lab = readout.lab else { return "—" }
                return String(format: "%.1f, %.1f, %.1f", lab.l, lab.a, lab.b)
            }
        case .precision(let readout):
            let codes = readout.codes.prefix(3)
            switch mode {
            case .rgb:
                return codes.map { String(format: "%.1f", $0 / 257) }.joined(separator: ", ")
            case .codes16:
                return Self.codes16Text(readout.codes)
            case .hsb, .lab:
                return "—"
            }
        case .lab(let readout):
            // The Info footnote's rule: a Lab document has no RGB or HSB reading.
            switch mode {
            case .lab:
                return LabDocument.readoutText(readout.codes.map { $0 / 65535 })
            case .codes16:
                return Self.codes16Text(readout.codes)
            case .rgb, .hsb:
                return "—"
            }
        case .float(let readout):
            switch mode {
            case .rgb, .codes16:
                return readout.values.prefix(3).map { String(format: "%.4g", $0) }
                    .joined(separator: ", ")
            case .hsb, .lab:
                return "—"
            }
        case .unavailable:
            return "—"
        }
    }

    /// The 16-bit codes row: the first three codes, whole when they are, else one decimal.
    private static func codes16Text(_ codes: [Double]) -> String {
        codes.prefix(3).map {
            $0 == $0.rounded() ? String(Int($0)) : String(format: "%.1f", $0)
        }.joined(separator: ", ")
    }
}

/// Reads a sampler list: the committed composite, or a sheet's preview.
enum ColorSamplerReader {
    /// The committed composite, resolved ONCE per call at the document's
    /// depth (`byteComposite(of:)` or `currentPrecisionProjection()`); each
    /// sampler averages its (sampleSize)² block. A throw turns every row
    /// into `.unavailable(sentence)`.
    static func read(_ set: ColorSamplerSet, in document: ImageDocument) -> [ColorSamplerReading] {
        guard let doc = document.doc else { return set.items.map { _ in .unavailable(nil) } }
        do {
            if !doc.isHighDepth {
                let projection = try document.byteComposite(of: doc)
                return set.items.map { sampler in
                    PixelReadout.at((sampler.x, sampler.y), projection: projection, doc: doc,
                                    reach: sampler.sampleSize / 2)
                        .map { .byte($0) } ?? .unavailable(nil)
                }
            }
            let projection = try document.currentPrecisionProjection()
            return try set.items.map { sampler in
                let point = (x: sampler.x, y: sampler.y)
                let reach = sampler.sampleSize / 2
                if doc.isFloat {
                    return try FloatReadout.at(point, projection: projection, reach: reach)
                        .map { .float($0) } ?? .unavailable(nil)
                }
                let read = try PrecisionReadout.at(point, projection: projection, reach: reach)
                // #547 Lab phase 2d: the projection holds the L*a*b* codes.
                if doc.isLab { return read.map { .lab($0) } ?? .unavailable(nil) }
                return read.map { .precision($0) } ?? .unavailable(nil)
            }
        } catch {
            return set.items.map { _ in .unavailable(error.localizedDescription) }
        }
    }

    /// An 8-bit sheet's preview image, read the way `ops_stats::sample_mean`
    /// reads the composite: the per-channel truncating mean of the block's
    /// in-bounds pixels. nil when the image is not the canvas-sized straight
    /// RGBA8 the byte preview path makes in the document's own space.
    static func read(_ set: ColorSamplerSet, preview: CGImage, doc: RasterDocument)
        -> [ColorSamplerReading]?
    {
        let width = preview.width, height = preview.height
        guard doc.precision == nil, width == doc.width, height == doc.height,
              preview.bitsPerComponent == 8, preview.bitsPerPixel == 32,
              preview.alphaInfo == .last, preview.bytesPerRow == width * 4,
              preview.colorSpace == doc.colorSpace,
              let data = preview.dataProvider?.data,
              CFDataGetLength(data) >= width * height * 4,
              let bytes = CFDataGetBytePtr(data) else { return nil }
        return set.items.map { sampler in
            let reach = sampler.sampleSize / 2
            var sum = [UInt32](repeating: 0, count: 4)
            var count: UInt32 = 0
            for py in (sampler.y - reach)...(sampler.y + reach) where py >= 0 && py < height {
                for px in (sampler.x - reach)...(sampler.x + reach) where px >= 0 && px < width {
                    let offset = (py * width + px) * 4
                    for channel in 0..<4 { sum[channel] += UInt32(bytes[offset + channel]) }
                    count += 1
                }
            }
            guard count > 0 else { return .unavailable(nil) }
            let rgba = (r: UInt8(sum[0] / count), g: UInt8(sum[1] / count),
                        b: UInt8(sum[2] / count), a: UInt8(sum[3] / count))
            return .byte(PixelReadout.reading(rgba, at: (sampler.x, sampler.y), doc: doc))
        }
    }

    /// A 16-bit sheet's authoritative preview projection; `lab` reads a Lab Color document's
    /// codes (#547 phase 2d).
    static func read(_ set: ColorSamplerSet, precisionPreview: RasterPrecisionImage,
                     lab: Bool = false) -> [ColorSamplerReading]
    {
        set.items.map { sampler in
            do {
                let read = try PrecisionReadout.at((sampler.x, sampler.y),
                                                   projection: precisionPreview,
                                                   reach: sampler.sampleSize / 2)
                if lab { return read.map { .lab($0) } ?? .unavailable(nil) }
                return read.map { .precision($0) } ?? .unavailable(nil)
            } catch {
                return .unavailable(error.localizedDescription)
            }
        }
    }
}

extension Notification.Name {
    /// Posted by `ImageDocument.colorSamplers` on a real change.
    static let imageDocumentColorSamplersDidChange =
        Notification.Name("ImageDocumentColorSamplersDidChange")
}
