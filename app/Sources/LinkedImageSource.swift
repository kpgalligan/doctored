import Foundation

/// Shared UI/MCP/Actions placement. A camera RAW file lands a retained RAW
/// layer (`RawSourcePlacement`); every other file lands an ordinary raster
/// layer at the document's depth.
enum LinkedImageWork {
    /// The history name of a place, as the Layer menu names the command.
    static func placeTitle(_ storage: ImageSourceStorage) -> String {
        storage == .linked ? "Place Linked" : "Place Embedded"
    }

    /// `insertion` names WHERE the new layer lands (12E-2b); nil means the UI's
    /// own decision, `.above` the active entry. Returns the import's
    /// disclosure: the core open's profile refusal for the imported file,
    /// or a host source's joined disclosures; nil when nothing
    /// was imported or there is nothing to disclose. `storage` matters only to
    /// a camera RAW file, whose layer keeps the link.
    @discardableResult
    static func importImage(in document: ImageDocument, url: URL, name: String,
                            x: Int = 0, y: Int = 0,
                            storage: ImageSourceStorage,
                            insertion: LayerInsertion? = nil,
                            raw: RawImportMode = .headless(RawDevelopSettings()),
                            record: [ActionStep] = .notACommand) throws -> String? {
        if HostSourceInput.claims(url) {
            let notes = try HostSourceWork.place(in: document, url: url, name: name, x: x, y: y,
                storage: storage, insertion: insertion, record: record)
            return notes.isEmpty ? nil : notes.joined(separator: " ")
        }
        // A camera file lands a retained RAW layer (`RawSourcePlacement`), which a Grayscale
        // or Lab Color document does not hold (#1: `GrayDocument.rawTools`, `LabDocument.rawTools`).
        if RawSourcePlacement.claims(url), document.doc?.isGray == true {
            throw DoctoredCoreError(message: GrayDocument.rawRefusal(tool: "Placing a camera RAW file"))
        }
        if RawSourcePlacement.claims(url), document.doc?.isLab == true {
            throw DoctoredCoreError(message: LabDocument.rawRefusal(tool: "Placing a camera RAW file"))
        }
        if RawSourcePlacement.claims(url) {
            return try RawSourcePlacement.importRaw(in: document, url: url, name: name,
                x: x, y: y, storage: storage, raw: raw, insertion: insertion, record: record)
        }
        let capture = try ImageSourceCapture(document)
        let file = try ImageSourceFile(url: url)
        _ = try RasterResources.shared.admit(scratch: file.scratchEstimate)
        let control = try RasterJobControl()
        let decoded = try BackgroundWork.wait(try WorkerTask(scratchEstimate: file.scratchEstimate) {
            try file.decode(control: control)
        }, title: "Reading Image", control: control)
        try capture.requireCurrent(document)
        // The slot the new layer lands at — the CORE's rule, carried in the
        // request as the `anchor`/`position` pair and read back here for the
        // rasterization and the "select what I just made" answer.
        let placement = insertion ?? (document.activeLayerIndex, LayerInsertPosition.above)
        let landing = placement.position.landingIndex(in: capture.source, anchor: placement.anchor)
        let id = UUID().uuidString
        // The core's place renders the file into the document's profile and
        // depth (`convert_fallback`); the rasterization then drops the binding
        // and keeps exactly those pixels.
        let place = try PrecisionPixelCommand(["op": "place", "source_id": id, "revision_id": id,
            "depth_conversion": ImageSourceDepthConversion.convertFallback.rawValue,
            "name": name, "x": x, "y": y,
            "anchor": placement.anchor, "position": placement.position.rawValue])
        let rasterize = try PrecisionPixelCommand(["op": "rasterize", "layer": landing])
        guard try RetainedImageWork.edit(in: document, source: capture.source,
            steps: [(asset: decoded.asset, request: place), (asset: nil, request: rasterize)],
            title: placeTitle(storage), record: record, landing: landing) else { return nil }
        return decoded.profileRefusal
    }

    /// Layer `layer`'s retained source report with its link's current status
    /// (`embedded`, `current`, `changed`, `missing` or `unreadable`); nil for
    /// a layer with no retained source.
    static func info(_ source: RasterDocument, layer: Int) throws -> [String: Any]? {
        guard var info = try source.retainedSourceInfo(layer) else { return nil }
        guard let link = info["link"] as? [String: Any], let path = link["path"] as? String,
              let expected = link["sha256"] as? String else {
            info["link_status"] = "embedded"
            return info
        }
        let scratch: UInt64 = 2 * 1024 * 1024
        _ = try RasterResources.shared.admit(scratch: scratch)
        // Only file I/O failures describe the link. Admission/busy failures and a cancel
        // propagate instead of incorrectly labeling a readable file unreadable.
        let control = try RasterJobControl()
        let status = try BackgroundWork.wait(try WorkerTask(scratchEstimate: scratch) {
            do {
                let hash = try ImageSourceFile.fingerprint(URL(fileURLWithPath: path), control: control)
                return ["link_status": hash == expected ? "current" : "changed"]
            } catch let cancel as JobCancelled {
                throw cancel
            } catch {
                let failure = error as NSError
                let missing = failure.domain == NSCocoaErrorDomain
                    && [NSFileNoSuchFileError, NSFileReadNoSuchFileError].contains(failure.code)
                return ["link_status": missing ? "missing" : "unreadable",
                        "link_error": error.localizedDescription]
            }
        }, title: "Checking Linked Image", control: control)
        info.merge(status) { _, new in new }
        info["cached_original_available"] = true
        return info
    }

    static func requireSource(_ source: RasterDocument, layer: Int, editing: Bool = false) throws -> [String: Any] {
        guard let info = try source.retainedSourceInfo(layer) else {
            throw DoctoredCoreError(message: "Choose a layer with a retained image source.")
        }
        if editing, !source.lockBlockingFlags(layer, kind: DR_EDIT_PIXELS).isEmpty {
            throw DoctoredCoreError(message: "The image source layer is locked.")
        }
        return info
    }
}
