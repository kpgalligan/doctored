import AppKit
import CoreGraphics
import Foundation
import UniformTypeIdentifiers

// `VectorImport` rasterizes ONE PDF page or an SVG at a chosen PPI over TRANSPARENCY into an sRGB
// document — an OPEN, not an edit, with the page and ppi recorded in the metadata report — and
// `VectorImportRequest` is the PSD-import-request pattern (a static mode a headless caller sets and
// restores) because `NSDocumentController` gives an opener nowhere to pass a parameter.

/// PDF and SVG raster IMPORT (Phase 11D): one page of a PDF, or an SVG, drawn
/// at a chosen resolution into an ordinary RGBA8 document.
///
/// # Rasterized, not retained
///
/// The result is PIXELS. A PDF page or an SVG is a resolution-independent
/// description, and keeping it as a re-rasterizable retained source is a real
/// feature (`RetainedImageSource`'s shape) that this slice deliberately does
/// not build: it would need a source kind, a native record, a re-render path
/// and its own qualification. What it builds instead is an honest one-way
/// import at a PPI the user picks, with the chosen page and PPI recorded in
/// the open's metadata report so the same import can be repeated exactly.
///
/// # The renderer
///
/// PDF goes through `CGPDFDocument`, which is the renderer Preview and
/// Quick Look use. SVG goes through `NSImage`, which reaches CoreSVG — the
/// system's own SVG renderer, and the only one on this machine; what it does
/// not draw (external references, unavailable fonts) it silently omits, which
/// is disclosed in the import report rather than detected here.
///
/// Both draw over TRANSPARENT, never white: a PDF page with no background is
/// transparent, and filling it white would be an edit the user never asked
/// for and could not undo.
enum VectorKind: Equatable {
    case pdf(pages: Int, pageSizes: [CGSize])
    case svg(size: CGSize)

    var name: String {
        switch self {
        case .pdf: return "pdf"
        case .svg: return "svg"
        }
    }

    var pages: Int {
        switch self {
        case .pdf(let pages, _): return pages
        case .svg: return 1
        }
    }

    var pageSizes: [CGSize] {
        switch self {
        case .pdf(_, let sizes): return sizes
        case .svg(let size): return [size]
        }
    }
}

/// What one vector import is asked for: which page, and at what resolution.
struct VectorImportChoice: Equatable {
    var page: Int = 1
    var ppi: Double = VectorImport.defaultPPI
}

enum VectorImport {
    /// The default import resolution. 300 ppi is the ordinary print figure
    /// and the one a scanned or placed page is most often wanted at; the
    /// sheet offers the usual ladder beside it.
    static let defaultPPI: Double = 300

    /// The resolutions the sheet offers, smallest first: screen, retina
    /// screen, the two print figures and a high-detail one.
    static let quickPPI: [Double] = [72, 144, 150, 300, 600]

    /// The bounds a PPI must sit in. The lower end is one pixel per inch —
    /// meaningless but harmless; the upper end is where the pixel count takes
    /// over as the real limit.
    static let ppiRange: ClosedRange<Double> = 1...30000

    /// True for a file this path OWNS. Extension and UTI both, because a PDF
    /// dragged from a mail attachment may carry neither reliably on its own.
    static func claims(_ url: URL) -> Bool {
        let ext = url.pathExtension.lowercased()
        if ext == "pdf" || ext == "svg" { return true }
        guard let type = UTType(filenameExtension: ext) else { return false }
        return type.conforms(to: .pdf) || type.conforms(to: .svg)
    }

    /// What the file is and how big its pages are, in POINTS — the query the
    /// sheet and `inspect_vector` both ask, with no pixel allocated.
    static func inspect(_ url: URL) throws -> VectorKind {
        if url.pathExtension.lowercased() == "svg" || (UTType(filenameExtension:
            url.pathExtension.lowercased())?.conforms(to: .svg) ?? false) {
            guard let image = NSImage(contentsOf: url), image.size.width > 0,
                  image.size.height > 0 else {
                throw DoctoredCoreError(message: "Could not read \(url.lastPathComponent) as an SVG.")
            }
            return .svg(size: image.size)
        }
        guard let document = CGPDFDocument(url as CFURL), document.numberOfPages > 0 else {
            throw DoctoredCoreError(message: "Could not read \(url.lastPathComponent) as a PDF.")
        }
        var sizes: [CGSize] = []
        for index in 1...document.numberOfPages {
            guard let page = document.page(at: index) else {
                throw DoctoredCoreError(message: "Page \(index) of \(url.lastPathComponent) is unreadable.")
            }
            let box = page.getBoxRect(.cropBox)
            let rotated = abs(page.rotationAngle % 180) == 90
            sizes.append(rotated ? CGSize(width: box.height, height: box.width)
                                 : CGSize(width: box.width, height: box.height))
        }
        return .pdf(pages: document.numberOfPages, pageSizes: sizes)
    }

    /// The pixel size `choice` produces for `kind`, so the sheet can show it
    /// and the refusals can be made before anything is drawn.
    static func pixelSize(_ kind: VectorKind, choice: VectorImportChoice) throws -> (Int, Int) {
        guard choice.page >= 1, choice.page <= kind.pages else {
            throw DoctoredCoreError(
                message: "Page \(choice.page) is outside this document's 1…\(kind.pages).")
        }
        guard choice.ppi.isFinite, ppiRange.contains(choice.ppi) else {
            throw DoctoredCoreError(
                message: "The import resolution must be between \(Int(ppiRange.lowerBound)) and "
                    + "\(Int(ppiRange.upperBound)) ppi.")
        }
        let points = kind.pageSizes[choice.page - 1]
        let scale = choice.ppi / 72.0
        let width = Int((points.width * CGFloat(scale)).rounded())
        let height = Int((points.height * CGFloat(scale)).rounded())
        guard width > 0, height > 0 else {
            throw DoctoredCoreError(message: "That page has no printable area.")
        }
        guard width * height <= RasterImage.maxResizePixels else {
            throw DoctoredCoreError(
                message: "\(width) × \(height) pixels is past this build's image limit. "
                    + "Choose a lower resolution.")
        }
        return (width, height)
    }

    /// Draws the chosen page at the chosen resolution into straight RGBA8.
    static func rasterize(_ url: URL, choice: VectorImportChoice) throws -> RasterImage {
        let kind = try inspect(url)
        let (width, height) = try pixelSize(kind, choice: choice)
        let points = kind.pageSizes[choice.page - 1]
        let scaleX = CGFloat(width) / points.width
        let scaleY = CGFloat(height) / points.height

        switch kind {
        case .pdf:
            guard let document = CGPDFDocument(url as CFURL),
                  let page = document.page(at: choice.page) else {
                throw DoctoredCoreError(message: "Page \(choice.page) could not be drawn.")
            }
            // The PAGE's own space, not the context's: `drawPDFPage` applies
            // the crop box and the rotation for us.
            guard let pixels = Bitmap.renderStraightRGBA(
                width: width, height: height, space: ColorProfile.sRGB, { context in
                    // `renderStraightRGBA` flips y so the canvas's origin is
                    // top-left; a PDF page draws in PDF coordinates, so the
                    // flip is undone for this one draw.
                    context.translateBy(x: 0, y: CGFloat(height))
                    context.scaleBy(x: scaleX, y: -scaleY)
                    context.interpolationQuality = .high
                    context.drawPDFPage(page)
                    return true
                }) else {
                throw DoctoredCoreError(message: "Could not rasterize \(url.lastPathComponent).")
            }
            guard let image = RasterImage.from(rgba: pixels, width: width, height: height) else {
                throw DoctoredCoreError(message: "Could not build an image from that page.")
            }
            return image
        case .svg:
            guard let source = NSImage(contentsOf: url) else {
                throw DoctoredCoreError(message: "Could not read \(url.lastPathComponent).")
            }
            guard let pixels = Bitmap.renderStraightRGBA(
                width: width, height: height, appKit: true, space: ColorProfile.sRGB, { context in
                    context.interpolationQuality = .high
                    let rect = CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height))
                    // The flipped context means AppKit's own draw lands the
                    // right way up without a second transform.
                    source.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1.0,
                                respectFlipped: true, hints: [.interpolation:
                                                                NSImageInterpolation.high])
                    return true
                }) else {
                throw DoctoredCoreError(message: "Could not rasterize \(url.lastPathComponent).")
            }
            guard let image = RasterImage.from(rgba: pixels, width: width, height: height) else {
                throw DoctoredCoreError(message: "Could not build an image from that drawing.")
            }
            return image
        }
    }

    /// What a vector import discloses, whatever it drew.
    static func warnings(_ kind: VectorKind) -> [String] {
        switch kind {
        case .pdf:
            return [
                "The page was RASTERIZED: text, vectors and any embedded colour management are gone, and the document holds pixels at the chosen resolution.",
                "Annotations, form fields, transparency groups and spot colours are drawn as the system's PDF renderer draws them; nothing is retained for re-rasterization.",
            ]
        case .svg:
            return [
                "The drawing was RASTERIZED at the chosen resolution by the system's SVG renderer; nothing is retained for re-rasterization.",
                "Web fonts, external references and features that renderer does not support are omitted silently.",
            ]
        }
    }
}

/// How a vector open decides its page and PPI: the sheet, or a caller that
/// already knows (MCP's `open_document`, a replayed Action).
///
/// The PSD import request's pattern exactly — a static mode that a headless
/// caller sets and restores around one open, because `NSDocumentController`
/// gives an opener no place to pass a parameter.
enum VectorImportRequest {
    enum Mode {
        case ask
        case headless(VectorImportChoice)
    }

    static var mode: Mode = .ask

    /// The choice an MCP call asked for, or nil when it named neither key.
    /// Refuses a non-integer page, a non-finite PPI, and anything outside the
    /// two ranges, by name.
    static func requested(page: Any?, ppi: Any?) throws -> VectorImportChoice? {
        var choice = VectorImportChoice()
        var named = false
        if let page {
            guard let value = (page as? NSNumber)?.intValue
                    ?? Int((page as? String) ?? ""), value >= 1 else {
                throw DoctoredCoreError(message: "page must be a whole number of 1 or more.")
            }
            choice.page = value
            named = true
        }
        if let ppi {
            guard let value = (ppi as? NSNumber)?.doubleValue
                    ?? Double((ppi as? String) ?? ""), value.isFinite,
                  VectorImport.ppiRange.contains(value) else {
                throw DoctoredCoreError(
                    message: "ppi must be between \(Int(VectorImport.ppiRange.lowerBound)) and "
                        + "\(Int(VectorImport.ppiRange.upperBound)).")
            }
            choice.ppi = value
            named = true
        }
        return named ? choice : nil
    }
}
