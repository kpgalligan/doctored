import Foundation
import Darwin

// The 7E facade connects the typed owners to central Undo and automatic recovery; the separate
// internal archive remains a compatibility probe.

/// Internal typed-document archive proof. It deliberately is not an
/// EditingStateArchive, .drsnapshot, RecoveryStore generation or public mode.
/// Only a typed native source and bounded plain values are persisted.
enum PrecisionStateArchive {
    static let version = 2
    static let metadataLimit: UInt64 = 64 * 1024

    struct Manifest: Codable {
        let version: Int
        let kind: String
        let name: String
        let documentFile: String
        let storage: String
        let evaluation: String
        let width: Int
        let height: Int
        let decodeEstimateBytes: UInt64
    }

    struct Captured {
        let save: PrecisionCapturedSave
        let manifest: Manifest
        let scratchEstimate: UInt64
    }

    struct Loaded {
        let document: RasterPrecisionDocument
        let name: String
    }

    static func capture(_ source: RasterPrecisionDocument, name: String) throws -> Captured {
        let name = try validatedName(name)
        let save = try PrecisionCapturedSave(source)
        let descriptor = save.descriptor
        let decode = try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.sum(descriptor.reopenedRetainedBytes,
                                            descriptor.decodeScratchBytes), metadataLimit)
        let manifest = Manifest(version: version, kind: "precision-document", name: name,
            documentFile: "image.rz", storage: PrecisionDocumentDescriptor.storage,
            evaluation: descriptor.evaluationPolicy,
            width: descriptor.width, height: descriptor.height, decodeEstimateBytes: decode)
        return Captured(save: save, manifest: manifest,
                        scratchEstimate: try PrecisionDocumentDescriptor.sum(save.scratchEstimate,
                                                                               metadataLimit))
    }

    /// Creates a new directory only. Failure removes only this call's partial
    /// directory. An existing package is replaced by save(), after completion.
    static func write(_ captured: Captured, to directory: URL) throws {
        let data = try JSONEncoder().encode(captured.manifest)
        guard data.count <= Int(metadataLimit) else {
            throw failure("Archive metadata exceeds its limit.")
        }
        guard mkdir(directory.path, 0o700) == 0 else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        do {
            try captured.save.write(to: directory.appendingPathComponent("image.rz"),
                                    additionalRetainedBytes: metadataLimit)
            try data.write(to: directory.appendingPathComponent("precision-state.json"))
        } catch {
            try? FileManager.default.removeItem(at: directory)
            throw error
        }
    }

    static func save(_ captured: Captured, to destination: URL) throws {
        _ = try RasterResources.shared.admit(scratch: captured.scratchEstimate)
        let task = try WorkerTask(scratchEstimate: captured.scratchEstimate) {
            let staged = destination.deletingLastPathComponent().appendingPathComponent(
                ".precision-archive-\(UUID().uuidString)")
            defer { try? FileManager.default.removeItem(at: staged) }
            try write(captured, to: staged)
            try DurableFile.publish(staged, to: destination)
        }
        try BackgroundWork.wait(task, title: "Saving Captured High-Depth Archive")
    }

    /// No generic image decoder or working-space adoption is reachable here.
    /// The manifest is admission guidance; the typed native decoder must still
    /// validate actual record types, dimensions and allocations independently.
    static func read(from directory: URL) throws -> Loaded {
        let values = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard values.isDirectory == true, values.isSymbolicLink != true else {
            throw failure("Expected a high-depth archive directory.")
        }
        let bytes = try EditingStateArchive.readFile(
            directory.appendingPathComponent("precision-state.json"), maximum: Int(metadataLimit))
        let manifest = try JSONDecoder().decode(Manifest.self, from: bytes)
        let knownKind = (manifest.version == 1 && manifest.kind == "precision-neutral")
            || (manifest.version == version && manifest.kind == "precision-document")
        guard knownKind,
              manifest.documentFile == "image.rz",
              manifest.storage == PrecisionDocumentDescriptor.storage,
              manifest.evaluation == PrecisionDocumentDescriptor.evaluation,
              manifest.width > 0, manifest.height > 0,
              manifest.width <= RasterImage.maxResizePixels / manifest.height,
              manifest.decodeEstimateBytes >= metadataLimit else {
            throw failure("Unsupported or invalid high-depth archive manifest.")
        }
        let name = try validatedName(manifest.name)
        _ = try RasterResources.shared.admit(scratch: manifest.decodeEstimateBytes)
        let native = directory.appendingPathComponent(manifest.documentFile)
        let nativeValues = try native.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
        guard nativeValues.isRegularFile == true, nativeValues.isSymbolicLink != true else {
            throw failure("Expected a regular typed native document.")
        }
        let file = try FileHandle(forReadingFrom: native)
        defer { try? file.close() }
        guard let head = try file.read(upToCount: 4), ImageDocument.isNativeMagic(head) else {
            throw failure("High-depth archive image.rz is not a native document.")
        }
        let budget = try PrecisionWorkContext(additionalRetainedBytes: metadataLimit)
        let document = try RasterPrecisionDocument.openPreserving(
            url: native, workingLimit: budget.limit, otherRetained: budget.otherRetained)
        let descriptor = try PrecisionDocumentDescriptor(document.descriptor())
        guard descriptor.width == manifest.width, descriptor.height == manifest.height,
              descriptor.evaluationPolicy == manifest.evaluation else {
            throw failure("Archive manifest and typed native document dimensions or evaluation differ.")
        }
        return Loaded(document: document, name: name)
    }

    static func load(from directory: URL) throws -> Loaded {
        let task = try WorkerTask { try read(from: directory) }
        return try BackgroundWork.wait(task, title: "Loading Captured High-Depth Archive")
    }

    private static func validatedName(_ value: String) throws -> String {
        let name = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name.utf8.count <= 512 else {
            throw failure("High-depth archive names must contain 1–512 UTF-8 bytes.")
        }
        return name
    }

    private static func failure(_ message: String) -> DoctoredCoreError {
        DoctoredCoreError(message: message)
    }
}
