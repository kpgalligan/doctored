import Foundation

// Synchronize RAW Settings' Masking: the source layer's locals REPLACE each target's, in
// order. Drawn coverage is resampled per axis in normalised coordinates of each RAW's canonical
// frame (`RasterRawLocalMask.resized`); feather, a blur radius in mask pixels, is scaled by the
// geometric mean of the two axis ratios (clamped to 256); a range keeps its stored Lab; a
// Subject, People or Sky local is detected again on the target (Recompute's rule).

/// Where a recorded Masking sync keeps each drawn mask, as `<sha256>.drmask`, so the
/// `apply_raw_settings` step it records can replay without the source document.
enum RawLocalMaskStore {
    static let environmentName = "DR_RAW_LOCAL_MASKS_DIR"

    static var directory: URL {
        if let path = ProcessInfo.processInfo.environment[environmentName], !path.isEmpty {
            return URL(fileURLWithPath: path, isDirectory: true)
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                               in: .userDomainMask)[0]
        return support.appendingPathComponent("Doctored/RAW Local Masks", isDirectory: true)
    }

    /// Writes `mask` once (content-addressed) and answers its path. The admit is
    /// `get_raw_local_mask`'s export admit.
    static func store(_ mask: RasterRawLocalMask) throws -> String {
        let folder = directory
        let url = folder.appendingPathComponent(
            try mask.sha256() + "." + SideFileExtension.rawLocalMask.current)
        if FileManager.default.fileExists(atPath: url.path) { return url.path }
        let info = try mask.info()
        let width = try PrecisionDocumentDescriptor.integer(info, "width")
        let height = try PrecisionDocumentDescriptor.integer(info, "height")
        let budget = width * height * 4 + 1024 * 1024
        _ = try RasterResources.shared.admit(scratch: budget)
        do {
            try BackgroundWork.wait(try WorkerTask(scratchEstimate: budget) {
                try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                let bytes = try mask.encodedBytes()
                try DurableFile.write(bytes, to: url)
            }, title: "Recording RAW Local Masks")
        } catch {
            throw DoctoredCoreError(message: "Could not record the RAW local mask at \(url.path): "
                + error.localizedDescription)
        }
        return url.path
    }
}

struct RawLocalSyncPayload {
    enum Mask {
        case coverage(RasterRawLocalMask)
        case generated(RawLocalGeneratedKind)
    }
    struct Local {
        /// `["id": id]` plus the node's `controlsJSON` without `generated`.
        let controls: [String: Any]
        let mask: Mask
    }
    /// Source order, at most `RawEnhancementRecipe.maximumLocals`.
    let locals: [Local]
    /// The source state's frame, the one each local's `feather` is stated in.
    let sourceWidth: Int
    let sourceHeight: Int

    static func capture(source: RasterDocument, layer: Int) throws -> RawLocalSyncPayload {
        let state = try RawLocalWork.state(source, layer: layer)
        let locals = zip(state.nodes, state.masks).map { node, mask -> Local in
            var controls = node.controlsJSON
            controls.removeValue(forKey: "generated")
            controls["id"] = node.id
            return Local(controls: controls, mask: node.generated.map { .generated($0) } ?? .coverage(mask))
        }
        return RawLocalSyncPayload(locals: locals, sourceWidth: state.width, sourceHeight: state.height)
    }

    /// `apply_raw_settings`' `locals`, `mask_paths` and `source_size` (the replay of a recorded
    /// sync). Each mask file is read whole under `encodedBytes`' admit.
    static func parse(locals: Any?, maskPaths: Any?, sourceSize: Any?) throws -> RawLocalSyncPayload {
        guard let objects = locals as? [[String: Any]],
              objects.count <= RawEnhancementRecipe.maximumLocals else {
            throw DoctoredCoreError(message: "locals must be an array of at most 16 local objects.")
        }
        func side(_ value: Any) -> Int? {
            if let text = value as? String, let integer = Int(text) {
                return (1...64_000_000).contains(integer) ? integer : nil
            }
            guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
                  number.doubleValue.rounded() == number.doubleValue,
                  (1...64_000_000).contains(number.doubleValue) else { return nil }
            return number.intValue
        }
        guard let size = sourceSize as? [Any], size.count == 2,
              let sourceWidth = side(size[0]), let sourceHeight = side(size[1]) else {
            throw DoctoredCoreError(message: "source_size must be [width, height], two positive integers, with locals.")
        }
        var paths: [String] = []
        if let maskPaths, !(maskPaths is NSNull) {
            guard let values = maskPaths as? [String], values.count <= RawEnhancementRecipe.maximumLocals else {
                throw DoctoredCoreError(message: "mask_paths must be an array of at most 16 .drmask paths.")
            }
            paths = values
        }
        var masks: [RasterRawLocalMask] = []
        for path in paths {
            let maskFile = SideFileExtension.rawLocalMask
            guard path.hasPrefix("/"), !path.utf8.contains(0), maskFile.matches(path: path) else {
                throw DoctoredCoreError(message: "Each mask_paths entry must be an absolute "
                    + ".\(maskFile.current) path (or the legacy .\(maskFile.legacy)).")
            }
            let url = URL(fileURLWithPath: path)
            let attributes = try? FileManager.default.attributesOfItem(atPath: path)
            guard let count = (attributes?[.size] as? NSNumber)?.uint64Value,
                  count >= 16, count <= 128_000_016 else {
                throw DoctoredCoreError(message: "The RAW local mask at \(path) is missing or exceeds its bound.")
            }
            _ = try RasterResources.shared.admit(scratch: count * 2 + 1024 * 1024)
            let data: Data
            do { data = try Data(contentsOf: url) } catch {
                throw DoctoredCoreError(message: "Could not read the RAW local mask at \(path): "
                    + error.localizedDescription)
            }
            masks.append(try RasterRawLocalMask.fromEncodedBytes(data))
        }
        var used = Set<Int>()
        var parsed: [Local] = []
        for object in objects {
            try RawLocalJSON.keys(object, RawLocalOperations.controlKeys.union(["id", "mask"]))
            guard let id = object["id"] as? String, RawLocalJSON.validID(id),
                  let mask = object["mask"] as? [String: Any] else {
                throw DoctoredCoreError(message: "Each local needs a stable id and a mask.")
            }
            var controls = object
            controls.removeValue(forKey: "mask")
            if let kind = try RawLocalGenerated.kind(of: mask) {
                parsed.append(Local(controls: controls, mask: .generated(kind)))
            } else if let index = RawLocalOperations.coverageIndex(mask, provided: masks.count),
                      used.insert(index).inserted {
                parsed.append(Local(controls: controls, mask: .coverage(masks[index])))
            } else {
                throw DoctoredCoreError(message: "Each local's mask is {op: coverage, index} naming its own mask_paths entry, or {op: subject|people|sky}.")
            }
        }
        guard used.count == masks.count else {
            throw DoctoredCoreError(message: "Each mask_paths entry must be named by exactly one coverage local.")
        }
        return RawLocalSyncPayload(locals: parsed, sourceWidth: sourceWidth, sourceHeight: sourceHeight)
    }

    /// Remove every target local, then create each source local in order. `provided` holds the
    /// drawn masks the `coverage` creates name.
    func operations(replacing target: RawLocalState)
        -> (operations: [[String: Any]], provided: [RasterRawLocalMask]) {
        var operations: [[String: Any]] = target.nodes.map { ["op": "remove", "id": $0.id] }
        var provided: [RasterRawLocalMask] = []
        let scale = ((Double(target.width) / Double(sourceWidth))
            * (Double(target.height) / Double(sourceHeight))).squareRoot()
        for local in locals {
            var create = local.controls
            create["op"] = "create"
            if let feather = local.controls["feather"] as? Double {
                create["feather"] = min(256, feather * scale)
            }
            switch local.mask {
            case .coverage(let mask):
                create["mask"] = ["op": "coverage", "index": provided.count] as [String: Any]
                provided.append(mask)
            case .generated(let kind):
                create["mask"] = ["op": kind.rawValue]
            }
            operations.append(create)
        }
        return (operations, provided)
    }

    /// The recorded step's `locals`, `mask_paths` and `source_size`. Masks are written to
    /// `RawLocalMaskStore` only while a recording runs; otherwise nothing stores the step, so
    /// it carries no paths.
    func recordedArguments() throws -> [String: Any] {
        let recording = ActionRecorder.shared.isRecording
        var objects: [[String: Any]] = []
        var paths: [String] = []
        var index = 0
        for local in locals {
            var object = local.controls
            switch local.mask {
            case .coverage(let mask):
                object["mask"] = ["op": "coverage", "index": index] as [String: Any]
                index += 1
                if recording { paths.append(try RawLocalMaskStore.store(mask)) }
            case .generated(let kind):
                object["mask"] = ["op": kind.rawValue]
            }
            objects.append(object)
        }
        var result: [String: Any] = ["locals": objects, "source_size": [sourceWidth, sourceHeight]]
        if recording { result["mask_paths"] = paths }
        return result
    }
}
