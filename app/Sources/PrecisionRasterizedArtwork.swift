import AppKit

/// A typed flat raster assembled from scalar drawing coverage and independent
/// continuous colors. Text/shape descriptions are intentionally not persisted
/// as editable metadata: only the new raster is authoritative in this slice.
struct PrecisionRasterizedArtwork {
    static let maximumTextUnits = 4096
    let edit: PrecisionPixelEdit
    let platformWorkingEstimate: UInt64
    let coverageInformation = "AppKit alpha-only coverage has 256 levels, promoted exactly by 257. "
        + "Authored RGB remains continuous; glyphs use uniform supplied ink, including color-font silhouettes."

    func run() throws -> RasterPrecisionDocument? {
        try edit.run(title: "Rasterizing Captured High-Depth Artwork")
    }

    static func text(_ source: RasterPrecisionDocument, payload: TextLayerPayload,
                     anchor: CGPoint, ink: PrecisionInk, space: PrecisionColorSpace,
                     name: String = "Rasterized Text") throws -> Self {
        try requireMainPreparation()
        guard !payload.string.isEmpty, payload.string.utf16.count <= maximumTextUnits,
              payload.font.utf8.count <= 512, (1...1000).contains(payload.size),
              TextToolOptions.trackingRange.contains(payload.tracking),
              TextToolOptions.leadingRange.contains(payload.leading),
              TextToolOptions.baselineShiftRange.contains(payload.baselineShift) else {
            throw failure("Typed text exceeds the bounded text/typography limits.")
        }
        // Reuse the existing strict typography validator. The legacy hex color
        // is irrelevant here; opaque white draws only glyph geometry.
        var geometry = payload
        geometry.color = "#FFFFFFFF"
        guard let json = geometry.json(), TextLayerPayload.decode(json) != nil else {
            throw failure("Typed text has invalid typography or geometry.")
        }
        let planning = UInt64(payload.string.utf16.count) * 4096 + 8 * 1024 * 1024
        _ = try RasterResources.shared.admit(scratch: planning)
        guard let drawing = TextLayer.drawingPlan(geometry, anchor: anchor, color: .white) else {
            throw failure("Typed text requires resolved layout bounds within the supported raster extent.")
        }
        return try assemble(source, drawings: [drawing], colors: [ink], space: space,
                            name: name, planningEstimate: planning)
    }

    static func shape(_ source: RasterPrecisionDocument, payload: ShapeLayerPayload,
                      anchor: CGPoint, fill: PrecisionInk?, stroke: PrecisionInk?,
                      space: PrecisionColorSpace, name: String = "Rasterized Shape") throws -> Self {
        try requireMainPreparation()
        guard payload.kind != "line" || fill == nil else {
            throw failure("A line has stroke coverage only.")
        }
        var geometry = payload
        geometry.fill = fill == nil ? "" : "#FFFFFFFF"
        geometry.stroke = stroke == nil ? "" : "#FFFFFFFF"
        guard let json = geometry.json(), ShapeLayerPayload.decode(json) != nil else {
            throw failure("Typed shape has invalid geometry or no drawable fill/stroke.")
        }
        let planning: UInt64 = 1024 * 1024
        _ = try RasterResources.shared.admit(scratch: planning)
        var drawings: [DescribedDrawingPlan] = []
        var colors: [PrecisionInk] = []
        if let fill {
            guard let drawing = ShapeLayer.drawingPlan(geometry, anchor: anchor, fill: .white, stroke: nil) else {
                throw failure("Typed shape fill cannot be rasterized within the supported extent.")
            }
            drawings.append(drawing)
            colors.append(fill)
        }
        if let stroke, geometry.strokeWidth > 0 {
            guard let drawing = ShapeLayer.drawingPlan(geometry, anchor: anchor, fill: nil, stroke: .white) else {
                throw failure("Typed shape stroke cannot be rasterized within the supported extent.")
            }
            drawings.append(drawing)
            colors.append(stroke)
        }
        return try assemble(source, drawings: drawings, colors: colors, space: space,
                            name: name, planningEstimate: planning)
    }

    private static func assemble(_ source: RasterPrecisionDocument,
                                 drawings: [DescribedDrawingPlan], colors: [PrecisionInk],
                                 space: PrecisionColorSpace, name: String,
                                 planningEstimate: UInt64) throws -> Self {
        guard let first = drawings.first, drawings.count == colors.count,
              drawings.allSatisfy({ $0.width == first.width && $0.height == first.height
                  && $0.offsetX == first.offsetX && $0.offsetY == first.offsetY }) else {
            throw failure("Typed artwork requires matching fill/stroke coverage extents.")
        }
        // Account byte coverage, promotion, plane joining and drawing allowance
        // before drawing. The platform allowance is an estimate, not measured
        // TextKit/CoreGraphics allocation or an OS resident-memory ceiling.
        let count = UInt64(first.width * first.height)
        let estimate = try PrecisionDocumentDescriptor.sum(
            count * UInt64(drawings.count) * 16, planningEstimate)
        _ = try RasterResources.shared.admit(scratch: estimate)
        var planes: [PrecisionScalarBuffer] = []
        for drawing in drawings {
            guard let coverage = drawing.coverage() else {
                throw failure("The platform refused an alpha-only artwork context.")
            }
            planes.append(try .promoting(coverage, width: drawing.width, height: drawing.height))
        }
        let input = try PrecisionScalarBuffer.joining(planes)
        let command = try PrecisionPixelCommand.rasterize(name: name, width: first.width,
            height: first.height, x: first.offsetX, y: first.offsetY, colors: colors, space: space)
        let edit = try PrecisionPixelEdit(source, command: command, input: input)
        return Self(edit: edit, platformWorkingEstimate: estimate)
    }

    private static func requireMainPreparation() throws {
        guard Thread.isMainThread, !BackgroundWork.occupied else {
            throw failure("Prepare typed artwork on the main thread after the current background operation finishes.")
        }
    }

    private static func failure(_ message: String) -> DoctoredCoreError { DoctoredCoreError(message: message) }
}
