import AppKit
import CryptoKit

// Edit ▸ Sky Replacement's skies: the downloaded library — the model store's `assets` entry
// `doctored/sky-library` (capability `sky_library`: CC0 sky crops of Poly Haven HDRIs and their
// `manifest.json`, read from `ModelStore.readyDirectory`) — and the user's own skies, a bounded
// `PreferenceLibrary` (`DR_SKIES_DIR`) whose entries name a copy of the picked file under
// `<library>/Images/<sha256>.<ext>`. Library writes are PREFERENCES: no undo step, never an Action
// step. The sheet (`SkyReplacementSheet.swift`), `sky_replacement` / `list_skies` over MCP and a
// replayed Action all resolve a sky here.

/// One library sky, as `manifest.json` lists it.
struct SkyPreset: Equatable {
    let id: String
    let name: String
    /// blue, sunset, dramatic or night.
    let category: String
    let file: String
    let sourceURL: URL
    let license: String
}

/// One of the user's own skies: its name (the picked file's base name) and its image's file name
/// under `<DR_SKIES_DIR>/Images`.
struct UserSky: Equatable {
    static let schemaVersion = 1
    let name: String
    let image: String
    let width: Int
    let height: Int
}

enum UserSkyCodec: PreferenceLibraryCodec {
    typealias Item = UserSky

    static let noun = "sky"
    static let plural = "skies"
    static let directoryEnvironment = "DR_SKIES_DIR"
    static let directoryName = "Doctored/Skies"
    static let maximumEntries = 64
    static let maximumFileBytes = 4096
    static let schemaVersions = UserSky.schemaVersion...UserSky.schemaVersion
    static let didChange = Notification.Name("UserSkyLibraryDidChange")

    static func name(of item: UserSky) -> String { item.name }
    static func json(of item: UserSky) -> [String: Any] {
        ["schema_version": UserSky.schemaVersion, "name": item.name, "image": item.image,
         "width": item.width, "height": item.height]
    }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> UserSky {
        let keys: Set<String> = ["schema_version", "name", "image", "width", "height"]
        guard Set(object.keys) == keys else {
            let list = keys.sorted().joined(separator: ", ")
            throw DoctoredCoreError(message: "A sky entry holds exactly \(list).")
        }
        guard let name = object["name"] as? String, !name.isEmpty,
              let image = object["image"] as? String, SkyLibrary.isImageFileName(image),
              let width = (object["width"] as? NSNumber)?.intValue, width > 0,
              let height = (object["height"] as? NSNumber)?.intValue, height > 0 else {
            throw DoctoredCoreError(message: "A sky entry needs a name, an image file name and its size.")
        }
        return UserSky(name: name, image: image, width: width, height: height)
    }
}

/// Where a sky comes from: the library, the user's own skies, or a file named directly (MCP's
/// `sky_path`).
enum SkyChoice: Equatable {
    case library(SkyPreset)
    case user(UserSky)
    case file(URL)
}

enum SkyLibrary {
    static let capability = "sky_library"
    static let notInstalled = "the sky library is not installed; download it in Settings ▸ Models"
    /// The library's categories, in the popup's order.
    static let categories = ["blue", "sunset", "dramatic", "night"]
    /// The largest file a user sky may be copied from.
    static let maximumImageBytes = 64 * 1024 * 1024
    private typealias Library = PreferenceLibrary<UserSkyCodec>

    /// The catalogue's sky library entry.
    static var entry: ModelCatalogEntry? { ModelCatalog.entry(capability: capability) }

    /// The ready library's folder, nil while it is not downloaded.
    static var readyDirectory: URL? { entry.flatMap { ModelStore.shared.readyDirectory($0.id) } }

    /// The ready library's skies, in manifest order; empty while it is not downloaded (or its
    /// manifest does not read, which the store's SHA-256 check makes a defect).
    static func presets() -> [SkyPreset] {
        guard let directory = readyDirectory else { return [] }
        return (try? presets(in: directory)) ?? []
    }

    /// The skies `manifest.json` in `directory` lists: version 1, each with an id, a name, a
    /// known category, a bare file name, a source URL and a licence.
    static func presets(in directory: URL) throws -> [SkyPreset] {
        let url = directory.appendingPathComponent("manifest.json")
        guard let data = try? Data(contentsOf: url),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              (object["version"] as? NSNumber)?.intValue == 1,
              let rows = object["skies"] as? [[String: Any]]
        else { throw DoctoredCoreError(message: "The sky library's manifest could not be read.") }
        return try rows.map { row in
            guard let id = row["id"] as? String, !id.isEmpty, let name = row["name"] as? String,
                  let category = row["category"] as? String, categories.contains(category),
                  let file = row["file"] as? String, !file.isEmpty, !file.contains("/"), file != "..",
                  let source = (row["source_url"] as? String).flatMap(URL.init(string:)),
                  let license = row["license"] as? String else {
                throw DoctoredCoreError(message: "The sky library's manifest has an unreadable sky.")
            }
            return SkyPreset(id: id, name: name, category: category, file: file, sourceURL: source,
                             license: license)
        }
    }

    /// The user's own skies, in the library's order.
    static func userSkies() throws -> [UserSky] { try Library.entries() }

    /// The folder the user skies' image copies live in.
    static var imagesDirectory: URL {
        Library.directory.appendingPathComponent("Images", isDirectory: true)
    }

    /// A content-addressed image file name: 64 hex digits, a dot and a short extension.
    static func isImageFileName(_ name: String) -> Bool {
        let parts = name.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 2, parts[0].count == 64, (1...8).contains(parts[1].count) else { return false }
        return parts[0].allSatisfy { $0.isHexDigit && !$0.isUppercase }
            && parts[1].allSatisfy { $0.isLetter || $0.isNumber }
    }

    /// Copies the image at `url` into the library (at most 64 MiB, and it must open) and saves an
    /// entry named after the file's base name. Refuses a name already in use.
    static func addUserSky(from url: URL) throws -> UserSky {
        let attributes = try? FileManager.default.attributesOfItem(atPath: url.path)
        guard let size = (attributes?[.size] as? NSNumber)?.intValue else {
            throw DoctoredCoreError(message: "Could not read \(url.lastPathComponent).")
        }
        guard size <= maximumImageBytes else {
            throw DoctoredCoreError(
                message: "\(url.lastPathComponent) is larger than 64 MB; pick a smaller sky.")
        }
        let image = try RasterImage.open(url: url)
        let data: Data
        do { data = try Data(contentsOf: url) } catch {
            throw DoctoredCoreError(message: "Could not read \(url.lastPathComponent).")
        }
        let digest = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        let raw = url.pathExtension.lowercased()
        let plain = (1...8).contains(raw.count) && raw.allSatisfy({ $0.isLetter || $0.isNumber })
        let ext = plain ? raw : "img"
        let name = try Library.normalizedName(url.deletingPathExtension().lastPathComponent)
        let sky = UserSky(name: name, image: "\(digest).\(ext)", width: image.width, height: image.height)
        try FileManager.default.createDirectory(at: imagesDirectory, withIntermediateDirectories: true,
                                                attributes: [.posixPermissions: 0o700])
        let target = imagesDirectory.appendingPathComponent(sky.image)
        // A copy this call wrote is removed again when the entry is refused (name in use, library
        // full), so a refusal leaves no unreferenced image behind.
        let created = !FileManager.default.fileExists(atPath: target.path)
        if created { try DurableFile.write(data, to: target) }
        do { return try Library.save(sky) } catch {
            if created { try? FileManager.default.removeItem(at: target) }
            throw error
        }
    }

    /// Removes the named user sky, and its image copy when no other entry names it.
    static func deleteUserSky(named name: String) throws {
        let skies = try userSkies()
        let key = Library.key(try Library.normalizedName(name))
        guard let sky = skies.first(where: { Library.key($0.name) == key }) else {
            throw DoctoredCoreError(message: "No sky named “\(name)”.")
        }
        try Library.delete(name: sky.name)
        if !skies.contains(where: { $0.image == sky.image && $0 != sky }) {
            try? FileManager.default.removeItem(at: imagesDirectory.appendingPathComponent(sky.image))
        }
    }

    /// A sky by name: a library id first, then a user sky's name. A name matching neither refuses
    /// `notInstalled` while the library is not downloaded (it may name a library sky), else
    /// "No sky named “x”.".
    static func resolve(_ name: String) throws -> SkyChoice {
        if let preset = presets().first(where: { $0.id == name }) { return .library(preset) }
        let key = Library.key(name.trimmingCharacters(in: .whitespacesAndNewlines))
        if let sky = try userSkies().first(where: { Library.key($0.name) == key }) { return .user(sky) }
        if readyDirectory == nil { throw DoctoredCoreError(message: notInstalled) }
        throw DoctoredCoreError(message: "No sky named “\(name)”.")
    }

    /// The sky's pixels at full size.
    static func image(_ choice: SkyChoice) throws -> RasterImage {
        switch choice {
        case .library(let preset):
            guard let directory = readyDirectory else { throw DoctoredCoreError(message: notInstalled) }
            return try RasterImage.open(url: directory.appendingPathComponent(preset.file))
        case .user(let sky):
            return try RasterImage.open(url: imagesDirectory.appendingPathComponent(sky.image))
        case .file(let url):
            return try RasterImage.open(url: url)
        }
    }

    /// The copy of `sky` the dialog previews with: resized ONCE by f = min(1, 4·max(proxyW/sw,
    /// proxyH/sh)) — enough for the proxy at the minimum 25 % scale — or the sky itself at f = 1.
    /// Framing is in canvas fractions, so the reduced sky frames as the full one does.
    static func previewImage(_ sky: RasterImage, proxyWidth: Int, proxyHeight: Int) -> RasterImage {
        let f = min(1, 4 * max(Double(proxyWidth) / Double(sky.width),
                               Double(proxyHeight) / Double(sky.height)))
        guard f < 1 else { return sky }
        let w = max(1, Int((Double(sky.width) * f).rounded()))
        let h = max(1, Int((Double(sky.height) * f).rounded()))
        return sky.resized(w: w, h: h, filter: DR_FILTER_LANCZOS3) ?? sky
    }

    /// How a reply names the sky: `{kind, id|name|path}`.
    static func identifier(_ choice: SkyChoice) -> [String: Any] {
        switch choice {
        case .library(let preset): return ["kind": "library", "id": preset.id]
        case .user(let sky): return ["kind": "user", "name": sky.name]
        case .file(let url): return ["kind": "file", "path": url.path]
        }
    }
}
