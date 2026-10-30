import Foundation

// The local model store (`designs/neural-models.md` architecture decision 3). On disk:
//
//   <directory>/acceptances.json                         {id: {version, accepted_at}}
//   <directory>/<id folder>/<version>/<path>             a verified file
//   <directory>/<id folder>/<version>/<path>.partial     an unverified download, never used
//   <directory>/<id folder>/<version>/compiled/<stem>.mlmodelc
//   <directory>/<id folder>/<version>/verified.json      the ready marker, written last
//
// The id folder is the catalogue id with "/" spelled "--", so one model is one folder and Delete
// removes exactly it. The directory is `DR_MODELS_DIR` when set (read once, by `shared`), else
// ~/Library/Application Support/Doctored/Models. Tests, probes and drives never use the user's.
//
// First-party ids were `rasterize/<name>` before the Doctored rename
// (`ModelCatalog.legacyID(of:)`). A store's `rasterize--<name>` folder and `rasterize/<name>` acceptance are read in place under
// the new id, never moved or rewritten: `modelDirectory` resolves the new folder when it exists,
// else the legacy one when it exists, else the new one, so a ready legacy install stays ready and
// a legacy partial resumes where it is; a new download goes to `doctored--<name>`.

enum ModelState: Equatable {
    case notDownloaded
    case downloading(fraction: Double)
    case ready
    case failed(String)

    /// The state as `list_models` and the Models pane spell it.
    var name: String {
        switch self {
        case .notDownloaded: return "not_downloaded"
        case .downloading: return "downloading"
        case .ready: return "ready"
        case .failed: return "failed"
        }
    }
}

struct ModelStoreError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// The catalogue's models on disk: which are ready, which are downloading, which licences the
/// user accepted. Main thread; downloads run on `ModelDownloader`'s delegate queue and report
/// back to main, posting `didChange`.
final class ModelStore {
    /// The app's store. Replaced only by probes, which point it at a scratch folder.
    static var shared = ModelStore(directory: defaultDirectory(), catalog: ModelCatalog.entries)
    static let didChange = Notification.Name("DoctoredModelStoreDidChange")
    static let directoryEnvironment = "DR_MODELS_DIR"

    let directory: URL
    let catalog: [ModelCatalogEntry]
    private let configuration: URLSessionConfiguration
    private var progress: [String: Double] = [:]
    private var failures: [String: String] = [:]
    private lazy var downloader = ModelDownloader(configuration: configuration) { [weak self] event in
        self?.handle(event)
    }

    init(directory: URL, catalog: [ModelCatalogEntry],
         configuration: URLSessionConfiguration = .default) {
        self.directory = directory
        self.catalog = catalog
        self.configuration = configuration
    }

    /// `DR_MODELS_DIR`, else the Application Support folder.
    private static func defaultDirectory() -> URL {
        if let value = ProcessInfo.processInfo.environment[directoryEnvironment], !value.isEmpty {
            return URL(fileURLWithPath: (value as NSString).expandingTildeInPath, isDirectory: true)
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        return support.appendingPathComponent("Doctored/Models", isDirectory: true)
    }

    func entry(_ id: String) -> ModelCatalogEntry? { catalog.first { $0.id == id } }

    /// The id's folder (module comment): the new id's folder when it exists, else the legacy id's
    /// when that exists, else the new id's.
    func modelDirectory(_ id: String) -> URL {
        let current = folder(for: id)
        if FileManager.default.fileExists(atPath: current.path) { return current }
        if let legacyID = ModelCatalog.legacyID(of: id) {
            let legacy = folder(for: legacyID)
            if FileManager.default.fileExists(atPath: legacy.path) { return legacy }
        }
        return current
    }

    /// The new id's folder, then the legacy id's folder when the id has one (module comment).
    private func storeFolders(_ id: String) -> [URL] {
        var folders = [folder(for: id)]
        if let legacy = ModelCatalog.legacyID(of: id) { folders.append(folder(for: legacy)) }
        return folders
    }

    /// The catalogue id with "/" spelled "--" (module comment).
    private func folder(for id: String) -> URL {
        directory.appendingPathComponent(id.replacingOccurrences(of: "/", with: "--"), isDirectory: true)
    }

    func versionDirectory(_ entry: ModelCatalogEntry) -> URL {
        modelDirectory(entry.id).appendingPathComponent(entry.version, isDirectory: true)
    }

    private var acceptancesURL: URL { directory.appendingPathComponent("acceptances.json") }

    // MARK: - State

    func state(_ id: String) -> ModelState {
        if let fraction = progress[id] { return .downloading(fraction: fraction) }
        if isReady(id) { return .ready }
        if let message = failures[id] { return .failed(message) }
        return .notDownloaded
    }

    /// Ready: the version's marker names this version and every catalogued file, and every
    /// compiled package is on disk.
    private func isReady(_ id: String) -> Bool {
        guard let entry = entry(id) else { return false }
        let marker = versionDirectory(entry).appendingPathComponent("verified.json")
        guard let data = try? Data(contentsOf: marker),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              object["version"] as? String == entry.version,
              (object["files"] as? [String]) == entry.files.map(\.sha256) else { return false }
        return entry.packages.allSatisfy {
            FileManager.default.fileExists(atPath: compiledLocation(entry, package: $0).path)
        }
    }

    /// `compiled/<stem>.mlmodelc` in the version folder, the downloader's destination for
    /// `package`.
    func compiledLocation(_ entry: ModelCatalogEntry, package: String) -> URL {
        let stem = (package as NSString).deletingPathExtension
        return versionDirectory(entry).appendingPathComponent("compiled", isDirectory: true)
            .appendingPathComponent(stem + ".mlmodelc", isDirectory: true)
    }

    /// Only a ready model's compiled `package` (nil: its first); nil for every other state, for
    /// a package the entry does not list and for an entry with nothing to compile.
    func compiledModelURL(_ id: String, package: String? = nil) -> URL? {
        guard let entry = entry(id), isReady(id),
              let chosen = package ?? entry.packages.first, entry.packages.contains(chosen) else { return nil }
        return compiledLocation(entry, package: chosen)
    }

    /// The version folder of a ready entry, nil for every other state: where an `assets` entry's
    /// files are read from (the sky library's manifest and JPEGs).
    func readyDirectory(_ id: String) -> URL? {
        guard let entry = entry(id), isReady(id) else { return nil }
        return versionDirectory(entry)
    }

    /// Every byte under the model's folders (the new id's and the legacy id's), every version and
    /// partial included.
    func bytesOnDisk(_ id: String) -> Int64 {
        var total: Int64 = 0
        for root in storeFolders(id) {
            guard let walker = FileManager.default.enumerator(
                at: root, includingPropertiesForKeys: [.fileSizeKey, .isRegularFileKey]) else { continue }
            for case let url as URL in walker {
                let values = try? url.resourceValues(forKeys: [.fileSizeKey, .isRegularFileKey])
                if values?.isRegularFile == true { total += Int64(values?.fileSize ?? 0) }
            }
        }
        return total
    }

    // MARK: - Licence acceptance

    private func acceptances() -> [String: [String: Any]] {
        guard let data = try? Data(contentsOf: acceptancesURL),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: [String: Any]]
        else { return [:] }
        return object
    }

    /// Whether the user accepted this entry's licence for its catalogued version, under its id or
    /// under its legacy id (module comment).
    func isAccepted(_ id: String) -> Bool {
        guard let entry = entry(id) else { return false }
        let all = acceptances()
        if all[id]?["version"] as? String == entry.version { return true }
        guard let legacy = ModelCatalog.legacyID(of: id) else { return false }
        return all[legacy]?["version"] as? String == entry.version
    }

    /// Records the acceptance of the entry's licence for its version.
    func accept(_ id: String) throws {
        guard let entry = entry(id) else { throw ModelStoreError(message: "no catalogued model \(id)") }
        var all = acceptances()
        all[id] = ["version": entry.version, "accepted_at": ISO8601DateFormatter().string(from: Date())]
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            let data = try JSONSerialization.data(withJSONObject: all, options: [.prettyPrinted, .sortedKeys])
            try data.write(to: acceptancesURL, options: .atomic)
        } catch {
            throw ModelStoreError(message: "could not record the licence acceptance: \(error.localizedDescription)")
        }
        NotificationCenter.default.post(name: Self.didChange, object: self)
    }

    // MARK: - Download, cancel, delete

    /// Starts (or resumes) the entry's download. Refuses before any request when the licence
    /// for this version is not accepted. A ready or downloading model does nothing.
    func download(_ id: String) throws {
        guard let entry = entry(id) else { throw ModelStoreError(message: "no catalogued model \(id)") }
        guard isAccepted(id) else { throw ModelStoreError(message: "accept the licence for \(entry.name) first") }
        guard progress[id] == nil, !isReady(id) else { return }
        failures[id] = nil
        progress[id] = 0
        downloader.start(entry, into: versionDirectory(entry))
        NotificationCenter.default.post(name: Self.didChange, object: self)
    }

    /// Stops a download; its partial file stays for the next resume.
    func cancel(_ id: String) {
        guard progress[id] != nil else { return }
        downloader.cancel(id)
        progress[id] = nil
        NotificationCenter.default.post(name: Self.didChange, object: self)
    }

    /// Removes the model's folders, the new id's and the legacy id's (every version and partial).
    /// The acceptance is kept.
    func delete(_ id: String) throws {
        cancel(id)
        failures[id] = nil
        for root in storeFolders(id) where FileManager.default.fileExists(atPath: root.path) {
            do { try FileManager.default.removeItem(at: root) } catch {
                throw ModelStoreError(message: "could not delete \(root.path): \(error.localizedDescription)")
            }
        }
        NotificationCenter.default.post(name: Self.didChange, object: self)
    }

    private func handle(_ event: ModelDownloadEvent) {
        switch event {
        case .progress(let id, let fraction):
            guard progress[id] != nil else { return }
            progress[id] = fraction
        case .finished(let id):
            guard progress[id] != nil else { return }
            progress[id] = nil
        case .failed(let id, let message):
            guard progress[id] != nil else { return }
            progress[id] = nil
            failures[id] = message
        }
        NotificationCenter.default.post(name: Self.didChange, object: self)
    }
}
