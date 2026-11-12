import AppKit
import CryptoKit

// The model store through the app's own `ModelStore` and `ModelDownloader`, against the
// loopback file server `verify.py` starts (`scripts/range_file_server.py`, its URL in
// DR_JOBS_PROBE_MODEL_SERVER and its request log in DR_JOBS_PROBE_MODEL_LOG). The served files
// are written by these cases under $DR_JOBS_PROBE_DIR/model-server; every store lives in a scratch
// folder of the probe's, never the user's.

/// The server's base URL, the request log and the folder it serves.
private func modelServer() throws -> (url: URL, log: URL, root: URL) {
    let environment = ProcessInfo.processInfo.environment
    guard let base = environment["DR_JOBS_PROBE_MODEL_SERVER"].flatMap(URL.init(string:)),
          let log = environment["DR_JOBS_PROBE_MODEL_LOG"] else {
        throw ProbeFailure(description: "DR_JOBS_PROBE_MODEL_SERVER and DR_JOBS_PROBE_MODEL_LOG are not set "
            + "(scripts/jobs-probe/verify.py sets them)")
    }
    return (base, URL(fileURLWithPath: log), directory.appendingPathComponent("model-server"))
}

/// The logged requests whose path starts with `/<prefix>`.
private func modelRequests(_ prefix: String) throws -> [[String: Any]] {
    let log = try modelServer().log
    let text = (try? String(contentsOf: log, encoding: .utf8)) ?? ""
    return text.split(separator: "\n").compactMap { line in
        (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any]
    }.filter { ($0["path"] as? String)?.hasPrefix("/" + prefix) == true }
}

private func sha256(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

/// Deterministic bytes.
private func modelBytes(_ count: Int, seed: UInt8) -> Data {
    Data((0..<count).map { UInt8(truncatingIfNeeded: $0 &* 31 &+ Int(seed) &* 7 &+ ($0 >> 9)) })
}

/// A catalogue entry for `files`, served under `<id>/`; `wrong` gives the first file a hash
/// that does not match its bytes; `packages` are the Core ML packages it compiles.
private func modelEntry(_ id: String, version: String = "v1", _ files: [(String, Data)],
                        wrong: Bool = false, packages: [String] = []) throws -> ModelCatalogEntry {
    let server = try modelServer()
    var catalogued: [ModelFile] = []
    for (index, (path, data)) in files.enumerated() {
        let served = server.root.appendingPathComponent(id).appendingPathComponent(path)
        try FileManager.default.createDirectory(at: served.deletingLastPathComponent(),
                                                withIntermediateDirectories: true)
        try data.write(to: served)
        let hash = wrong && index == 0 ? String(repeating: "0", count: 64) : sha256(data)
        catalogued.append(ModelFile(path: path, url: server.url.appendingPathComponent(id).appendingPathComponent(path),
                                    sha256: hash, size: Int64(data.count)))
    }
    return ModelCatalogEntry(
        id: id, name: "Probe Model \(id)", capability: "probe-\(id)", version: version, files: catalogued,
        runtime: .coreml, minimumMacOS: "15.0",
        license: ModelLicense(name: "Apache-2.0", url: URL(fileURLWithPath: "/dev/null"), tier: .clean),
        trainingData: "synthetic probe bytes", inferenceBytes: 1 << 20, packages: packages)
}

/// A store over `entries` in a fresh scratch folder.
private func scratchStore(_ name: String, _ entries: [ModelCatalogEntry]) -> ModelStore {
    let folder = directory.appendingPathComponent("model-stores").appendingPathComponent(name)
    let configuration = URLSessionConfiguration.ephemeral
    configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
    return ModelStore(directory: folder, catalog: entries, configuration: configuration)
}

private func settled(_ store: ModelStore, _ id: String) throws -> ModelState {
    try pumpUntil({
        switch store.state(id) {
        case .downloading: return false
        default: return true
        }
    }, seconds: 60)
    return store.state(id)
}

/// "model store licence gate": no request before the licence of this version is accepted; the
/// acceptance is recorded per version; then the download completes and verifies.
func verifyModelStoreLicenceGate() throws {
    let tag = "[model store licence gate]"
    let entry = try modelEntry("gate", [("pkg/a.bin", modelBytes(300_000, seed: 1)),
                                        ("pkg/b.json", Data("{\"probe\":true}".utf8))])
    let store = scratchStore("gate", [entry])
    do {
        try store.download(entry.id)
        throw ProbeFailure(description: "\(tag) a download before acceptance was not refused")
    } catch let error as ModelStoreError {
        try require(error.message == "accept the licence for \(entry.name) first",
                    "\(tag) the refusal names the licence: \(error.message)")
    }
    let early = try modelRequests("gate")
    try require(early.isEmpty, "\(tag) no request before acceptance")
    try require(store.state(entry.id) == .notDownloaded && !store.isAccepted(entry.id),
                "\(tag) not downloaded and not accepted")
    try store.accept(entry.id)
    try require(store.isAccepted(entry.id), "\(tag) accepted")
    let recorded = try JSONSerialization.jsonObject(
        with: Data(contentsOf: store.directory.appendingPathComponent("acceptances.json"))) as? [String: Any]
    try require((recorded?[entry.id] as? [String: Any])?["version"] as? String == "v1",
                "\(tag) acceptances.json records version v1: \(recorded ?? [:])")
    // The same model at another version is not accepted.
    let later = try modelEntry("gate", version: "v2", [("pkg/a.bin", modelBytes(300_000, seed: 1)),
                                                     ("pkg/b.json", Data("{\"probe\":true}".utf8))])
    try require(!ModelStore(directory: store.directory, catalog: [later]).isAccepted(entry.id),
                "\(tag) an acceptance of v1 does not accept v2")
    try store.download(entry.id)
    let state = try settled(store, entry.id)
    try require(state == .ready, "\(tag) the download ends ready: \(state)")
    let folder = store.versionDirectory(entry)
    for file in entry.files {
        let data = try Data(contentsOf: folder.appendingPathComponent(file.path))
        try require(sha256(data) == file.sha256, "\(tag) \(file.path) is the served file")
        try require(!FileManager.default.fileExists(atPath: folder.appendingPathComponent(file.path + ".partial").path),
                    "\(tag) no partial of \(file.path) is left")
    }
    try require(FileManager.default.fileExists(atPath: folder.appendingPathComponent("verified.json").path),
                "\(tag) the ready marker is written")
    try require(store.bytesOnDisk(entry.id) >= entry.totalBytes, "\(tag) the bytes on disk count the files")
    print("PASS: model store licence gate — download refuses \"accept the licence for … first\" with no request; "
        + "accept records the version in acceptances.json and does not accept another version; the download "
        + "then verifies both files, leaves no partial and writes verified.json")
}

/// "model store sha refusal": a file whose bytes do not match its catalogued SHA-256 is refused
/// by name and removed; the model never becomes ready.
func verifyModelStoreShaRefusal() throws {
    let tag = "[model store sha refusal]"
    let entry = try modelEntry("sha", [("pkg/weights.bin", modelBytes(200_000, seed: 2))], wrong: true)
    let store = scratchStore("sha", [entry])
    try store.accept(entry.id)
    try store.download(entry.id)
    let state = try settled(store, entry.id)
    try require(state == .failed("pkg/weights.bin: checksum mismatch"), "\(tag) refused by name: \(state)")
    let folder = store.versionDirectory(entry)
    for name in ["pkg/weights.bin", "pkg/weights.bin.partial", "verified.json"] {
        try require(!FileManager.default.fileExists(atPath: folder.appendingPathComponent(name).path),
                    "\(tag) \(name) is not on disk")
    }
    try require(store.compiledModelURL(entry.id) == nil, "\(tag) no compiled model is offered")
    print("PASS: model store sha refusal — a served file whose SHA-256 differs from the catalogue fails "
        + "\"pkg/weights.bin: checksum mismatch\", its partial is removed and the model is not ready")
}

/// "model store resume": a partial cut mid-file resumes with a Range request answered 206 and
/// completes; a partial already at full length is verified and renamed with no request.
func verifyModelStoreResume() throws {
    let tag = "[model store resume]"
    let data = modelBytes(1_000_000, seed: 3)
    let entry = try modelEntry("resume", [("pkg/weights.bin", data)])
    let store = scratchStore("resume", [entry])
    let folder = store.versionDirectory(entry)
    let partial = folder.appendingPathComponent("pkg/weights.bin.partial")
    try FileManager.default.createDirectory(at: partial.deletingLastPathComponent(), withIntermediateDirectories: true)
    try data.prefix(400_000).write(to: partial)
    try store.accept(entry.id)
    try store.download(entry.id)
    var state = try settled(store, entry.id)
    try require(state == .ready, "\(tag) the cut partial resumes to ready: \(state)")
    let requests = try modelRequests("resume")
    try require(requests.count == 1 && requests[0]["range"] as? String == "bytes=400000-"
                && requests[0]["status"] as? Int == 206 && requests[0]["bytes"] as? Int == 600_000,
                "\(tag) one Range request from byte 400000 answered 206 with the rest: \(requests)")
    let resumed = try Data(contentsOf: folder.appendingPathComponent("pkg/weights.bin"))
    try require(sha256(resumed) == sha256(data),
                "\(tag) the resumed file is the served file")

    let whole = modelBytes(500_000, seed: 4)
    let full = try modelEntry("fulllength", [("pkg/weights.bin", whole)])
    let second = scratchStore("fulllength", [full])
    let fullPartial = second.versionDirectory(full).appendingPathComponent("pkg/weights.bin.partial")
    try FileManager.default.createDirectory(at: fullPartial.deletingLastPathComponent(), withIntermediateDirectories: true)
    try whole.write(to: fullPartial)
    try second.accept(full.id)
    try second.download(full.id)
    state = try settled(second, full.id)
    try require(state == .ready, "\(tag) the full-length partial is verified to ready: \(state)")
    let fullRequests = try modelRequests("fulllength")
    try require(fullRequests.isEmpty, "\(tag) the full-length partial made no request")
    try require(!FileManager.default.fileExists(atPath: fullPartial.path), "\(tag) the partial was renamed")
    print("PASS: model store resume — a partial cut at 400000 of 1000000 bytes resumes with one "
        + "\"Range: bytes=400000-\" request answered 206 and verifies; a partial already at full length is "
        + "verified and renamed with no request in the server log")
}

/// "models pane": Settings ▸ Models through its own buttons on a scratch store: the licence card
/// gates the first download (declined: no request), then Download reaches ready, and Delete
/// removes the files and keeps the acceptance; the Settings card holds its three tabs.
func verifyModelsPane() throws {
    let tag = "[models pane]"
    let entry = try modelEntry("pane", [("pkg/weights.bin", modelBytes(250_000, seed: 5))])
    let store = scratchStore("pane", [entry])
    let previous = ModelStore.shared
    ModelStore.shared = store
    defer { ModelStore.shared = previous }
    let pane = ModelsSettingsPane()
    let state = try uiControl(pane.view, id: "model-state-pane", as: NSTextField.self)
    let download = try uiControl(pane.view, id: "model-download-pane", as: NSButton.self)
    let delete = try uiControl(pane.view, id: "model-delete-pane", as: NSButton.self)
    try require(state.stringValue == "Not downloaded", "\(tag) not downloaded: \(state.stringValue)")
    var asked: [String] = []
    pane.confirmLicense = { asked.append($0.id); return false }
    try uiSend(download, "\(tag) Download")
    try require(asked == ["pane"] && !store.isAccepted(entry.id) && store.state(entry.id) == .notDownloaded,
                "\(tag) a declined licence card accepts nothing and starts nothing")
    let declined = try modelRequests("pane")
    try require(declined.isEmpty, "\(tag) no request after a declined card")
    pane.confirmLicense = { asked.append($0.id); return true }
    try uiSend(download, "\(tag) Download")
    try require(store.isAccepted(entry.id), "\(tag) the accepted card records the acceptance")
    let ready = try settled(store, entry.id)
    try pumpUntil({ state.stringValue == "Ready" }, seconds: 10)
    try require(ready == .ready && !download.isEnabled && delete.isEnabled,
                "\(tag) ready, Download off, Delete on: \(ready) \(download.isEnabled) \(delete.isEnabled)")
    try require(store.bytesOnDisk(entry.id) >= entry.totalBytes, "\(tag) the bytes on disk")
    try uiSend(delete, "\(tag) Delete")
    try require(store.state(entry.id) == .notDownloaded && store.bytesOnDisk(entry.id) == 0
                && store.isAccepted(entry.id) && state.stringValue == "Not downloaded",
                "\(tag) Delete removes the files and keeps the acceptance")
    try uiSend(download, "\(tag) Download again")
    try require(asked.count == 2, "\(tag) an accepted version is not asked again: \(asked)")
    _ = try settled(store, entry.id)

    let settings = SettingsWindowController(menu: NSApp.mainMenu ?? NSMenu())
    let window = settings.makeWindow()
    try require(window.window?.title == "Settings" && settings.tabs.identifier?.rawValue == "settings-tabs"
                && settings.tabs.tabViewItems.map(\.label) == ["Shortcuts", "Models", "MCP", "Cloud"],
                "\(tag) the Settings card holds the Shortcuts, Models, MCP and Cloud tabs: "
                + "\(settings.tabs.tabViewItems.map(\.label))")
    print("PASS: models pane — Settings ▸ Models shows Not downloaded; a declined licence card accepts and "
        + "requests nothing; an accepted one records the acceptance and Download reaches Ready; Delete removes "
        + "the files and keeps the acceptance, so the next Download asks no card; the Settings card holds "
        + "the Shortcuts, Models, MCP and Cloud tabs")
}

/// "models pane ready size": a row's size line once a Core ML model is ready, and while one is
/// partial. A scratch store holds two entries laid out on disk by hand, with no download: a
/// ready one (its package's files, a compiled `.mlmodelc` folder at least as large again and
/// `verified.json`), whose line is its disk total "on disk" with no " of ", and one holding only a
/// `.partial` file, whose line stays "<partial> of <download>". The oracle is the bytes this case
/// wrote, not `bytesOnDisk`.
func verifyModelsPaneReadySize() throws {
    let tag = "[models pane ready size]"
    let package = "Probe.mlpackage"
    let packageFiles = [("\(package)/Data/weights.bin", modelBytes(180_000, seed: 7)),
                        ("\(package)/Manifest.json", Data("{\"probe\":\"ready\"}".utf8))]
    let ready = try modelEntry("readysize", packageFiles, packages: [package])
    let partial = try modelEntry("partialsize", [("pkg/weights.bin", modelBytes(300_000, seed: 8))])
    let store = scratchStore("ready-size", [ready, partial])
    var written: Int64 = 0
    func write(_ data: Data, _ url: URL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(),
                                                withIntermediateDirectories: true)
        try data.write(to: url)
        written += Int64(data.count)
    }
    let folder = store.versionDirectory(ready)
    for (path, data) in packageFiles { try write(data, folder.appendingPathComponent(path)) }
    let compiled = store.compiledLocation(ready, package: package)
    try write(modelBytes(200_000, seed: 9), compiled.appendingPathComponent("weights/weight.bin"))
    try write(modelBytes(4_000, seed: 10), compiled.appendingPathComponent("coremldata.bin"))
    let marker = try JSONSerialization.data(withJSONObject: [
        "version": ready.version, "files": ready.files.map(\.sha256),
    ])
    try write(marker, folder.appendingPathComponent("verified.json"))
    let partialData = modelBytes(120_000, seed: 8)
    try write(partialData, store.versionDirectory(partial).appendingPathComponent("pkg/weights.bin.partial"))
    let readyBytes = written - Int64(partialData.count)
    try require(store.state(ready.id) == .ready, "\(tag) the hand-made store is ready: \(store.state(ready.id))")
    try require(store.state(partial.id) == .notDownloaded,
                "\(tag) the partial entry is not downloaded: \(store.state(partial.id))")

    let previous = ModelStore.shared
    ModelStore.shared = store
    defer { ModelStore.shared = previous }
    let pane = ModelsSettingsPane()
    func size(_ bytes: Int64) -> String { ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file) }
    let readyLine = try uiControl(pane.view, id: "model-size-readysize", as: NSTextField.self).stringValue
    let partialLine = try uiControl(pane.view, id: "model-size-partialsize", as: NSTextField.self).stringValue
    print("\(tag) the ready row reads \"\(readyLine)\" (\(readyBytes) bytes written, download "
        + "\(ready.totalBytes)); the partial row reads \"\(partialLine)\"")
    var failures: [String] = []
    if readyLine != size(readyBytes) + " on disk" || readyLine.contains(" of ") {
        failures.append("the ready row reads \"\(readyLine)\", not \"\(size(readyBytes)) on disk\"")
    }
    let partialWanted = size(Int64(partialData.count)) + " of " + size(partial.totalBytes)
    if partialLine != partialWanted {
        failures.append("the partial row reads \"\(partialLine)\", not \"\(partialWanted)\"")
    }
    try require(failures.isEmpty, "\(tag) " + failures.joined(separator: "; "))
    print("PASS: models pane ready size — a ready Core ML model (package, compiled folder and marker) "
        + "reads \"\(readyLine)\", its disk total with no \" of \"; a partial download reads "
        + "\"\(partialLine)\"")
}

/// "models intro path": Settings ▸ Models' intro writes the models folder from `~` when it lies
/// under the home, so no picture of the pane shows a home or scratch path. `displayPath` is asked
/// with literal homes outside `/Users/` (a folder under it, the home itself, a trailing slash, a
/// folder outside it, a sibling whose name only starts with the home's, the root as home), and a
/// pane built over a scratch store under a scratch home must show the `~` folder and not the
/// store's full path. Every row is checked before the case fails.
func verifyModelsIntroPath() throws {
    let tag = "[models intro path]"
    let home = "/Volumes/probe/home"
    let rows: [(directory: String, home: String, expected: String)] = [
        ("/Volumes/probe/home/Library/Application Support/Doctored/Models", home,
         "~/Library/Application Support/Doctored/Models"),
        ("/Volumes/probe/home", home, "~"),
        ("/Volumes/probe/home/Library/Application Support/Doctored/Models", home + "/",
         "~/Library/Application Support/Doctored/Models"),
        ("/Volumes/other/Models", home, "/Volumes/other/Models"),
        ("/Volumes/probe/home2/Models", home, "/Volumes/probe/home2/Models"),
        ("/Volumes/probe/home/Models", "/", "/Volumes/probe/home/Models"),
    ]
    var failures: [String] = []
    for row in rows {
        let shown = ModelsSettingsPane.displayPath(URL(fileURLWithPath: row.directory, isDirectory: true),
                                                   home: row.home)
        if shown != row.expected {
            failures.append("\(row.directory) under home \(row.home) is \"\(shown)\", not \"\(row.expected)\"")
        }
    }
    let scratchHome = directory.appendingPathComponent("models-home", isDirectory: true)
    let folder = scratchHome.appendingPathComponent("Library/Application Support/Doctored/Models",
                                                    isDirectory: true)
    let previous = ModelStore.shared
    ModelStore.shared = ModelStore(directory: folder, catalog: ModelCatalog.entries)
    defer { ModelStore.shared = previous }
    let pane = ModelsSettingsPane(home: scratchHome.path)
    let intro = try uiControl(pane.view, id: "models-intro", as: NSTextField.self).stringValue
    print("\(tag) the pane's intro: \"\(intro)\"")
    if !intro.contains(" into ~/Library/Application Support/Doctored/Models.") {
        failures.append("the pane's intro names no ~/Library/Application Support/Doctored/Models")
    }
    if intro.contains(folder.path) || intro.contains(directory.path) {
        failures.append("the pane's intro shows the scratch path")
    }
    try require(failures.isEmpty, "\(tag) " + failures.joined(separator: "; "))
    print("PASS: models intro path — displayPath writes a folder under the home (with or without a "
        + "trailing slash) and the home itself from ~, and a folder outside it, a sibling sharing the "
        + "home's prefix and any folder under the root home in full; a pane over a scratch store under "
        + "a scratch home names ~/Library/Application Support/Doctored/Models and no scratch path")
}

/// "settings card height": the Settings card built over the real catalogue (a scratch store,
/// nothing downloaded) fits a laptop screen: at most 700 pt tall, the visible height of a
/// 1280 × 800 point screen less the menu bar and the Dock, and no taller than the main screen's
/// visible frame when there is one. The Models tab's rows scroll inside `models-list`, whose
/// document is taller than its viewport, and every model's Download button is still in the pane.
/// The card is built as `run()` builds it, without running it modally; the size printed is the
/// window's frame.
func verifySettingsCardHeight() throws {
    let tag = "[settings card height]"
    let entries = ModelCatalog.entries
    let store = scratchStore("settings-card", entries)
    let previous = ModelStore.shared
    ModelStore.shared = store
    defer { ModelStore.shared = previous }
    let settings = SettingsWindowController(menu: NSApp.mainMenu ?? NSMenu())
    guard let window = settings.makeWindow().window else {
        throw ProbeFailure(description: "\(tag) the card has no window")
    }
    let size = window.frame.size
    let screen = NSScreen.main?.visibleFrame.size
    print("\(tag) the Settings card is \(Int(size.width)) × \(Int(size.height)) pt over \(entries.count) "
        + "models; the main screen's visible frame is "
        + (screen.map { "\(Int($0.width)) × \(Int($0.height)) pt" } ?? "absent"))
    // Which pane sets the card's height: each tab's content at its fitting height.
    print("\(tag) the tabs' content heights: " + settings.tabs.tabViewItems.map { item in
        "\(item.label) \(Int(item.view?.subviews.first?.fittingSize.height ?? 0)) pt"
    }.joined(separator: ", "))
    var failures: [String] = []
    if size.height > 700 { failures.append("the card is \(Int(size.height)) pt tall, over 700") }
    if let screen, size.height > screen.height {
        failures.append("the card is taller than the screen's visible \(Int(screen.height)) pt")
    }
    settings.tabs.selectTabViewItem(withIdentifier: "Models")
    window.contentView?.layoutSubtreeIfNeeded()
    if window.frame.size != size {
        failures.append("selecting Models resized the card to \(window.frame.size)")
    }
    do {
        let list = try uiControl(settings.models.view, id: "models-list", as: NSScrollView.self)
        let viewport = list.contentView.bounds.height
        let document = list.documentView?.frame.height ?? 0
        print("\(tag) models-list shows \(Int(viewport)) of \(Int(document)) pt")
        if !(viewport > 0 && document > viewport) {
            failures.append("models-list does not scroll: \(Int(viewport)) of \(Int(document)) pt")
        }
    } catch {
        failures.append("\(error)")
    }
    var squeezed: [String] = []
    for entry in entries {
        do { _ = try uiControl(settings.models.view, id: "model-download-\(entry.id)", as: NSButton.self) }
        catch { failures.append("\(entry.id): \(error)") }
        // Each row names its model: the name line keeps its own height inside the scrolled list
        // rather than being squeezed to fit the list's viewport.
        do {
            let name = try uiControl(settings.models.view, id: "model-name-\(entry.id)", as: NSTextField.self)
            let wanted = name.intrinsicContentSize.height
            if !(wanted > 0 && name.frame.height + 0.5 >= wanted) {
                squeezed.append("\(entry.id) \(Int(name.frame.height)) of \(Int(wanted)) pt")
            }
        } catch {
            failures.append("\(entry.id): \(error)")
        }
    }
    if !squeezed.isEmpty {
        failures.append("\(squeezed.count) model names are squeezed: " + squeezed.joined(separator: ", "))
    }
    try require(failures.isEmpty, "\(tag) " + failures.joined(separator: "; "))
    print("PASS: settings card height — over the \(entries.count)-model catalogue the Settings card is "
        + "\(Int(size.height)) pt tall (at most 700 and within the screen), the model rows scroll inside "
        + "models-list, every Download button is in the pane and every model's name keeps its height")
}

/// "model catalog release urls" (the Doctored rename): every file and licence URL names no
/// old release repo (the legacy prefix's name + "-models", spelled so a repo grep for the old
/// name finds nothing); the 40 release assets are on kpgalligan/doctored-models and belong to the
/// six first-party `doctored/…` entries; no id keeps the legacy prefix.
func verifyModelCatalogReleaseURLs() throws {
    let tag = "[model catalog release urls]"
    let release = "https://github.com/kpgalligan/doctored-models/releases/download/"
    let oldRepo = String(ModelCatalog.legacyFirstPartyPrefix.dropLast()) + "-models"
    let entries = ModelCatalog.entries
    for entry in entries {
        for url in entry.files.map(\.url.absoluteString) + [entry.license.url.absoluteString] {
            try require(!url.contains(oldRepo), "\(tag) \(entry.id) names \(oldRepo): \(url)")
        }
        if entry.files.contains(where: { $0.url.absoluteString.hasPrefix(release) }) {
            try require(entry.id.hasPrefix(ModelCatalog.firstPartyPrefix),
                        "\(tag) \(entry.id) is served by the release, so it is first-party")
        }
        try require(!entry.id.hasPrefix(ModelCatalog.legacyFirstPartyPrefix), "\(tag) a legacy id: \(entry.id)")
    }
    let released = entries.flatMap(\.files).filter { $0.url.absoluteString.hasPrefix(release) }
    try require(released.count == 40, "\(tag) 40 file URLs on the doctored-models release: \(released.count)")
    let firstParty = Set(entries.map(\.id).filter { $0.hasPrefix(ModelCatalog.firstPartyPrefix) })
    let expected: Set<String> = [
        "doctored/mediapipe-selfie-multiclass-256x256", "doctored/sky-library", "doctored/migan-512-places2",
        "doctored/realesrgan-x4plus", "doctored/fbcnn-color", "doctored/ddcolor",
    ]
    try require(firstParty == expected, "\(tag) the first-party ids: \(firstParty.sorted())")
    try require(ModelCatalog.legacyID(of: "doctored/ddcolor") == "rasterize/ddcolor",
                "\(tag) doctored/ddcolor's legacy id is rasterize/ddcolor")
    try require(ModelCatalog.legacyID(of: "apple/coreml-sam2.1-tiny") == nil, "\(tag) an Apple id has no legacy id")
    print("PASS: model catalog release urls — no catalogue URL names \(oldRepo); all 40 release assets "
        + "are on kpgalligan/doctored-models and belong to the six doctored/… entries; no id keeps "
        + "rasterize/, and legacyID maps doctored/ddcolor to rasterize/ddcolor and an Apple id to nil "
        + "(the Doctored rename)")
}

/// Every file under `root`, as sorted relative paths.
private func storeListing(_ root: URL) -> [String] {
    let walker = FileManager.default.enumerator(atPath: root.path)
    var paths: [String] = []
    while let path = walker?.nextObject() as? String { paths.append(path) }
    return paths.sorted()
}

/// "model store legacy id" (the Doctored rename): a store the old app filled under
/// `rasterize/legacy-probe` is ready and accepted under `doctored/legacy-probe`, read in place with
/// nothing moved or written; Delete removes the legacy folder and keeps the acceptance, and the
/// next download goes to `doctored--legacy-probe`.
func verifyModelStoreLegacyID() throws {
    let tag = "[model store legacy id]"
    let files = [("pkg/weights.bin", modelBytes(300_000, seed: 6)),
                 ("pkg/config.json", Data("{\"legacy\":true}".utf8))]
    // The old app's state: accepted and downloaded under the legacy id.
    let legacy = try modelEntry("rasterize/legacy-probe", files)
    let old = scratchStore("legacy", [legacy])
    try old.accept(legacy.id)
    try old.download(legacy.id)
    let oldState = try settled(old, legacy.id)
    try require(oldState == .ready, "\(tag) the legacy install is ready: \(oldState)")
    let root = old.directory
    let legacyFolder = root.appendingPathComponent("rasterize--legacy-probe", isDirectory: true)
    let newFolder = root.appendingPathComponent("doctored--legacy-probe", isDirectory: true)
    let acceptancesURL = root.appendingPathComponent("acceptances.json")
    let listingBefore = storeListing(root)
    let acceptancesBefore = try Data(contentsOf: acceptancesURL)
    try require(listingBefore.contains("rasterize--legacy-probe/v1/verified.json"),
                "\(tag) the legacy folder holds the ready marker: \(listingBefore)")

    // The renamed app over the same folder.
    let entry = try modelEntry("doctored/legacy-probe", files)
    let store = scratchStore("legacy", [entry])
    try require(store.isAccepted(entry.id), "\(tag) the legacy acceptance accepts the new id")
    try require(store.state(entry.id) == .ready, "\(tag) ready in place: \(store.state(entry.id))")
    let ready = store.readyDirectory(entry.id)?.path
    try require(ready == legacyFolder.appendingPathComponent("v1", isDirectory: true).path,
                "\(tag) the ready folder is rasterize--legacy-probe/v1: \(ready ?? "nil")")
    try require(store.bytesOnDisk(entry.id) >= entry.totalBytes,
                "\(tag) the bytes on disk count the legacy files: \(store.bytesOnDisk(entry.id))")
    let listingAfter = storeListing(root)
    let acceptancesAfter = try Data(contentsOf: acceptancesURL)
    try require(listingAfter == listingBefore, "\(tag) no file moved or was written: \(listingAfter)")
    try require(acceptancesAfter == acceptancesBefore, "\(tag) acceptances.json is unchanged")
    try require(!FileManager.default.fileExists(atPath: newFolder.path), "\(tag) no doctored--legacy-probe")

    try store.delete(entry.id)
    try require(!FileManager.default.fileExists(atPath: legacyFolder.path), "\(tag) Delete removes the legacy folder")
    try require(store.isAccepted(entry.id) && store.state(entry.id) == .notDownloaded,
                "\(tag) Delete keeps the acceptance: \(store.state(entry.id))")
    try require(store.modelDirectory(entry.id).lastPathComponent == "doctored--legacy-probe",
                "\(tag) the folder is now the new id's: \(store.modelDirectory(entry.id).path)")
    try store.download(entry.id)
    let state = try settled(store, entry.id)
    let fresh = store.readyDirectory(entry.id)?.path
    try require(state == .ready && fresh == newFolder.appendingPathComponent("v1", isDirectory: true).path,
                "\(tag) a new download is ready under doctored--legacy-probe/v1: \(state) \(fresh ?? "nil")")
    try require(!FileManager.default.fileExists(atPath: legacyFolder.path), "\(tag) no legacy folder returns")
    print("PASS: model store legacy id — a store accepted and filled under rasterize/legacy-probe is accepted "
        + "and ready under doctored/legacy-probe from rasterize--legacy-probe/v1, its bytes counted, with the "
        + "file list and acceptances.json unchanged and no doctored-- folder; Delete removes the legacy folder "
        + "and keeps the acceptance, and the next download is ready under doctored--legacy-probe/v1 "
        + "(the Doctored rename)")
}
