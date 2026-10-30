import AppKit
import CryptoKit

// `ImageDocument.exportLayers` is ONE cancellable job — a compound parent `RasterJobControl` with a
// child per item — and its report names every item, written or refused: there is no partial silent
// success, and it records ONE resolved `export_layers` step when at least one item was written and
// the job was not cancelled.

/// Phase 11B layer and group exports: every selected (or every visible, or
/// every) TOP-LEVEL item delivered as its own file, through 11A's request,
/// preset library and naming tokens.
///
/// **Nothing here changes a document.** Like every other export it reads the
/// captured immutable handle and produces bytes: no `applyEdit`, no
/// `updateChangeCount`, no history entry.
///
/// The whole batch is ONE cancellable job — a compound parent token with a
/// child per item — and its report names every item, written or refused.
/// There is no partial silent success: a run that wrote three of five says so,
/// and says why the other two did not.

// MARK: - The request

/// Which items a run delivers. An ITEM is a depth-0 entry: a raster layer or a
/// whole group with its members (the core's `top_level_items`).
enum LayerExportItems: String, CaseIterable {
    case selected
    case visible
    case all

    var displayName: String {
        switch self {
        case .selected: return "Selected layers"
        case .visible: return "Visible layers"
        case .all: return "All layers"
        }
    }
}

/// One item to deliver: where it is, what it is called, and its 1-based
/// position in THIS run (which is what `{index}` expands to).
struct LayerExportItem {
    let index: Int
    let name: String
    let ordinal: Int
}

struct LayerExportRequest {
    var items: LayerExportItems
    var trimmed: Bool
    var settings: ExportSettings
    var destination: String
    var pattern: String
    /// The preset this came from, for `{preset}` and the recorded step. Empty
    /// when the caller supplied settings directly.
    var preset: String?
}

/// What happened to ONE item. Exactly one of `url` and `refusal` is non-nil.
struct LayerExportOutcome {
    let item: LayerExportItem
    let url: URL?
    let refusal: String?
    let info: [String: Any]?
    let sha256: String?
}

struct LayerExportReport {
    let outcomes: [LayerExportOutcome]
    let cancelled: Bool

    var written: Int { outcomes.filter { $0.url != nil }.count }

    /// The notice's text: one line per item, so a run that refused something
    /// cannot be mistaken for one that did not.
    var text: String {
        var lines: [String] = []
        for outcome in outcomes {
            if let url = outcome.url {
                var line = "\(outcome.item.ordinal). \(outcome.item.name) → \(url.lastPathComponent)"
                if let info = outcome.info, let metadata = MetadataOutcome(info) {
                    line += " (\(metadata.sentence))"
                }
                lines.append(line)
            } else {
                lines.append("\(outcome.item.ordinal). \(outcome.item.name) → refused: "
                             + (outcome.refusal ?? "unknown reason"))
            }
        }
        if cancelled { lines.append("Cancelled before every item was delivered.") }
        return lines.joined(separator: "\n")
    }

    var json: [String: Any] {
        [
            "ok": !cancelled && outcomes.allSatisfy { $0.url != nil },
            "written": written,
            "refused": outcomes.filter { $0.url == nil }.count,
            "cancelled": cancelled,
            "items": outcomes.map { outcome -> [String: Any] in
                var object: [String: Any] = [
                    "index": outcome.item.index,
                    "name": outcome.item.name,
                    "ordinal": outcome.item.ordinal,
                ]
                if let url = outcome.url { object["path"] = url.path }
                if let refusal = outcome.refusal { object["refusal"] = refusal }
                if let sha = outcome.sha256 { object["encoded_sha256"] = sha }
                if let info = outcome.info {
                    if let width = info["width"] { object["width"] = width }
                    if let height = info["height"] { object["height"] = height }
                    if let layer = info["layer"] as? [String: Any] {
                        if let origin = layer["origin"] { object["origin"] = origin }
                        if let size = layer["size"] { object["size"] = size }
                    }
                    if let metadata = info["metadata"] { object["metadata"] = metadata }
                }
                return object
            },
        ]
    }
}

// MARK: - Item selection

enum LayerExport {
    /// The items a mode names, in PANEL order (top first) — which is the order
    /// the core reports them in and the order `ordinal` counts.
    ///
    /// `selected` takes the TOP-LEVEL ANCESTOR of every selected entry, so
    /// selecting a layer inside a group exports that group: the group is the
    /// picture the layer is part of, and a member pulled out of it is a
    /// composite nobody has seen.
    static func items(_ document: ImageDocument, _ mode: LayerExportItems) -> [LayerExportItem] {
        guard let source = document.doc else { return [] }
        let count = source.layerCount
        var roots: [Int] = []
        for index in 0..<count where source.layerDepth(index) == 0 { roots.append(index) }
        var wanted = roots
        switch mode {
        case .all:
            break
        case .visible:
            wanted = roots.filter { source.layerInfo($0)?.visible ?? false }
        case .selected:
            let selected = document.layerSelection.indices
            wanted = roots.filter { root in
                guard let subtree = source.layerSubtree(root) else { return false }
                return selected.contains { $0 >= subtree.start && $0 < subtree.end }
            }
        }
        // Panel order: the stack is bottom-first, the panel shows it top-first.
        return wanted.reversed().enumerated().map { ordinal, index in
            LayerExportItem(index: index,
                            name: source.layerInfo(index)?.name ?? "Layer \(index)",
                            ordinal: ordinal + 1)
        }
    }

    /// Why this pattern cannot name a batch of items, or nil. A pattern with
    /// neither `{layer}` nor `{index}` gives every item the same name, and the
    /// second write would be refused as an existing file — a refusal that
    /// reads like a bug, so it is caught here with a sentence that explains.
    /// The core applies the same rule through `layer_export`.
    static func patternProblem(_ pattern: String) -> String? {
        guard pattern.contains("{layer}") || pattern.contains("{index}") else {
            return "A layer export pattern must contain {layer} or {index}, "
                + "or every item would be given the same file name."
        }
        return nil
    }

    static let tokenHelp = "{layer} (the item's name), {index} (its position in this run, "
        + "zero-padded), {name} (the document's name), {width}, {height}, {format}, {date}, {preset}."
}

// MARK: - The run

extension ImageDocument {
    /// Deliver every item of `request`. ONE compound job for the whole batch,
    /// a child token per item, and a report naming every item.
    ///
    /// Three refusals per item, all deliberate and all the same as Quick
    /// Export's: an existing file is never overwritten, a name already claimed
    /// EARLIER IN THIS RUN is refused (case-insensitively, because the volume
    /// probably is), and a destination folder is never created.
    ///
    /// ONE resolved `export_layers` step is recorded when at least one item was
    /// written and the job was not cancelled.
    func exportLayers(_ request: LayerExportRequest, date: Date = Date()) throws -> LayerExportReport {
        let items = LayerExport.items(self, request.items)
        guard !items.isEmpty else {
            throw DoctoredCoreError(message: "There are no \(request.items.rawValue) layers to export.")
        }
        if let problem = LayerExport.patternProblem(request.pattern) {
            throw DoctoredCoreError(message: problem)
        }
        let folder = URL(fileURLWithPath: (request.destination as NSString).expandingTildeInPath,
                         isDirectory: true)
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: folder.path, isDirectory: &isDirectory),
              isDirectory.boolValue else {
            throw DoctoredCoreError(
                message: "The export destination folder \(folder.path) does not exist.")
        }
        // The core refuses each item's encode into a failing destination;
        // this asks the same rule once, so the whole run refuses
        // before its first item. "Document profile" never refuses.
        if let doc, let refusal = request.settings.destinationRefusal(for: doc) {
            throw DoctoredCoreError(message: refusal)
        }

        let capture = try ExportCapture(self)
        let settings = request.settings
        let trimmed = request.trimmed
        let presetName = request.preset ?? ""
        let baseName = ExportNaming.baseName(of: self)
        let dateText = date
        let parent = try RasterJobControl()
        try parent.beginCompoundOperation()

        // One conservative reservation for the whole run: the items are
        // encoded one at a time, so the peak is one item's, not their sum.
        let scratch = try capture.scratchEstimate(settings)
        _ = try RasterResources.shared.admit(scratch: scratch, own: capture.ownRetainedBytes,
                                             remedy: capture.refusalRemedy(for: settings))

        let report: LayerExportReport = try BackgroundWork.wait(
            title: "Export Layers", control: parent,
            makeTask: {
                try WorkerTask(scratchEstimate: scratch) {
                    var outcomes: [LayerExportOutcome] = []
                    var claimed = Set<String>()
                    var cancelled = false
                    for item in items {
                        if cancelled { break }
                        let span = (Double(item.ordinal - 1) / Double(items.count),
                                    Double(item.ordinal) / Double(items.count))
                        do {
                            let result = try parent.withChildOperation(from: span.0, to: span.1) {
                                child in
                                try capture.encodeLayer(index: item.index, trimmed: trimmed,
                                                        settings: settings, control: child)
                            }
                            let info = try result.info()
                            let width = (info["width"] as? NSNumber)?.intValue ?? 0
                            let height = (info["height"] as? NSNumber)?.intValue ?? 0
                            let base = try RasterExport.expandName(
                                pattern: request.pattern,
                                tokens: ExportNaming.tokens(
                                    name: baseName, width: width, height: height,
                                    format: settings.format, date: dateText, preset: presetName,
                                    layer: item.name, index: (item.ordinal, items.count)))
                            let url = folder.appendingPathComponent(base)
                                .appendingPathExtension(settings.format.fileExtension)
                            let key = url.path.lowercased()
                            guard !claimed.contains(key) else {
                                throw DoctoredCoreError(
                                    message: "\(url.lastPathComponent) was already written by this "
                                        + "export. Use {layer} or {index} in the pattern.")
                            }
                            guard !FileManager.default.fileExists(atPath: url.path) else {
                                throw DoctoredCoreError(
                                    message: "\(url.path) already exists. A layer export never "
                                        + "overwrites a file.")
                            }
                            let digest = SHA256.hash(data: result.bytes)
                                .map { String(format: "%02x", $0) }.joined()
                            try result.write(to: url, overwrite: false)
                            claimed.insert(key)
                            outcomes.append(LayerExportOutcome(item: item, url: url, refusal: nil,
                                                               info: info, sha256: digest))
                        } catch {
                            let message = (error as? DoctoredCoreError)?.message
                                ?? error.localizedDescription
                            // Only the job itself decides a run was cancelled:
                            // a per-item refusal whose text happens to mention
                            // cancelling must not abort the remaining items.
                            if parent.isCancelled { cancelled = true }
                            outcomes.append(LayerExportOutcome(item: item, url: nil,
                                                               refusal: message, info: nil,
                                                               sha256: nil))
                        }
                    }
                    return LayerExportReport(outcomes: outcomes, cancelled: cancelled)
                }
            })

        if report.written > 0 && !report.cancelled {
            ActionRecorder.shared.record(
                [ActionStep(tool: "export_layers",
                            arguments: ["items": request.items.rawValue,
                                        "trimmed": request.trimmed,
                                        "settings": settings.json,
                                        "destination": request.destination,
                                        "pattern": request.pattern,
                                        "preset": presetName],
                            note: "File ▸ Export Layers…")],
                on: self)
        }
        return report
    }
}

extension ExportCapture {
    /// Encode ONE item, at whichever depth the captured document is. The
    /// isolated render happens in the core; this side only chooses the call.
    func encodeLayer(index: Int, trimmed: Bool, settings: ExportSettings,
                     control: RasterJobControl?) throws -> RasterExportResult {
        if let typed = source.precision {
            let context = try PrecisionWorkContext(source: typed, image: nil)
            return try ExportCapture.hostFinish(
                typed.exportLayerEncoded(index: index, trimmed: trimmed,
                                         request: settings.requestJSON,
                                         workingLimit: context.limit,
                                         otherRetained: context.otherRetained,
                                         control: control),
                settings: settings)
        }
        return try ExportCapture.hostFinish(
            source.exportLayerEncoded(index: index, trimmed: trimmed,
                                      request: settings.requestJSON, control: control),
            settings: settings)
    }
}
