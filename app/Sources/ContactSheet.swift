import AppKit

// `ImageDocument.contactSheet` is the recordable form; `ContactSheet.cells` encodes layer cells
// CANVAS-FRAMED (`trimmed: false`), which is why an empty item is a background-coloured cell rather
// than a refusal, and a 16-bit document's cell is its display projection re-tagged with the
// document profile. `ContactSheet.bitDepthRefusal` is checked in `deliver` and in both
// `contact_sheet*` handlers BEFORE any cell is built. Captions are the ONE place this feature draws
// type: `Bitmap.renderStraightRGBA(appKit: true, space: sRGB)` with `DS.sans(11)`, black on a light
// background and white on a dark one by Rec.709 luma — the core places bitmaps and rasterizes no
// text.

/// Phase 11B contact sheets: a grid of thumbnails composed from a document's
/// layers, from other open documents or from files, delivered as a new
/// document or as an exported file through an 11A preset.
///
/// The CORE composes (`contact_sheet.rs`): the geometry, the fit, the
/// placement and the background are all its. This side decides what the cells
/// ARE, renders the caption BITMAPS — type is a platform concern, so the core
/// places bitmaps and rasterizes no text — and delivers the result.
///
/// **Nothing here changes a source.** Every cell comes from a captured
/// immutable handle, and the sheet is a new picture.

// MARK: - The layout

struct ContactSheetLayout: Equatable {
    var columns: Int
    var cellWidth: Int
    var cellHeight: Int
    var gutter: Int
    var captionHeight: Int
    /// Whether a caption bitmap is rendered at all. A caption strip with no
    /// caption is a deliberate state — it is how a sheet gets even spacing
    /// under its cells — so the two are separate.
    var captions: Bool
    var background: String
    var filterName: String

    static let defaults = ContactSheetLayout(columns: 4, cellWidth: 256, cellHeight: 256,
                                             gutter: 16, captionHeight: 24, captions: true,
                                             background: "#ffffff", filterName: "lanczos3")

    /// The core's request object. Every bound is the core's, so this side
    /// never restates one.
    var json: [String: Any] {
        [
            "columns": columns,
            "cell_width": cellWidth,
            "cell_height": cellHeight,
            "gutter": gutter,
            "caption_height": captions ? captionHeight : 0,
            "background": background,
            "filter": filterName,
        ]
    }

    var requestJSON: String {
        guard let data = try? JSONSerialization.data(withJSONObject: json, options: [.sortedKeys])
        else { return "{}" }
        return String(decoding: data, as: UTF8.self)
    }

    static func decode(_ object: [String: Any]) throws -> ContactSheetLayout {
        let allowed: Set<String> = ["columns", "cell_width", "cell_height", "gutter",
                                    "caption_height", "captions", "background", "filter"]
        guard Set(object.keys).isSubset(of: allowed) else {
            throw DoctoredCoreError(message: "A contact sheet layout carries an unknown key.")
        }
        func integer(_ key: String, _ fallback: Int) throws -> Int {
            guard let value = object[key], !(value is NSNull) else { return fallback }
            if let number = value as? NSNumber { return number.intValue }
            if let text = value as? String, let parsed = Int(text) { return parsed }
            throw DoctoredCoreError(message: "A contact sheet \(key) must be a number.")
        }
        var layout = ContactSheetLayout.defaults
        layout.columns = try integer("columns", layout.columns)
        layout.cellWidth = try integer("cell_width", layout.cellWidth)
        layout.cellHeight = try integer("cell_height", layout.cellHeight)
        layout.gutter = try integer("gutter", layout.gutter)
        layout.captionHeight = try integer("caption_height", layout.captionHeight)
        if let value = object["captions"], !(value is NSNull) {
            guard let flag = value as? NSNumber else {
                throw DoctoredCoreError(message: "A contact sheet captions flag must be true or false.")
            }
            layout.captions = flag.boolValue
        }
        if let value = object["background"], !(value is NSNull) {
            guard let text = value as? String else {
                throw DoctoredCoreError(message: "A contact sheet background must be a string.")
            }
            layout.background = text
        }
        if let value = object["filter"], !(value is NSNull) {
            guard let text = value as? String,
                  ExportSettings.filterNames.contains(where: { $0.1 == text }) else {
                throw DoctoredCoreError(
                    message: "A contact sheet filter must be nearest, bilinear, catmull-rom or lanczos3.")
            }
            layout.filterName = text
        }
        return layout
    }
}

// MARK: - Sources and outputs

/// Where the cells come from. Every case produces 8-bit sRGB byte documents,
/// because that is what a contact sheet is composed at.
enum ContactSheetSource {
    case layers(ImageDocument, LayerExportItems)
    case documents([ImageDocument])
    case files([URL])
}

/// Where the sheet goes.
enum ContactSheetOutput {
    case document
    case export(settings: ExportSettings, destination: String, pattern: String, preset: String?)
}

struct ContactSheetCell {
    let source: RasterDocument
    let label: String
    /// A file cell's core-open profile refusal: the sentence naming
    /// an embedded profile the open converted from or ignored; nil otherwise.
    var profileRefusal: String? = nil
}

struct ContactSheetOutcome {
    let document: ImageDocument?
    let url: URL?
    let result: RasterExportResult?
    let cells: Int
    let width: Int
    let height: Int
    /// The compose's admitted working-memory plan
    /// (`ContactSheetAdmission`); `deliver` leaves it 0 and the caller fills it.
    let planBytes: UInt64
}

// MARK: - The builder

enum ContactSheet {
    /// A sheet is composed at 8 bits, so an export output asking for 16 is
    /// refused BEFORE any cell is built — otherwise the byte encoder's own
    /// "convert it to 16-bit first" would be the message, which describes a
    /// document rather than the sheet.
    static let bitDepthRefusal =
        "Contact sheets are composed at 8 bits; use an 8-bit preset or settings."

    /// Why an export output cannot land, asked BEFORE any cell is built:
    /// `bitDepthRefusal` first, then the export destination refusal —
    /// the sheet is always sRGB, so only a `.file` destination is described.
    /// nil for a document output.
    static func outputRefusal(_ output: ContactSheetOutput) -> String? {
        guard case .export(let settings, _, _, _) = output else { return nil }
        if settings.bitDepth != 8 { return bitDepthRefusal }
        return settings.profile.destinationRefusal
    }

    /// The cells a source names. Layer cells are encoded CANVAS-FRAMED, so an
    /// empty item is a background-coloured cell rather than a refusal, and
    /// every cell of one document lines up.
    ///
    /// Each layer encode and each core JPEG/PNG file open is admitted before
    /// it runs, with the cells already built as its `own` bytes: a
    /// layer cell charges `ExportCapture.scratchEstimate` as Export Layers
    /// does, a file cell `FlatOpenAdmission.charge(for:)` as
    /// `openDocument(at:)` does — other containers are not charged before
    /// decoding, there as here.
    static func cells(_ source: ContactSheetSource) throws -> [ContactSheetCell] {
        switch source {
        case .layers(let document, let mode):
            let items = LayerExport.items(document, mode)
            guard !items.isEmpty else {
                throw DoctoredCoreError(message: "There are no \(mode.rawValue) layers to compose.")
            }
            let capture = try ExportCapture(document)
            var settings = ExportSettings.defaults(for: document)
            settings.format = .png
            settings.bitDepth = 8
            settings.dither = "none"
            settings.size = .none
            settings.profile = .srgb
            settings.sharpen = nil
            settings.embedProfile = true
            settings.stripMetadata = true
            settings.metadata = nil
            let scratch = try capture.scratchEstimate(settings)
            var built: UInt64 = 0
            return try items.map { item in
                _ = try RasterResources.shared.admit(
                    scratch: scratch,
                    own: PrecisionDocumentDescriptor.sum(capture.ownRetainedBytes, built),
                    remedy: capture.refusalRemedy(for: settings))
                let result = try capture.encodeLayer(index: item.index, trimmed: false,
                                                     settings: settings, control: nil)
                let image = try result.preview()
                guard let cell = RasterDocument.from(image: image) else {
                    throw DoctoredCoreError(message: "Could not build a cell for “\(item.name)”.")
                }
                built = try PrecisionDocumentDescriptor.sum(built, cell.ownRetainedBytes())
                return ContactSheetCell(source: cell, label: item.name)
            }
        case .documents(let documents):
            guard !documents.isEmpty else {
                throw DoctoredCoreError(message: "A contact sheet needs at least one document.")
            }
            return try documents.map { document in
                guard let source = document.doc else {
                    throw DoctoredCoreError(message: "A contact sheet source has no image loaded.")
                }
                let label = ExportNaming.baseName(of: document)
                if source.precision != nil {
                    // A 16-bit document has no byte handle; its display
                    // projection is the reduction a cell wants anyway, and it
                    // is re-tagged so the core knows which space it is in.
                    let display = try document.currentDisplayProjection()
                    guard let byte = RasterDocument.from(image: display) else {
                        throw DoctoredCoreError(message: "Could not build a cell for “\(label)”.")
                    }
                    // A Lab document's display projection is its Display P3 view, never
                    // the twin's codes (#547 Lab close), so it is tagged Display P3.
                    let profile = source.isLab ? RasterProfile.builtin(.displayP3) : source.iccProfile
                    let tagged = try profile.flatMap { try byte.assigningProfile($0) } ?? byte
                    return ContactSheetCell(source: tagged, label: label)
                }
                return ContactSheetCell(source: source, label: label)
            }
        case .files(let urls):
            guard !urls.isEmpty else {
                throw DoctoredCoreError(message: "A contact sheet needs at least one file.")
            }
            var built: UInt64 = 0
            return try urls.map { url in
                if let charge = try FlatOpenAdmission.charge(for: url) {
                    _ = try RasterResources.shared.admit(scratch: charge, own: built,
                                                         remedy: ContactSheetAdmission.remedy)
                }
                let source = try RasterDocument.open(url: url)
                built = try PrecisionDocumentDescriptor.sum(built, source.ownRetainedBytes())
                return ContactSheetCell(source: source,
                                        label: (url.lastPathComponent as NSString).deletingPathExtension,
                                        profileRefusal: RasterDocument.flatOpenProfileRefusal(at: url))
            }
        }
    }

    /// One caption strip, rendered with AppKit into straight sRGB RGBA8 — the
    /// ONE place this feature draws type. Black on a light background, white
    /// on a dark one, by Rec.709 luma, so the text is legible on whatever the
    /// layout's background colour is.
    static func captionImage(_ text: String, width: Int, height: Int,
                             background: String) -> RasterImage? {
        guard width > 0, height > 0, !text.isEmpty else { return nil }
        let space = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
        let dark = luma(of: background) < 0.5
        let colour = dark ? NSColor.white : NSColor.black
        let attributes: [NSAttributedString.Key: Any] = [
            .font: DS.sans(11),
            .foregroundColor: colour,
        ]
        guard let pixels = Bitmap.renderStraightRGBA(
            width: width, height: height, appKit: true, space: space, { _ in
                let string = NSAttributedString(string: text, attributes: attributes)
                let size = string.size()
                let origin = NSPoint(x: max(0, (CGFloat(width) - size.width) / 2),
                                     y: max(0, (CGFloat(height) - size.height) / 2))
                string.draw(at: origin)
                return true
            }) else { return nil }
        return RasterImage.from(rgba: pixels, width: width, height: height)
    }

    /// Rec.709 luma of `#rrggbb` / `#rrggbbaa`, in [0,1]. An unparsable
    /// colour is treated as light, which is the default background.
    private static func luma(of background: String) -> Double {
        let body = background.hasPrefix("#") ? String(background.dropFirst()) : background
        guard body.count == 6 || body.count == 8,
              let value = UInt32(body.prefix(6), radix: 16) else { return 1.0 }
        let r = Double((value >> 16) & 0xff) / 255.0
        let g = Double((value >> 8) & 0xff) / 255.0
        let b = Double(value & 0xff) / 255.0
        return 0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// Composes the sheet as a cancellable job off main. The captions
    /// are rendered here, on main (AppKit text); the compose's plan
    /// (`ContactSheetAdmission.composeScratch`, from the core's own sheet
    /// size and the largest cell) is admitted beside every distinct cell's
    /// own bytes before the job starts, and returned with the sheet. A cancel
    /// throws `JobCancelled` before anything is delivered.
    static func compose(_ cells: [ContactSheetCell], layout: ContactSheetLayout) throws
        -> (sheet: RasterImage, planBytes: UInt64) {
        let captions: [RasterImage?] = layout.captions && layout.captionHeight > 0
            ? cells.map { captionImage($0.label, width: layout.cellWidth,
                                       height: layout.captionHeight, background: layout.background) }
            : []
        let request = layout.requestJSON
        let sources = cells.map(\.source)
        let (width, height, _) = try RasterExport.contactSheetSize(request: request,
                                                                  count: sources.count)
        let largest = sources.map { UInt64($0.width) * UInt64($0.height) }.max() ?? 0
        let plan = try ContactSheetAdmission.composeScratch(sheetWidth: width, sheetHeight: height,
                                                            largestCellPixels: largest)
        var seen = Set<ObjectIdentifier>()
        var own: UInt64 = 0
        for source in sources where seen.insert(ObjectIdentifier(source)).inserted {
            own = try PrecisionDocumentDescriptor.sum(own, source.ownRetainedBytes())
        }
        _ = try RasterResources.shared.admit(scratch: plan, own: own,
                                             remedy: ContactSheetAdmission.remedy)
        let control = try RasterJobControl()
        let sheet = try BackgroundWork.wait(title: "Contact Sheet", control: control, makeTask: {
            try WorkerTask(scratchEstimate: plan) {
                try RasterExport.contactSheetCompose(request: request, sources: sources,
                                                     captions: captions, control: control)
            }
        })
        return (sheet, plan)
    }

    /// Deliver a composed sheet. `outputRefusal` is checked by the CALLER
    /// before composing as well, so no work is done for a request that cannot
    /// land; it is repeated here because this is the last gate.
    static func deliver(_ sheet: RasterImage, output: ContactSheetOutput, baseName: String,
                        date: Date) throws -> ContactSheetOutcome {
        let width = sheet.width
        let height = sheet.height
        switch output {
        case .document:
            guard let document = ImageDocument.makeUntitled(with: sheet,
                                                            profile: RasterProfile.builtin(.sRGB))
            else {
                throw DoctoredCoreError(message: "Could not create a document for the contact sheet.")
            }
            NSDocumentController.shared.addDocument(document)
            document.makeWindowControllers()
            document.showWindows()
            return ContactSheetOutcome(document: document, url: nil, result: nil, cells: 0,
                                       width: width, height: height, planBytes: 0)
        case .export(let settings, let destination, let pattern, let preset):
            if let refusal = outputRefusal(output) {
                throw DoctoredCoreError(message: refusal)
            }
            guard let source = RasterDocument.from(image: sheet) else {
                throw DoctoredCoreError(message: "Could not prepare the contact sheet for export.")
            }
            let tagged = try source.assigningProfile(RasterProfile.builtin(.sRGB)) ?? source
            let folder = URL(fileURLWithPath: (destination as NSString).expandingTildeInPath,
                             isDirectory: true)
            var isDirectory: ObjCBool = false
            guard FileManager.default.fileExists(atPath: folder.path, isDirectory: &isDirectory),
                  isDirectory.boolValue else {
                throw DoctoredCoreError(
                    message: "The export destination folder \(folder.path) does not exist.")
            }
            let result = try ExportCapture.hostFinish(
                tagged.exportEncoded(flat: nil, request: settings.requestJSON),
                settings: settings)
            let info = try result.info()
            let base = try RasterExport.expandName(
                pattern: pattern,
                tokens: ExportNaming.tokens(
                    name: baseName,
                    width: (info["width"] as? NSNumber)?.intValue ?? width,
                    height: (info["height"] as? NSNumber)?.intValue ?? height,
                    format: settings.format, date: date, preset: preset ?? ""))
            let url = folder.appendingPathComponent(base)
                .appendingPathExtension(settings.format.fileExtension)
            guard !FileManager.default.fileExists(atPath: url.path) else {
                throw DoctoredCoreError(
                    message: "\(url.path) already exists. A contact sheet never overwrites a file.")
            }
            try result.write(to: url, overwrite: false)
            return ContactSheetOutcome(document: nil, url: url, result: result, cells: 0,
                                       width: width, height: height, planBytes: 0)
        }
    }
}

extension ImageDocument {
    /// The RECORDABLE form: a contact sheet of THIS document's own items.
    /// `contact_sheet_from_sources` has no document to replay against and is
    /// deliberately not a step.
    func contactSheet(items: LayerExportItems, layout: ContactSheetLayout,
                      output: ContactSheetOutput) throws -> ContactSheetOutcome {
        if let refusal = ContactSheet.outputRefusal(output) {
            throw DoctoredCoreError(message: refusal)
        }
        let cells = try ContactSheet.cells(.layers(self, items))
        let (sheet, planBytes) = try ContactSheet.compose(cells, layout: layout)
        let outcome = try ContactSheet.deliver(sheet, output: output,
                                               baseName: ExportNaming.baseName(of: self),
                                               date: Date())
        var arguments: [String: Any] = ["items": items.rawValue, "layout": layout.json]
        switch output {
        case .document:
            arguments["output"] = ["kind": "document"]
        case .export(let settings, let destination, let pattern, let preset):
            arguments["output"] = ["kind": "export", "settings": settings.json,
                                   "destination": destination, "pattern": pattern,
                                   "preset": preset ?? ""]
        }
        ActionRecorder.shared.record(
            [ActionStep(tool: "contact_sheet", arguments: arguments,
                        note: "File ▸ Contact Sheet…")],
            on: self)
        return ContactSheetOutcome(document: outcome.document, url: outcome.url,
                                   result: outcome.result, cells: cells.count,
                                   width: outcome.width, height: outcome.height,
                                   planBytes: planBytes)
    }
}
