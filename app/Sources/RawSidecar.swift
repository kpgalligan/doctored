import Foundation

// The `<basename>.xmp` Adobe Camera Raw sidecar beside a camera file, read when that file is
// opened (`ImageDocument.openRetainedRaw`) or placed (`RawSourcePlacement`). Photoshop honours the
// sidecar's exposure (`float-raw-602` takes 1–2); its other settings have no measured mapping onto
// this build's recipe, so they are named in a disclosure and not applied.

/// The camera file's `.xmp` sidecar: the one setting it applies and what it
/// tells the user about the rest. Reading it never fails a place or an open.
struct RawSidecar {
    /// A sidecar is a few KiB of XML; anything past this is not one.
    static let maximumBytes = 1 << 20
    static let namespace = "http://ns.adobe.com/camera-raw-settings/1.0/"
    /// Bookkeeping properties that describe the sidecar, not the develop.
    static let bookkeeping: Set<String> = ["Version", "ProcessVersion", "CompatibleVersion",
                                           "HasSettings", "HasCrop", "AlreadyApplied", "RawFileName"]

    let url: URL
    /// Only `exposure` is ever set.
    let settings: RawDevelopSettings
    let disclosures: [String]

    /// nil when the camera file has no `<basename>.xmp` beside it.
    static func read(beside camera: URL) -> RawSidecar? {
        let url = camera.deletingPathExtension().appendingPathExtension("xmp")
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        let ignored = RawSidecar(url: url, settings: RawDevelopSettings(), disclosures: [
            "The Adobe Camera Raw sidecar \(url.lastPathComponent) could not be read as XMP of at most 1 MiB and was ignored."])
        guard let info = try? url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey]),
              info.isRegularFile == true, let size = info.fileSize, size <= maximumBytes,
              let data = try? Data(contentsOf: url, options: .uncached), data.count <= maximumBytes else {
            return ignored
        }
        let reader = Reader()
        let parser = XMLParser(data: data)
        parser.shouldProcessNamespaces = true
        parser.shouldReportNamespacePrefixes = true
        parser.shouldResolveExternalEntities = false
        parser.delegate = reader
        guard parser.parse() else { return ignored }
        var settings = RawDevelopSettings()
        var disclosures: [String] = []
        var skipped: [String] = []
        for (name, value) in reader.properties where !bookkeeping.contains(name) {
            guard name == "Exposure2012" else {
                if !skipped.contains(name) { skipped.append(name) }
                continue
            }
            let text = value.trimmingCharacters(in: .whitespacesAndNewlines)
            guard let stops = Double(text), stops.isFinite else {
                disclosures.append("The Adobe Camera Raw sidecar's exposure \"\(text)\" is not a number and was not applied.")
                continue
            }
            let clamped = RawDevelopRange.exposure.clamping(stops, fallback: 0)
            if clamped != stops {
                disclosures.append(String(format: "The Adobe Camera Raw sidecar's exposure %+.2f was limited to %+.2f, this build's exposure range.",
                                          stops, clamped))
            }
            settings.exposure = clamped
        }
        if !skipped.isEmpty {
            var names = skipped.prefix(8).joined(separator: ", ")
            if skipped.count > 8 { names += " and \(skipped.count - 8) more" }
            disclosures.append("The Adobe Camera Raw sidecar's exposure is applied; its other settings "
                + "are not: \(names).")
        }
        return RawSidecar(url: url, settings: settings, disclosures: disclosures)
    }

    /// Collects every `crs:` property in document order, from `rdf:Description`
    /// attributes and from simple child elements; a property nested inside
    /// another (a structured value's own fields) is not a property of its own.
    private final class Reader: NSObject, XMLParserDelegate {
        var properties: [(String, String)] = []
        private var prefixes: [String: [String]] = [:]
        private var open: (name: String, depth: Int, text: String)?
        private var depth = 0

        func parser(_ parser: XMLParser, didStartMappingPrefix prefix: String, toURI namespaceURI: String) {
            prefixes[prefix, default: []].append(namespaceURI)
        }

        func parser(_ parser: XMLParser, didEndMappingPrefix prefix: String) {
            _ = prefixes[prefix]?.popLast()
        }

        func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?,
                    qualifiedName qName: String?, attributes attributeDict: [String: String] = [:]) {
            depth += 1
            guard open == nil else { return }
            if namespaceURI == RawSidecar.namespace {
                open = (elementName, depth, "")
                return
            }
            for (key, value) in attributeDict.sorted(by: { $0.key < $1.key }) {
                let parts = key.split(separator: ":", maxSplits: 1).map(String.init)
                guard parts.count == 2, prefixes[parts[0]]?.last == RawSidecar.namespace else { continue }
                properties.append((parts[1], value))
            }
        }

        func parser(_ parser: XMLParser, foundCharacters string: String) {
            open?.text += string
        }

        func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?,
                    qualifiedName qName: String?) {
            if let current = open, current.depth == depth {
                properties.append((current.name, current.text))
                open = nil
            }
            depth -= 1
        }
    }
}

extension RawDevelopSettings {
    /// `patch` over this request: each setting `patch` names wins, every other
    /// keeps this request's value (a sidecar's exposure under the dialog's or
    /// an agent's own settings).
    func overlaid(by patch: RawDevelopSettings) -> RawDevelopSettings {
        var out = self
        if let value = patch.baselineCompensation { out.baselineCompensation = value }
        if let value = patch.moire { out.moire = value }
        if let value = patch.vignette { out.vignette = value }
        if let value = patch.grain { out.grain = value }
        if let value = patch.texture { out.texture = value }
        if let value = patch.clarity { out.clarity = value }
        if let value = patch.dehaze { out.dehaze = value }
        if let value = patch.sourceGeometry { out.sourceGeometry = value }
        if let value = patch.outputSpace { out.outputSpace = value }
        if let value = patch.outputEncoding { out.outputEncoding = value }
        if let value = patch.exposure { out.exposure = value }
        if let value = patch.temperature { out.temperature = value }
        if let value = patch.tint { out.tint = value }
        if let value = patch.asShotWhiteBalance { out.asShotWhiteBalance = value }
        if let value = patch.toneCurve { out.toneCurve = value }
        if let value = patch.shadows { out.shadows = value }
        if let value = patch.contrast { out.contrast = value }
        if let value = patch.sharpness { out.sharpness = value }
        if let value = patch.detail { out.detail = value }
        if let value = patch.luminanceNoise { out.luminanceNoise = value }
        if let value = patch.colorNoise { out.colorNoise = value }
        if let value = patch.lensCorrection { out.lensCorrection = value }
        if let value = patch.highlightRecovery { out.highlightRecovery = value }
        return out
    }
}
