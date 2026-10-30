import AppKit

// A preview's `PrecisionGeometryWork` is `interactive`, so the Crop tool's preview straighten keeps
// the core's interactive work limit while its commit has none under its job control (no
// non-interactive geometry request has); the same flag rides the `.transform`,
// `.transformSet` and `.duplicateTransform` requests
// (`PrecisionPixelCommand.affine`/`.perspective(…, interactive:)`,
// `GeometrySetRequest.command(interactive:)`, `geometrySetPrepared(…, interactive:)`), in `compute`
// AND `plannedCommands` so plan and run agree, and every commit caller keeps the default `false`.
// The `.transform` and `.crop` arms run the core through `editingGeometry(request:control:)` with
// the job's own control (`.crop` on a child, since its artwork stage uses the control after), so
// Cancel reaches the kernel and the geometry stage reports progress.
//
// A single RAW layer's `.transform` reaches the core as the source `transform` request
// (`retainedAffineRequest` keeps every key), so its preview's `interactive` keeps `MAX_WORK_UNITS`
// there and its commit takes its job's cap (none under a control), which also fixed the 63467a5
// preview refusal "unknown field `interactive`".

/// Value-only geometry captured at the start of an interactive gesture.
enum PrecisionGeometryRequest {
    case move(layers: [Int], dx: Int, dy: Int)
    /// The Move tool's ⌥ duplicate with no selection: duplicate
    /// `layers`, then move the copies (`RasterDocument.duplicatingAndMoving`).
    case duplicateMove(layers: [Int], dx: Int, dy: Int)
    case transform(layer: Int, matrix: CGAffineTransform, quad: [CGPoint]?, sampler: DrResizeFilter)
    case transformSet(layers: [Int], sourceRect: CGRect, matrix: CGAffineTransform,
                      quad: [CGPoint]?, sampler: DrResizeFilter)
    /// Free Transform a Copy with no selection: duplicate `layers` (the
    /// ORIGINALS in the captured source), then transform the copies.
    case duplicateTransform(layers: [Int], sourceRect: CGRect, matrix: CGAffineTransform,
                            quad: [CGPoint]?, sampler: DrResizeFilter)
    /// `deleteCropped`: the Crop tool's Delete Cropped Pixels — an
    /// angle-0 crop runs as the identity straighten with the flag.
    /// `background`: a window past the canvas's Background fill.
    case crop(rect: CGRect, angle: Double, sampler: DrResizeFilter, deleteCropped: Bool = false,
              background: CropBackground? = nil)
    /// Move Selected Pixels: the shared request plus canvas u16 coverage.
    case moveSelected(command: PrecisionPixelCommand, coverage: [UInt16])
    /// Free Transform of the selected pixels: the same, `transform_selected`.
    case transformSelected(command: PrecisionPixelCommand, coverage: [UInt16])
    /// Layer ▸ Align: the core's `align_set`; `edge` is
    /// `align_layers`' vocabulary.
    case align(layers: [Int], edge: String, toCanvas: Bool)
    /// Layer ▸ Distribute: the core's `distribute_set`.
    case distribute(layers: [Int], vertical: Bool)
}

struct PrecisionGeometryWork {
    let source: RasterDocument
    let request: PrecisionGeometryRequest
    let artwork: ArtworkGeometryWork?
    /// `.duplicateTransform`'s duplicate, made on main when the originals'
    /// subtrees hold editable artwork so the artwork is captured over the
    /// COPIES (`.transformSet`'s rule), and `.duplicateMove`'s when the
    /// copies carry a pattern fill (the same rule); nil otherwise.
    private let duplicated: RasterDocument?
    /// A preview's work: a `.crop` straighten and a
    /// `.transform`, `.transformSet` or `.duplicateTransform` keep the core's
    /// interactive pixel-work limit; the commit's have none under a job
    /// control (a refused preview falls back to a reduced proxy or to
    /// the commit request, `PrecisionGeometryGesture.request`), and the plan
    /// and the run send the same request.
    let interactive: Bool
    /// #1 Indexed phase 3b: on an Indexed source, the colour an uncovered pixel's entry is
    /// nearest with no transparent index (the UI's background swatch); nil is white. Only
    /// `.move`, `.transform`, `.transformSet`, `.crop`, `.moveSelected` and `.transformSelected`
    /// carry it (`carriesIndexedBackground`); a duplicate, Align and Distribute keep their own
    /// arms, so the core's refusals stand.
    let indexedBackground: NSColor?

    init(source: RasterDocument, request: PrecisionGeometryRequest,
         interactive: Bool = false, indexedBackground: NSColor? = nil) throws {
        guard source.precision != nil else {
            throw DoctoredCoreError(message: "A high-depth geometry source is required.")
        }
        self.source = source
        self.request = request
        self.interactive = interactive
        self.indexedBackground = indexedBackground
        if case let .duplicateTransform(layers, rect, matrix, quad, sampler) = request,
           MoveDuplicateLayers.subtrees(of: layers, in: source)
               .contains(where: source.hasArtworkDescription),
           let made = try source.duplicateLayers(layers) {
            let copies = MoveDuplicateLayers.copies(of: layers, in: source)
            let set: GeometrySetRequest = quad.map {
                .perspective(layers: copies, sourceRect: rect, quad: $0, sampler: sampler)
            } ?? .affine(layers: copies, matrix: matrix, sampler: sampler)
            duplicated = made
            artwork = try ArtworkGeometryWork(source: made, request: set,
                                              rasterizeArtwork: true)
            return
        }
        // A moved pattern fill re-renders at the moved reference, so a
        // move or a duplicate-move carrying one captures its drawing here, on
        // main, and `compute` runs `geometrySetPrepared` (FillLayer.swift's
        // PIN); every other move keeps the core's integer translation.
        if case let .duplicateMove(layers, dx, dy) = request, dx != 0 || dy != 0,
           source.carriesPatternFill(moving: layers),
           let made = try source.duplicateLayers(layers) {
            let copies = MoveDuplicateLayers.copies(of: layers, in: source)
            if made.carriesPatternFill(moving: copies) {
                duplicated = made
                artwork = try ArtworkGeometryWork(
                    source: made, request: Self.translation(copies, dx: dx, dy: dy),
                    rasterizeArtwork: true)
                return
            }
        }
        duplicated = nil
        if case let .move(layers, dx, dy) = request, dx != 0 || dy != 0,
           source.carriesPatternFill(moving: layers) {
            artwork = try ArtworkGeometryWork(
                source: source, request: Self.translation(layers, dx: dx, dy: dy),
                rasterizeArtwork: true)
        } else if case let .transformSet(layers, rect, matrix, quad, sampler) = request,
           source.movingSet(layers).contains(where: source.hasArtworkDescription) {
            let set: GeometrySetRequest = quad.map {
                .perspective(layers: layers, sourceRect: rect, quad: $0, sampler: sampler)
            } ?? .affine(layers: layers, matrix: matrix, sampler: sampler)
            artwork = try ArtworkGeometryWork(source: source, request: set,
                                              rasterizeArtwork: true)
        } else if case let .crop(rect, angle, _, _, _) = request, angle != 0,
                  !source.artworkIndices.isEmpty {
            let window = CropSession.roundedWindow(rect,
                canvas: CGSize(width: source.width, height: source.height))
            var map = Self.straightenMatrix(window: window, angle: angle)
            map.tx -= window.minX
            map.ty -= window.minY
            artwork = try ArtworkGeometryWork(
                source: source, affineMap: map,
                canvas: (Int(window.width), Int(window.height)))
        } else if case let .crop(rect, angle, _, _, _) = request, angle == 0,
                  !source.fillIndices.isEmpty {
            // A plain crop keeps text and shape rasters, but a fill
            // layer is re-rendered to cover the window.
            let window = CropSession.roundedWindow(rect,
                canvas: CGSize(width: source.width, height: source.height))
            artwork = try ArtworkGeometryWork(
                fillsOf: source, canvas: (Int(window.width), Int(window.height)))
        } else {
            artwork = nil
        }
    }

    /// Plan native work before either preview or final admission, as the two
    /// sequential stages: the core commands (their scratch includes
    /// the native output) and the described-artwork re-render. The projection
    /// is admitted separately against the actual result after geometry.
    func stageEstimates() throws -> (core: UInt64, artwork: UInt64) {
        guard let precision = source.precision else {
            throw DoctoredCoreError(message: "A high-depth geometry source is required.")
        }
        let context = try PrecisionWorkContext(source: precision)
        let commands = try plannedCommands(interactive: interactive)
        var total: UInt64 = 65_536
        for command in commands {
            let plan = try precision.pixelEditPlan(
                request: command.request, workingLimit: context.limit,
                otherRetained: context.otherRetained)
            total = try PrecisionDocumentDescriptor.sum(total,
                PrecisionDocumentDescriptor.integer(plan, "scratch_bytes"))
        }
        return (total, artwork?.scratchBytes ?? 0)
    }

    /// The sum of `work_units` over the plans of
    /// `plannedCommands(interactive: false)` (what a reduced preview is
    /// sized from, `GeometryProxyScale`); nil for a request other than
    /// `.transform` / `.transformSet` / `.duplicateTransform` / `.crop`, when a
    /// plan refuses, or when one carries no `work_units`.
    func commitWorkUnits() -> UInt64? {
        switch request {
        case .transform, .transformSet, .duplicateTransform, .crop: break
        default: return nil
        }
        guard let precision = source.precision,
              let context = try? PrecisionWorkContext(source: precision),
              let commands = try? plannedCommands(interactive: false) else { return nil }
        var total: UInt64 = 0
        for command in commands {
            guard let plan = try? precision.pixelEditPlan(
                      request: command.request, workingLimit: context.limit,
                      otherRetained: context.otherRetained),
                  let units = try? PrecisionDocumentDescriptor.integer(plan, "work_units")
            else { return nil }
            let (sum, overflow) = total.addingReportingOverflow(units)
            guard !overflow else { return nil }
            total = sum
        }
        return total
    }

    /// Core + artwork, a preview's whole figure (reserved as core, with
    /// artwork held as a payload lease).
    func scratchEstimate() throws -> UInt64 {
        let stages = try stageEstimates()
        return try PrecisionDocumentDescriptor.sum(stages.core, stages.artwork)
    }

    /// `admittingArtwork` (the final job's worker only, never a preview)
    /// admits the artwork stage immediately before it does work.
    func compute(control: RasterJobControl, admittingArtwork: Bool = false) throws -> RasterDocument? {
        guard !control.isCancelled else { return nil }
        // #1 Indexed phase 3b: the one planned command, carrying the background key (`.crop`
        // adds it inside its own arm below, keeping its child control and artwork steps).
        if carriesIndexedBackground {
            switch request {
            case let .moveSelected(_, coverage), let .transformSelected(_, coverage):
                guard let command = try plannedCommands(interactive: interactive).first else {
                    return nil
                }
                return try source.editingSelectedPixels(request: command.request,
                                                        coverage: coverage, control: control)
            case .move, .transform, .transformSet:
                guard let command = try plannedCommands(interactive: interactive).first else {
                    return nil
                }
                return try source.editingGeometry(request: command.request, control: control)
            case .crop, .duplicateMove, .duplicateTransform, .align, .distribute:
                break
            }
        }
        switch request {
        case let .move(layers, dx, dy):
            if let artwork {
                return try source.geometrySetPrepared(
                    Self.translation(layers, dx: dx, dy: dy), artwork: artwork,
                    control: control, admittingArtwork: admittingArtwork)
            }
            return try source.precisionMoveLayers(layers, dx: dx, dy: dy, control: control)
        case let .duplicateMove(layers, dx, dy):
            if let artwork, let duplicated {
                let copies = MoveDuplicateLayers.copies(of: layers, in: source)
                return try duplicated.geometrySetPrepared(
                    Self.translation(copies, dx: dx, dy: dy), artwork: artwork,
                    control: control, admittingArtwork: admittingArtwork) ?? duplicated
            }
            return try source.duplicatingAndMoving(layers, dx: dx, dy: dy, control: control)?
                .document
        case let .moveSelected(command, coverage), let .transformSelected(command, coverage):
            return try source.editingSelectedPixels(request: command.request, coverage: coverage,
                                                    control: control)
        case .align, .distribute:
            guard let command = try alignmentCommand() else { return nil }
            return try source.editingGeometry(request: command.request, control: control)
        case let .transform(layer, matrix, quad, sampler):
            let filter = try RasterDocument.precisionFilter(sampler)
            let command = try quad.map {
                try PrecisionPixelCommand.perspective(
                    layer: layer, target: .pixels, quad: $0, filter: filter,
                    interactive: interactive)
            } ?? PrecisionPixelCommand.affine(
                layer: layer, target: .pixels, matrix: matrix, filter: filter,
                interactive: interactive)
            // The job's own control reaches the kernel, so Cancel stops it
            // and the geometry stage reports progress.
            return try source.editingGeometry(request: command.request, control: control)
        case let .transformSet(layers, rect, matrix, quad, sampler):
            let set: GeometrySetRequest = quad.map {
                .perspective(layers: layers, sourceRect: rect, quad: $0, sampler: sampler)
            } ?? .affine(layers: layers, matrix: matrix, sampler: sampler)
            return try source.geometrySetPrepared(set, artwork: artwork, control: control,
                                                  admittingArtwork: admittingArtwork,
                                                  interactive: interactive)
        case let .duplicateTransform(layers, rect, matrix, quad, sampler):
            guard let duplicated = try self.duplicated ?? source.duplicateLayers(layers) else {
                return nil
            }
            let copies = MoveDuplicateLayers.copies(of: layers, in: source)
            let set: GeometrySetRequest = quad.map {
                .perspective(layers: copies, sourceRect: rect, quad: $0, sampler: sampler)
            } ?? .affine(layers: copies, matrix: matrix, sampler: sampler)
            return try duplicated.geometrySetPrepared(set, artwork: artwork, control: control,
                                                      admittingArtwork: admittingArtwork,
                                                      interactive: interactive)
        case let .crop(rect, angle, sampler, deleteCropped, background):
            let window = try roundedCrop(rect, in: source)
            let full = window.minX == 0 && window.minY == 0
                && window.width == CGFloat(source.width)
                && window.height == CGFloat(source.height)
            guard angle != 0 || !full else { return nil }
            var command = try Self.cropCommand(window: window, angle: angle, sampler: sampler,
                                               interactive: interactive,
                                               deleteCropped: deleteCropped,
                                               background: background, source: source)
            if carriesIndexedBackground {
                command = try IndexedDocument.backgrounded(command, indexedBackground, in: source)
            }
            // A child of the job's control, so Cancel reaches the kernel and
            // the artwork stage below still has its own children.
            guard var result = try control.withChildOperation({ child in
                try source.editingGeometry(request: command.request, control: child)
            }) else { return nil }
            // A pure window move leaves every described raster intact. The
            // core clears descriptions touched by geometry; restore them
            // exactly as RasterDocument.cropped does, with thrown failures.
            if angle == 0 {
                for index in source.artworkIndices {
                    guard let description = source.layerMeta(index) else {
                        throw DoctoredCoreError(message: "The artwork description could not be preserved.")
                    }
                    result = try result.restoringArtworkDescription(description, layer: index)
                }
            }
            // The straighten's artwork, or a plain crop's fill layers.
            if let artwork, !artwork.members.isEmpty {
                if admittingArtwork {
                    _ = try RasterResources.shared.admit(scratch: artwork.scratchBytes)
                }
                let placed = result
                result = try artwork.prepareAfter(result, control: control)
                result = try result.followingHostPlacement(
                    of: artwork.members.map(\.index), placedBy: placed)
            }
            return result
        }
    }

    func cropWindow() throws -> CGRect? {
        guard case let .crop(rect, _, _, _, _) = request else { return nil }
        return try roundedCrop(rect, in: source)
    }

    /// Whether this work sends the core's `indexed_background` (#1 Indexed phase 3b): an
    /// Indexed or (#1 Bitmap phase 2a-i) Bitmap source given a colour, on the requests that
    /// move or uncover the layer's own pixels.
    private var carriesIndexedBackground: Bool {
        guard indexedBackground != nil, source.isIndexed || source.isBitmap else { return false }
        switch request {
        case .move, .transform, .transformSet, .crop, .moveSelected, .transformSelected:
            return true
        case .duplicateMove, .duplicateTransform, .align, .distribute:
            return false
        }
    }

    /// The core requests the work plans, with the background key merged when it carries one.
    private func plannedCommands(interactive: Bool) throws -> [PrecisionPixelCommand] {
        let commands = try unkeyedCommands(interactive: interactive)
        guard carriesIndexedBackground else { return commands }
        return try commands.map {
            try IndexedDocument.backgrounded($0, indexedBackground, in: source)
        }
    }

    private func unkeyedCommands(interactive: Bool) throws -> [PrecisionPixelCommand] {
        switch request {
        case let .move(layers, dx, dy), let .duplicateMove(layers, dx, dy):
            // A duplicate plans as the move of its originals.
            guard dx != 0 || dy != 0 else { return [] }
            let matrix = CGAffineTransform(translationX: CGFloat(dx), y: CGFloat(dy))
            return [try GeometrySetRequest.affine(layers: layers, matrix: matrix,
                                                  sampler: DR_FILTER_NEAREST).command()]
        case let .moveSelected(command, _), let .transformSelected(command, _):
            return [command]
        case .align, .distribute:
            return try alignmentCommand().map { [$0] } ?? []
        case let .transform(layer, matrix, quad, sampler):
            let filter = try RasterDocument.precisionFilter(sampler)
            if let quad {
                return [try .perspective(layer: layer, target: .pixels, quad: quad, filter: filter,
                                         interactive: interactive)]
            }
            return [try .affine(layer: layer, target: .pixels, matrix: matrix, filter: filter,
                                interactive: interactive)]
        case let .transformSet(layers, rect, matrix, quad, sampler),
             let .duplicateTransform(layers, rect, matrix, quad, sampler):
            // A duplicate plans as the transform of its originals, the way
            // `.duplicateMove` plans as the move.
            let set: GeometrySetRequest = quad.map {
                .perspective(layers: layers, sourceRect: rect, quad: $0, sampler: sampler)
            } ?? .affine(layers: layers, matrix: matrix, sampler: sampler)
            return [try set.command(interactive: interactive)]
        case let .crop(rect, angle, sampler, deleteCropped, background):
            let window = try roundedCrop(rect, in: source)
            let full = window.minX == 0 && window.minY == 0
                && window.width == CGFloat(source.width)
                && window.height == CGFloat(source.height)
            guard angle != 0 || !full else { return [] }
            return [try Self.cropCommand(window: window, angle: angle, sampler: sampler,
                                         interactive: interactive,
                                         deleteCropped: deleteCropped,
                                         background: background, source: source)]
        }
    }

    /// `.crop`'s one core request, for the run and the plan alike: the plain
    /// crop at angle 0, else the straighten; with Delete Cropped Pixels
    /// an angle-0 crop is the identity straighten over the window at Nearest
    /// (the crop bit for bit) carrying the flag, and so is any angle-0 window
    /// past the canvas, which also carries `background`.
    private static func cropCommand(window: CGRect, angle: Double, sampler: DrResizeFilter,
                                    interactive: Bool, deleteCropped: Bool,
                                    background: CropBackground?,
                                    source: RasterDocument) throws -> PrecisionPixelCommand {
        let extended = CropSession.extends(
            window, canvas: CGSize(width: source.width, height: source.height))
        if angle == 0 && !deleteCropped && !extended {
            return try .crop(x: Int(window.minX), y: Int(window.minY),
                             width: Int(window.width), height: Int(window.height))
        }
        let matrix = angle == 0 ? .identity : straightenMatrix(window: window, angle: angle)
        return try .straighten(
            matrix: matrix,
            filter: angle == 0 ? .nearest : RasterDocument.precisionFilter(sampler),
            x: Int(window.minX), y: Int(window.minY),
            width: Int(window.width), height: Int(window.height),
            interactive: interactive, deleteCropped: deleteCropped,
            background: extended ? background?.fields : nil)
    }

    /// `.align` / `.distribute`'s one typed request; nil for
    /// every other request.
    private func alignmentCommand() throws -> PrecisionPixelCommand? {
        switch request {
        case let .align(layers, edge, toCanvas):
            return try PrecisionPixelCommand([
                "op": "align_set", "layers": layers, "edge": edge, "to_canvas": toCanvas,
            ])
        case let .distribute(layers, vertical):
            return try PrecisionPixelCommand([
                "op": "distribute_set", "layers": layers, "vertical": vertical,
            ])
        default:
            return nil
        }
    }

    private func roundedCrop(_ rect: CGRect, in source: RasterDocument) throws -> CGRect {
        let window = CropSession.roundedWindow(rect, canvas: CGSize(
            width: source.width, height: source.height))
        guard window.width >= 1, window.height >= 1 else {
            throw DoctoredCoreError(message: "Crop needs a nonempty window.")
        }
        return window
    }

    /// A move's set request: the integer translation `precisionMoveLayers`
    /// sends, at Nearest.
    private static func translation(_ layers: [Int], dx: Int, dy: Int) -> GeometrySetRequest {
        .affine(layers: layers, matrix: CGAffineTransform(translationX: CGFloat(dx), y: CGFloat(dy)),
                sampler: DR_FILTER_NEAREST)
    }

    private static func straightenMatrix(window: CGRect, angle: Double) -> CGAffineTransform {
        let center = CGPoint(x: window.midX, y: window.midY)
        return CGAffineTransform(translationX: center.x, y: center.y)
            .rotated(by: CGFloat(-angle) * .pi / 180)
            .translatedBy(x: -center.x, y: -center.y)
    }
}
