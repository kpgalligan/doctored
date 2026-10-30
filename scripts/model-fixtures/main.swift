import AppKit
import CoreML
import Darwin

// The fixture model store's filler, built and run by
// `scripts/fill_model_fixtures.py` with DR_MODELS_DIR pointing at the shared store
// (~/devel/doctored-fixtures/models). It goes through the app's own `ModelStore` and
// `ModelDownloader` — the licence acceptance, the resumable SHA-256-verified download and the
// Core ML compile — for EVERY catalogue entry, then runs each model once on a small gradient
// (depth through `DepthEstimator`, sky and person parts through `ModelMaskRun.predictImage`, SAM
// 2.1 through its three packages), so a filled store is known to load and predict. The sky
// library (an `assets` entry) is ready through `readyDirectory`; its manifest is decoded and
// its first sky opened.
// NETWORK: it downloads from each entry's host. With DR_MODEL_FIXTURE_RELEASE_BASE set (the
// script's --release-dir), every https://github.com/kpgalligan/doctored-models/releases/download/
// URL is rewritten to that local server first, so our own conversions are filled from the
// conversion's output before the release is published.

_ = NSApplication.shared
AutomationAlerts.installIfEnabled()

func say(_ text: String) {
    print(text)
    fflush(stdout)
}

let releasePrefix = "https://github.com/kpgalligan/doctored-models/releases/download/"
var catalog = ModelCatalog.entries
if let base = ProcessInfo.processInfo.environment["DR_MODEL_FIXTURE_RELEASE_BASE"], !base.isEmpty {
    let root = base.hasSuffix("/") ? base : base + "/"
    catalog = catalog.map { entry in
        let files = entry.files.map { file -> ModelFile in
            let text = file.url.absoluteString
            guard text.hasPrefix(releasePrefix), let local = URL(string: root + text.dropFirst(releasePrefix.count))
            else { return file }
            return ModelFile(path: file.path, url: local, sha256: file.sha256, size: file.size)
        }
        return ModelCatalogEntry(
            id: entry.id, name: entry.name, capability: entry.capability, version: entry.version,
            files: files, runtime: entry.runtime, minimumMacOS: entry.minimumMacOS, license: entry.license,
            trainingData: entry.trainingData, inferenceBytes: entry.inferenceBytes, packages: entry.packages)
    }
    say("release assets: \(root)")
}
// DR_MODEL_FIXTURE_ONLY (the script's --only) keeps one catalogue entry, by id or legacy id.
if let only = ProcessInfo.processInfo.environment["DR_MODEL_FIXTURE_ONLY"], !only.isEmpty {
    catalog = catalog.filter { $0.id == only || ModelCatalog.legacyID(of: $0.id) == only }
    guard !catalog.isEmpty else {
        say("FAIL: no catalogue entry \(only)")
        exit(1)
    }
    say("only: \(only)")
}
let store = ModelStore(directory: ModelStore.shared.directory, catalog: catalog)
ModelStore.shared = store
say("store: \(store.directory.path)")

func fill(_ entry: ModelCatalogEntry) -> Bool {
    say("model: \(entry.name) (\(entry.id) @ \(entry.version))")
    say("licence: \(entry.license.name) \(entry.license.url.absoluteString) — tier \(entry.license.tier.rawValue)")
    say("training data: \(entry.trainingData)")
    do {
        if !store.isAccepted(entry.id) {
            try store.accept(entry.id)
            say("accepted the licence for version \(entry.version)")
        }
        try store.download(entry.id)
    } catch {
        say("FAIL: \(error.localizedDescription)")
        return false
    }
    var lastReport = Date.distantPast
    let deadline = Date().addingTimeInterval(1800)
    while true {
        let state = store.state(entry.id)
        if case .downloading(let fraction) = state {
            if Date().timeIntervalSince(lastReport) > 5 {
                say(String(format: "downloading %.1f%%", fraction * 100))
                lastReport = Date()
            }
        } else {
            break
        }
        if Date() > deadline {
            say("FAIL: the download did not finish within 30 minutes")
            return false
        }
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
    }
    guard store.state(entry.id) == .ready, let compiled = location(entry) else {
        say("FAIL: the model is not ready: \(store.state(entry.id))")
        return false
    }
    say("ready: \(compiled.path) (\(store.bytesOnDisk(entry.id)) bytes on disk)")
    return true
}

/// A ready entry's compiled package, or for an `assets` entry (the sky library) its ready
/// version folder; nil while it is not ready.
func location(_ entry: ModelCatalogEntry) -> URL? {
    entry.runtime == .assets ? store.readyDirectory(entry.id) : store.compiledModelURL(entry.id)
}

/// A 64 × 48 gradient: enough for every model to load and answer something with a range.
var gradient = [UInt8](repeating: 255, count: 64 * 48 * 4)
for y in 0..<48 {
    for x in 0..<64 {
        let p = (y * 64 + x) * 4
        gradient[p] = UInt8(x * 4)
        gradient[p + 1] = UInt8(y * 5)
        gradient[p + 2] = UInt8(128)
    }
}
guard let image = RasterImage.from(rgba: gradient, width: 64, height: 48)?.makeCGImage(in: ColorProfile.sRGB) else {
    say("FAIL: could not build the test image")
    exit(1)
}

/// SAM 2.1's three packages, one point prompt: the decoder's masks and scores.
func predictSAM(_ entry: ModelCatalogEntry) throws -> String {
    let urls = entry.packages.compactMap { store.compiledModelURL(entry.id, package: $0) }
    guard urls.count == 3 else { throw DoctoredCoreError(message: "SAM needs three compiled packages") }
    let encoder = try CoreMLRuntime.load(urls[0])
    let input = try CoreMLRuntime.imageInput(image, for: encoder)
    let encoded = try encoder.prediction(from: MLDictionaryFeatureProvider(dictionary: [input.name: input.value]))
    let points = try MLMultiArray(shape: [1, 1, 2], dataType: .float16)
    points[[0, 0, 0]] = 512
    points[[0, 0, 1]] = 512
    let labels = try MLMultiArray(shape: [1, 1], dataType: .float16)
    labels[[0, 0]] = 1
    let prompt = try CoreMLRuntime.load(urls[1]).prediction(from: MLDictionaryFeatureProvider(dictionary: [
        "points": MLFeatureValue(multiArray: points), "labels": MLFeatureValue(multiArray: labels),
    ]))
    var inputs: [String: MLFeatureValue] = [:]
    for name in ["image_embedding", "feats_s0", "feats_s1"] { inputs[name] = encoded.featureValue(for: name) }
    inputs["sparse_embedding"] = prompt.featureValue(for: "sparse_embeddings")
    inputs["dense_embedding"] = prompt.featureValue(for: "dense_embeddings")
    let decoded = try CoreMLRuntime.load(urls[2]).prediction(from: MLDictionaryFeatureProvider(dictionary: inputs))
    guard let masks = decoded.featureValue(for: "low_res_masks"),
          let scores = decoded.featureValue(for: "scores")?.multiArrayValue else {
        throw DoctoredCoreError(message: "the decoder answered no low_res_masks or scores")
    }
    let plane = try CoreMLRuntime.plane(masks)
    guard plane.channels == scores.count else { throw DoctoredCoreError(message: "masks and scores disagree") }
    return "\(plane.channels)×\(plane.width)×\(plane.height) masks, scores "
        + (0..<scores.count).map { String(format: "%.3f", scores[$0].doubleValue) }.joined(separator: " ")
}

var failed = false
for entry in catalog {
    guard fill(entry), let compiled = location(entry) else {
        failed = true
        continue
    }
    do {
        switch entry.capability {
        case "depth":
            let map = try DepthEstimator.estimate(image, compiledModel: compiled)
            let low = map.samples.min() ?? 0, high = map.samples.max() ?? 0
            say("prediction: \(map.width)×\(map.height), samples \(low)…\(high)")
            guard map.samples.count == map.width * map.height, high > low else {
                say("FAIL: the prediction has no range")
                failed = true
                continue
            }
        case "sky", "person_parts":
            let plane = try ModelMaskRun.predictImage(image, compiled: compiled,
                                                      output: entry.capability == "sky" ? "sky" : "classes")
            let low = plane.samples.min() ?? 0, high = plane.samples.max() ?? 0
            say("prediction: \(plane.channels)×\(plane.width)×\(plane.height), samples \(low)…\(high)")
            let expected = entry.capability == "sky" ? 1 : 6
            guard plane.channels == expected, plane.samples.allSatisfy(\.isFinite), low >= -0.01, high <= 1.01 else {
                say("FAIL: the prediction is not \(expected) probability channel(s)")
                failed = true
                continue
            }
        case "object_prompt":
            say("prediction: " + (try predictSAM(entry)))
        case RemoveModelRun.capability:
            // One fill of a mid-grey picture with a centred square hole.
            let side = RemoveModelRun.side, cells = side * side
            let mask = (0..<cells).map { i -> Float in
                let (x, y) = (i % side, i / side)
                return abs(x - side / 2) < side / 8 && abs(y - side / 2) < side / 8 ? 1 : 0
            }
            let plane = try RemoveModelRun.predict(
                image: [Float](repeating: 0.5, count: 3 * cells), mask: mask, compiled: compiled)
            let low = plane.samples.min() ?? 0, high = plane.samples.max() ?? 0
            say("prediction: \(plane.channels)×\(plane.width)×\(plane.height), samples \(low)…\(high)")
            guard plane.channels == 3, plane.width == side, plane.height == side,
                  plane.samples.allSatisfy(\.isFinite), low >= 0, high <= 1 else {
                say("FAIL: the fill is not 3×\(side)×\(side) finite samples in 0…1")
                failed = true
                continue
            }
        case SuperZoomRun.capability, JpegArtifactsRemovalRun.capability:
            // One tile of a horizontal ramp through the model, as the core's stage calls it.
            let superZoom = entry.capability == SuperZoomRun.capability
            let side = superZoom ? NeuralFilterRun.superResolutionTile : NeuralFilterRun.jpegTile
            let outputSide = superZoom ? side * NeuralFilterRun.superResolutionScale : side
            let model = try NeuralTileModel(
                compiled: compiled, side: side, outputSide: outputSide,
                qualityFactor: superZoom ? nil : JpegArtifactsStrength.medium.qualityFactor)
            let input = (0..<3 * side * side).map { Float($0 % side) / Float(side - 1) }
            var output = [Float](repeating: .nan, count: 3 * outputSide * outputSide)
            try input.withUnsafeBufferPointer { tile in
                try output.withUnsafeMutableBufferPointer { try model.predict(tile, into: $0) }
            }
            let low = output.min() ?? 0, high = output.max() ?? 0
            say("prediction: 3×\(outputSide)×\(outputSide), samples \(low)…\(high)")
            guard output.allSatisfy(\.isFinite), low >= -0.1, high <= 1.1, high > low else {
                say("FAIL: the tile is not 3×\(outputSide)×\(outputSide) finite samples near 0…1")
                failed = true
                continue
            }
        case ColorizeRun.capability:
            // One gray ramp through DDColor, as Colorize calls it: 2×512² finite Lab a*, b*.
            let model = try ColorizeRun.model(compiled)
            let side = NeuralFilterRun.colorizeSide
            let input = (0..<3 * side * side).map { Float($0 % side) / Float(side - 1) }
            var output = [Float](repeating: .nan, count: 2 * side * side)
            try input.withUnsafeBufferPointer { gray in
                try output.withUnsafeMutableBufferPointer { try model.predict(gray, into: $0) }
            }
            let low = output.min() ?? 0, high = output.max() ?? 0
            say("prediction: 2×\(side)×\(side), a*/b* \(low)…\(high)")
            guard output.allSatisfy(\.isFinite), low >= -150, high <= 150 else {
                say("FAIL: the chroma is not 2×\(side)×\(side) finite a*, b* within ±150")
                failed = true
                continue
            }
        case SkyLibrary.capability:
            let presets = try SkyLibrary.presets(in: compiled)
            guard let first = presets.first else {
                say("FAIL: the sky library's manifest lists no sky")
                failed = true
                continue
            }
            let sky = try SkyLibrary.image(.library(first))
            say("manifest: \(presets.count) skies; \(first.id) opens at \(sky.width)×\(sky.height)")
        default:
            say("no smoke prediction for capability \(entry.capability)")
        }
    } catch {
        say("FAIL: \(error.localizedDescription)")
        failed = true
    }
}
if failed {
    say("FAIL: the fixture model store is not complete")
    exit(1)
}
say("PASS: fixture model store filled (\(catalog.count) models)")
exit(0)
