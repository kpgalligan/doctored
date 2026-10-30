import Foundation

// The local model catalogue (`designs/neural-models.md` architecture decision 3): models are
// DATA, not code paths. One entry per model and version, decoded from the JSON below; the store
// (`ModelStore.swift`), the downloader (`ModelDownloader.swift`) and Settings ▸ Models
// (`ModelsSettingsPane.swift`) read nothing else. A new model is a new entry here plus the
// consumer that runs it, never a new download or licence path.

/// The licence tiers of `designs/neural-models.md`.
enum ModelLicenseTier: String, Codable {
    case clean
    case cleanDataEncumbered = "clean_data_encumbered"
    case userLicensed = "user_licensed"
    case never
    case cloud

    /// The tier as the licence card and the Models pane name it.
    var title: String {
        switch self {
        case .clean: return "Clean"
        case .cleanDataEncumbered: return "Clean code and weights; training data encumbered"
        case .userLicensed: return "User licensed"
        case .never: return "Not shipped"
        case .cloud: return "Cloud"
        }
    }
}

/// How a model runs: a Core ML package, a system framework with nothing to download, or plain
/// assets a feature reads from the ready version folder (`ModelStore.readyDirectory`): the sky
/// library's JPEGs and manifest, which nothing compiles.
enum ModelRuntime: String, Codable {
    case coreml
    case system
    case assets
}

/// One file of a model, fetched from `url` into the version folder at `path`.
struct ModelFile: Codable, Equatable {
    let path: String
    let url: URL
    let sha256: String
    let size: Int64
}

struct ModelLicense: Codable, Equatable {
    let name: String
    let url: URL
    let tier: ModelLicenseTier
}

/// One model at one pinned version. `packages` are the Core ML packages (folders among the files'
/// paths) compiled once every file is verified, in the order the model's consumer runs them
/// (SAM 2.1's image encoder, prompt encoder and mask decoder); empty for a model with nothing to
/// compile.
struct ModelCatalogEntry: Codable, Equatable {
    let id: String
    let name: String
    let capability: String
    let version: String
    let files: [ModelFile]
    let runtime: ModelRuntime
    let minimumMacOS: String
    let license: ModelLicense
    let trainingData: String
    /// The inference memory plan, admitted with `DepthBlurRun.captureBytesPerPixel` per canvas
    /// pixel before the model runs (`DepthBlurRun.capture`), or with
    /// `ModelMaskRun.captureBytesPerPixel` for the selection models (their records are on
    /// `ModelCatalog.json`).
    ///
    /// Record for the depth entry (`scripts/depth-blur-admission/measure.py`: the Release
    /// app at `DR_WORKING_LIMIT_BYTES` 8 GiB, `depth_blur` blur_strength 0 — the capture alone —
    /// on a one-layer 6000×4000 8-bit document of two-colour Clouds; `MallocLargeCache=0`, E =
    /// `ri_interval_max_phys_footprint` − pre around the one call, a fresh process per case,
    /// beside a baseline twin at E 16,384): E 315,932,912 / 315,572,440. The capture plan
    /// 209,715,200 (200 MiB) + 8 × 24,000,000 = 401,715,200 ≥ ⌈1.25 × 315,932,912⌉ =
    /// 394,916,140. Vision's subject mask, which the same job runs for Focus Subject, is not in
    /// the measured case.
    let inferenceBytes: UInt64
    let packages: [String]

    enum CodingKeys: String, CodingKey {
        case id, name, capability, version, files, runtime, license, packages
        case minimumMacOS = "minimum_macos"
        case trainingData = "training_data"
        case inferenceBytes = "inference_bytes"
    }

    /// Every file's catalogued size.
    var totalBytes: Int64 { files.reduce(0) { $0 + $1.size } }
}

enum ModelCatalog {
    /// The catalogue. A text that does not decode is a build defect: Debug traps, Release lists
    /// no model.
    static let entries: [ModelCatalogEntry] = {
        do {
            return try JSONDecoder().decode([ModelCatalogEntry].self, from: Data(json.utf8))
        } catch {
            assertionFailure("ModelCatalog.json does not decode: \(error)")
            return []
        }
    }()

    /// The entry that provides `capability` ("depth"), nil when none does.
    static func entry(capability: String) -> ModelCatalogEntry? {
        entries.first { $0.capability == capability }
    }

    /// Every entry that provides `capability` ("sky", "person_parts", "object_prompt"), in
    /// catalogue order, which is the order of preference: a consumer runs the first one ready.
    static func entries(capability: String) -> [ModelCatalogEntry] {
        entries.filter { $0.capability == capability }
    }

    /// The first-party package-id prefix, and the one those ids carried before the Doctored rename.
    static let firstPartyPrefix = "doctored/"
    static let legacyFirstPartyPrefix = "rasterize/"

    /// `rasterize/<name>` for `doctored/<name>`; nil for every other id (twistlabs/, apple/, …).
    /// A store's `rasterize--<name>` folder and `rasterize/<name>` acceptance are read in place
    /// under the new id (`ModelStore.modelDirectory`, `isAccepted`), never moved.
    static func legacyID(of id: String) -> String? {
        guard id.hasPrefix(firstPartyPrefix) else { return nil }
        let name = id.dropFirst(firstPartyPrefix.count)
        guard !name.isEmpty else { return nil }
        return legacyFirstPartyPrefix + name
    }

    /// Apple's Core ML conversion of Depth Anything V2 Small, F16 (the variant built for the
    /// Neural Engine), pinned to the repository commit whose file list and LFS hashes were read
    /// from the Hugging Face API on 2026-10-13 (`/api/models/apple/coreml-depth-anything-v2-small`
    /// and `/tree/<rev>?recursive=true`; the Manifest's hash from one verified download).
    ///
    /// The selection models, pinned the same way on 2026-10-14 (each repository's
    /// `/tree/<rev>?recursive=true` for sizes and LFS hashes, each Manifest hashed from one
    /// download): twistlabs' MIT Core ML conversion of the sky U²-Net small (Select ▸ Sky); our
    /// own conversion of MediaPipe's multiclass selfie segmenter (Select ▸ Person Parts…), whose
    /// assets `scripts/convert_mediapipe_multiclass.py` wrote with the SHA-256s in its
    /// receipt and which the doctored-models release `<version>` serves; and Apple's SAM 2.1
    /// Large then Tiny (the Object Finder runs the first one ready, so Large is preferred),
    /// three packages each, run in the listed order: image encoder, prompt encoder, mask decoder.
    ///
    /// Their `inference_bytes` records (`scripts/model-selection-admission/measure.py`:
    /// the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB, `MallocLargeCache=0`, a fresh process per
    /// case beside a baseline twin at E ≤ 49,152, E = `ri_interval_max_phys_footprint` − pre around
    /// the one call with `refine: false`, so the inference job alone; plan P = inference_bytes +
    /// `ModelMaskRun.captureBytesPerPixel` (14) × n + 2 × 4 × 2²⁰ + 2n, + n per person clip):
    /// - sky (sky.jpg at 6000×4000, `select_sky`): E 453,886,336 / 451,756,416 / 451,756,392;
    ///   200 MiB gives P 602,103,808 ≥ ⌈1.25 × 453,886,336⌉ = 567,357,920.
    /// - person parts (portrait.png at 6000×6000, one person, `select_person_parts` all four parts,
    ///   Vision's analysis and the clip inside the call): E 626,262,616 / 698,892,888 / 399,081,784;
    ///   320 MiB gives P 955,932,928 ≥ 873,616,110.
    /// - SAM 2.1 Large (product.jpg at 6000×4000, a box prompt): E 540,230,184 / 628,424,920 /
    ///   627,703,976; 448 MiB gives P 862,150,656 ≥ 785,531,150.
    /// - SAM 2.1 Tiny (the same, a store holding only Tiny): E 537,690,520 / 789,381,528 /
    ///   792,101,368; 640 MiB gives P 1,063,477,248 ≥ 990,126,710.
    ///
    /// The sky library (Edit ▸ Sky Replacement) is an `assets` entry, not a model: 24 CC0
    /// sky crops of Poly Haven HDRIs plus `manifest.json`, written with their SHA-256s by
    /// `scripts/prepare_sky_library.py` (`catalog-entry.json`) and served by the doctored-models
    /// release `sky-library-1`. It runs nothing, so `inference_bytes` is 0.
    ///
    /// MI-GAN 512 Places2 (the Remove tool's Model mode) is our own Core ML conversion of
    /// Picsart AI Research's MIT generator, written with its SHA-256s by
    /// `scripts/convert_migan.py` (`catalog-entry.json`, FLOAT16, self-check passed) and served
    /// by the doctored-models release `migan-512-places2-coreml-1`. Its `inference_bytes` record
    /// (`scripts/remove-model-admission/measure.py`: the Release app at `DR_WORKING_LIMIT_BYTES`
    /// 8 GiB, `MallocLargeCache=0`, a fresh process per case beside a baseline twin at E ≤ 49,152,
    /// sky.jpg at 6000×4000, `remove_stroke` model, size 400 from (1500, 1500) to (3500, 2500),
    /// crop 4804×4000; plan P = the reply's `planned_bytes` = the core's plan + inference_bytes +
    /// `RemoveModelRun.hostBytesPerPixel` (3) × n): at the 200 MiB place-holder the 8-bit cases
    /// missed — all E 583,139,808 / 581,960,064 / 581,927,320 against P 596,470,272 (need
    /// 728,924,760), current and below likewise (need 727,101,950 and 726,692,380) — while the
    /// 16-bit ones passed (P 1,465,713,152 / 888,664,576 / 1,465,713,152 against need 860,119,610
    /// / 859,873,880 / 860,263,000). 327 MiB, the least MiB multiple clearing 728,924,760, gives
    /// the 8-bit P 729,639,424. The confirming run at 327 MiB passed all six cases (8 bits need
    /// at most 727,286,240; 16 bits P 1,598,882,304 / 1,021,833,728 / 1,598,882,304 against
    /// 860,222,010 / 566,968,650 / 860,242,460); the 8-bit E is bimodal, about 0.35 or 0.58 GB.
    ///
    /// Real-ESRGAN x4plus (Filter ▸ Neural Filters ▸ Super Zoom; capability
    /// `super_resolution`) and FBCNN colour (JPEG Artifacts Removal and Super Zoom's Remove
    /// JPEG Artifacts; `jpeg_artifacts`) are our own Core ML conversions, written with their
    /// SHA-256s by `scripts/convert_realesrgan.py` and `scripts/convert_fbcnn.py`
    /// (`catalog-entry.json`, FLOAT16, self-checks passed) and served by the doctored-models
    /// releases `realesrgan-x4plus-coreml-1` and `fbcnn-color-coreml-1`. Their `inference_bytes`
    /// record (`scripts/neural-filters-admission/measure.py`, Release, 6000×4000, 3 runs + twins,
    /// plan P = each stage's core plan + inference_bytes + the host term
    /// `NeuralFilterRun.hostBytesPerOutputPixel(bytesPerSample:)` × the output): at 200 MiB the
    /// 16-bit cases, whose host share is the smaller, passed (jpeg16 max E 359,973,104 against
    /// P 985,055,232; sliders16 4,493,856,032 against 6,550,204,984), so the 8-bit misses were the
    /// host's display term, raised there, and 200 MiB stays.
    ///
    /// DDColor-L ModelScope (Filter ▸ Neural Filters ▸ Colorize; capability `colorize`) is
    /// our own Core ML conversion of `piddnad/ddcolor_modelscope`, written with its SHA-256s by
    /// `scripts/convert_ddcolor.py` (FLOAT32: FLOAT16 collapsed the predicted chroma, mean |Δab|
    /// 2.9–10.9 against the fp32 model; FLOAT32's worst mean 0.0006, p99 0.0045) and served by the
    /// doctored-models release `ddcolor-modelscope-coreml-1`. Chosen by the rule fixed before
    /// measuring (the smallest package unless a larger one's mean |Δab| against the photos' own
    /// ab is at least 10 % lower; none over 1.5 GB or 3 s): paper_tiny 242,202,364 B, 40.9 ms,
    /// 7.346; paper 933,752,553 B, 84.6 ms, 6.138; modelscope 933,752,553 B, 84.2 ms, 5.222 (M2
    /// Max, compute units ALL); `ddcolor_artistic` was not a candidate (ImageNet plus unnamed
    /// private data). Its `inference_bytes` record (`scripts/neural-filters-admission/measure.py`,
    /// Release, 6000×4000, 3 runs + twins, P = `dr_neural_colorize_plan` + inference_bytes + the
    /// host term × the pixels, + the canvas source for New Document): at the 200 MiB place-holder
    /// every case missed — colorize8 (layer, every slider) P 963,152,384 against need
    /// 2,198,344,640, colorize16 1,132,778,532 / 2,431,140,920, colordoc8 1,059,152,384 /
    /// 2,475,275,440, colordoc16 1,324,778,532 / 2,732,647,750 — the FP32 ConvNeXt-L load (E
    /// 1.76–2.19 GB, the image terms small). At 1551 MiB, the least MiB multiple clearing that
    /// run, the second measure passed colordoc8 (P 2,475,778,560, ratio 1.250) and colordoc16
    /// (2,741,404,708, 1.254) but missed colorize8 (P 2,379,778,560 against need 2,474,292,400)
    /// and colorize16 (2,549,404,708 / 2,706,781,450): E is bimodal, about 1.76 or 1.98 GB at 8
    /// bits and 1.94 or 2.17–2.19 GB at 16, and the layer cases met the upper mode. 1702 MiB, the
    /// least MiB multiple clearing every case over both measures' six runs (colorize16 needs
    /// 1,783,718,118), gives P 2,538,113,536 / 2,707,739,684 / 2,634,113,536 / 2,899,739,684;
    /// host terms unchanged.
    private static let json = """
    [
      {
        "id": "apple/coreml-depth-anything-v2-small",
        "name": "Depth Anything V2 Small (Core ML, F16)",
        "capability": "depth",
        "version": "cfef6f6f2a70783dedc0bfae40cecbc2052285d3",
        "files": [
          {
            "path": "DepthAnythingV2SmallF16.mlpackage/Manifest.json",
            "url": "https://huggingface.co/apple/coreml-depth-anything-v2-small/resolve/cfef6f6f2a70783dedc0bfae40cecbc2052285d3/DepthAnythingV2SmallF16.mlpackage/Manifest.json",
            "sha256": "2883ae290c48fe916dc5ececac03a7d847fa277165a49ef5652fa1d2b9cb55f7",
            "size": 617
          },
          {
            "path": "DepthAnythingV2SmallF16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://huggingface.co/apple/coreml-depth-anything-v2-small/resolve/cfef6f6f2a70783dedc0bfae40cecbc2052285d3/DepthAnythingV2SmallF16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "sha256": "44ac97a3efcfd52113183fb2862ff59cd0368e9ec2e30a90a54980dd11407042",
            "size": 399433
          },
          {
            "path": "DepthAnythingV2SmallF16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://huggingface.co/apple/coreml-depth-anything-v2-small/resolve/cfef6f6f2a70783dedc0bfae40cecbc2052285d3/DepthAnythingV2SmallF16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "sha256": "fa60d9b6a155734f59029ebb882fd54e549bfaee3539c1a9cbd2cbbab64a0fed",
            "size": 49419072
          }
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {
          "name": "Apache-2.0",
          "url": "https://www.apache.org/licenses/LICENSE-2.0",
          "tier": "clean_data_encumbered"
        },
        "training_data": "Depth Anything V2's teacher was trained on synthetic depth only (Hypersim, Virtual KITTI 2, BlendedMVS, IRS, TartanAir); the Small student was then trained on about 62 million real images pseudo-labelled by that teacher (from SA-1B, Open Images, BDD100K, Google Landmarks, ImageNet-21K, LSUN, Objects365 and Places365), whose own licences vary.",
        "inference_bytes": 209715200,
        "packages": ["DepthAnythingV2SmallF16.mlpackage"]
      },
      {
        "id": "twistlabs/fotometis-sky",
        "name": "Sky Segmentation, U²-Net small (Core ML)",
        "capability": "sky",
        "version": "97dfe50bf3fe4dc02badea287512afaf4ce857c2",
        "files": [
          {
            "path": "SkySegSmall.mlpackage/Manifest.json",
            "url": "https://huggingface.co/twistlabs/fotometis-sky/resolve/97dfe50bf3fe4dc02badea287512afaf4ce857c2/SkySegSmall.mlpackage/Manifest.json",
            "sha256": "2ef47fc3ea776f1f11b8870284fe15974e6d8be96515d071851df0d407791677",
            "size": 617
          },
          {
            "path": "SkySegSmall.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://huggingface.co/twistlabs/fotometis-sky/resolve/97dfe50bf3fe4dc02badea287512afaf4ce857c2/SkySegSmall.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "sha256": "f12a39fe104d0a5d2285820d322d9697de5a5011ee355132cbc01822e9d1a06f",
            "size": 256359
          },
          {
            "path": "SkySegSmall.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://huggingface.co/twistlabs/fotometis-sky/resolve/97dfe50bf3fe4dc02badea287512afaf4ce857c2/SkySegSmall.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "sha256": "5df3feebcc15a77480fabb9896b96036e5acc38dc043af414f09dc5b1d296efb",
            "size": 2268608
          }
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {
          "name": "MIT",
          "url": "https://opensource.org/license/mit",
          "tier": "clean_data_encumbered"
        },
        "training_data": "Undisclosed: the U²-Net small weights are xiongzhu666's Sky-Segmentation-and-Post-processing demo model (MIT), trained on sky masks the author has not published; twistlabs converted those ncnn weights to Core ML without retraining (fmtrain.json records the upstream files' SHA-256s).",
        "inference_bytes": 209715200,
        "packages": [
          "SkySegSmall.mlpackage"
        ]
      },
      {
        "id": "doctored/mediapipe-selfie-multiclass-256x256",
        "name": "MediaPipe Multiclass Selfie Segmenter 256 (Core ML)",
        "capability": "person_parts",
        "version": "mediapipe-selfie-multiclass-256x256-coreml-1",
        "files": [
          {
            "path": "selfie_multiclass_256x256.mlpackage/Manifest.json",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/mediapipe-selfie-multiclass-256x256-coreml-1/selfie_multiclass_256x256-Manifest.json",
            "sha256": "04735bcd9171e169a68f08879bdb00962d5e27890b44bf6243eee4b4b78a5f05",
            "size": 617
          },
          {
            "path": "selfie_multiclass_256x256.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/mediapipe-selfie-multiclass-256x256-coreml-1/selfie_multiclass_256x256-model.mlmodel",
            "sha256": "8c636fc6029188ff19bb313f6918fda0d6ecf642668d925a7cefa2125337ea86",
            "size": 311641
          },
          {
            "path": "selfie_multiclass_256x256.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/mediapipe-selfie-multiclass-256x256-coreml-1/selfie_multiclass_256x256-weight.bin",
            "sha256": "19be63ebfc121cc84cfdbf9451a791ae473fc1c238d1d6696b07f17f8ab7cd3e",
            "size": 16295488
          }
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {
          "name": "Apache-2.0",
          "url": "https://www.apache.org/licenses/LICENSE-2.0",
          "tier": "clean"
        },
        "training_data": "Google's own, undisclosed beyond its model card: thousands of annotated samples of people with diverse identities, expressions and poses. Doctored converted Google's TFLite release (selfie_multiclass_256x256, float32, version 1) to Core ML with scripts/convert_mediapipe_multiclass.py, changing no weights.",
        "inference_bytes": 335544320,
        "packages": [
          "selfie_multiclass_256x256.mlpackage"
        ]
      },
      {
        "id": "apple/coreml-sam2.1-large",
        "name": "Segment Anything 2.1 Large (Core ML, F16)",
        "capability": "object_prompt",
        "version": "e830c0874326be7ada2bdc9dff23b7073f884a83",
        "files": [
          {
            "path": "SAM2_1LargeImageEncoderFLOAT16.mlpackage/Manifest.json",
            "url": "https://huggingface.co/apple/coreml-sam2.1-large/resolve/e830c0874326be7ada2bdc9dff23b7073f884a83/SAM2_1LargeImageEncoderFLOAT16.mlpackage/Manifest.json",
            "sha256": "824ba98e9bf70d99139f69383284ca7a84b5666a5a54850b54a617c58fbecc63",
            "size": 617
          },
          {
            "path": "SAM2_1LargeImageEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://huggingface.co/apple/coreml-sam2.1-large/resolve/e830c0874326be7ada2bdc9dff23b7073f884a83/SAM2_1LargeImageEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "sha256": "b382195d0c3e2a93798f39e36085a71e66b1525ebd679d23a1bc8466361028b8",
            "size": 528540
          },
          {
            "path": "SAM2_1LargeImageEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://huggingface.co/apple/coreml-sam2.1-large/resolve/e830c0874326be7ada2bdc9dff23b7073f884a83/SAM2_1LargeImageEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "sha256": "a2908ac87bd862d756a4966c0ea9e710297bbc38b37b8c0243fc33dac5ff51d0",
            "size": 444351360
          },
          {
            "path": "SAM2_1LargePromptEncoderFLOAT16.mlpackage/Manifest.json",
            "url": "https://huggingface.co/apple/coreml-sam2.1-large/resolve/e830c0874326be7ada2bdc9dff23b7073f884a83/SAM2_1LargePromptEncoderFLOAT16.mlpackage/Manifest.json",
            "sha256": "babfa641ed5d14501a8036258561fe5fbcee90223f6045d92f64ca0d42e2e895",
            "size": 617
          },
          {
            "path": "SAM2_1LargePromptEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://huggingface.co/apple/coreml-sam2.1-large/resolve/e830c0874326be7ada2bdc9dff23b7073f884a83/SAM2_1LargePromptEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "sha256": "3a83c167d8bd63e80f86349a78c2ab0527ce97eca1f848a4ce57fe5351241fa3",
            "size": 20618
          },
          {
            "path": "SAM2_1LargePromptEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://huggingface.co/apple/coreml-sam2.1-large/resolve/e830c0874326be7ada2bdc9dff23b7073f884a83/SAM2_1LargePromptEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "sha256": "eadc79dd01eaf71001525a84bf835c8925d3a0463d1b34786bb6b734714fb0e7",
            "size": 2101056
          },
          {
            "path": "SAM2_1LargeMaskDecoderFLOAT16.mlpackage/Manifest.json",
            "url": "https://huggingface.co/apple/coreml-sam2.1-large/resolve/e830c0874326be7ada2bdc9dff23b7073f884a83/SAM2_1LargeMaskDecoderFLOAT16.mlpackage/Manifest.json",
            "sha256": "9b88413746456ef5e562dadd5676c3f2b391f51867ebd24c69e374033cbe094c",
            "size": 617
          },
          {
            "path": "SAM2_1LargeMaskDecoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://huggingface.co/apple/coreml-sam2.1-large/resolve/e830c0874326be7ada2bdc9dff23b7073f884a83/SAM2_1LargeMaskDecoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "sha256": "71d343eb9464951bb6e7de6d8099c268b0dd2bdeb78e694e792b2562a5b55bea",
            "size": 75167
          },
          {
            "path": "SAM2_1LargeMaskDecoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://huggingface.co/apple/coreml-sam2.1-large/resolve/e830c0874326be7ada2bdc9dff23b7073f884a83/SAM2_1LargeMaskDecoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "sha256": "d7ccf6fc95e4fe21dba407f0d97a5859c1abce0c31edf1befb14714cfac58a7f",
            "size": 10222400
          }
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {
          "name": "Apache-2.0",
          "url": "https://www.apache.org/licenses/LICENSE-2.0",
          "tier": "clean_data_encumbered"
        },
        "training_data": "SAM 2.1 was trained by Meta on SA-1B (11 million licensed images with 1.1 billion masks, under the SA-1B Dataset Research License), SA-V (50,900 videos with 642,600 masklets, CC BY 4.0) and Meta's internal licensed video data; the image and video licences are the datasets' own.",
        "inference_bytes": 469762048,
        "packages": [
          "SAM2_1LargeImageEncoderFLOAT16.mlpackage",
          "SAM2_1LargePromptEncoderFLOAT16.mlpackage",
          "SAM2_1LargeMaskDecoderFLOAT16.mlpackage"
        ]
      },
      {
        "id": "apple/coreml-sam2.1-tiny",
        "name": "Segment Anything 2.1 Tiny (Core ML, F16)",
        "capability": "object_prompt",
        "version": "39ae0a8a83e5e6cd196e804bf7cccc5f8171f306",
        "files": [
          {
            "path": "SAM2_1TinyImageEncoderFLOAT16.mlpackage/Manifest.json",
            "url": "https://huggingface.co/apple/coreml-sam2.1-tiny/resolve/39ae0a8a83e5e6cd196e804bf7cccc5f8171f306/SAM2_1TinyImageEncoderFLOAT16.mlpackage/Manifest.json",
            "sha256": "dd72aa75e3f2f92d0653696bf4d8350d87690d92b116b34912fe640f2b116e08",
            "size": 617
          },
          {
            "path": "SAM2_1TinyImageEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://huggingface.co/apple/coreml-sam2.1-tiny/resolve/39ae0a8a83e5e6cd196e804bf7cccc5f8171f306/SAM2_1TinyImageEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "sha256": "6cbc50301ee3ff4a9366083f9647e1f06762759542d8dd0fac394ebc3682cce7",
            "size": 154372
          },
          {
            "path": "SAM2_1TinyImageEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://huggingface.co/apple/coreml-sam2.1-tiny/resolve/39ae0a8a83e5e6cd196e804bf7cccc5f8171f306/SAM2_1TinyImageEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "sha256": "eab96eb8ff35720c79eedc0cac2a4ef32d685f9c994c39736027078528c48a97",
            "size": 67069504
          },
          {
            "path": "SAM2_1TinyPromptEncoderFLOAT16.mlpackage/Manifest.json",
            "url": "https://huggingface.co/apple/coreml-sam2.1-tiny/resolve/39ae0a8a83e5e6cd196e804bf7cccc5f8171f306/SAM2_1TinyPromptEncoderFLOAT16.mlpackage/Manifest.json",
            "sha256": "0c0f9b80f0445017dac52f81e93aeb50b9c2c9918708c882df4a65671fda2bd4",
            "size": 617
          },
          {
            "path": "SAM2_1TinyPromptEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://huggingface.co/apple/coreml-sam2.1-tiny/resolve/39ae0a8a83e5e6cd196e804bf7cccc5f8171f306/SAM2_1TinyPromptEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "sha256": "3a83c167d8bd63e80f86349a78c2ab0527ce97eca1f848a4ce57fe5351241fa3",
            "size": 20618
          },
          {
            "path": "SAM2_1TinyPromptEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://huggingface.co/apple/coreml-sam2.1-tiny/resolve/39ae0a8a83e5e6cd196e804bf7cccc5f8171f306/SAM2_1TinyPromptEncoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "sha256": "af466cf28ef8838f409c2bfd8cc0049b9efbf9db335d60a57dbfc5160af883f2",
            "size": 2101056
          },
          {
            "path": "SAM2_1TinyMaskDecoderFLOAT16.mlpackage/Manifest.json",
            "url": "https://huggingface.co/apple/coreml-sam2.1-tiny/resolve/39ae0a8a83e5e6cd196e804bf7cccc5f8171f306/SAM2_1TinyMaskDecoderFLOAT16.mlpackage/Manifest.json",
            "sha256": "dc6121b61ac560498080d55f9d5fb293cdb305f942a85adb5b71dc8e9d14a8aa",
            "size": 617
          },
          {
            "path": "SAM2_1TinyMaskDecoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://huggingface.co/apple/coreml-sam2.1-tiny/resolve/39ae0a8a83e5e6cd196e804bf7cccc5f8171f306/SAM2_1TinyMaskDecoderFLOAT16.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "sha256": "4601f302d4c6936e15de3a22089c2afe1fa009ef703f82147ff829b4be677577",
            "size": 75167
          },
          {
            "path": "SAM2_1TinyMaskDecoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://huggingface.co/apple/coreml-sam2.1-tiny/resolve/39ae0a8a83e5e6cd196e804bf7cccc5f8171f306/SAM2_1TinyMaskDecoderFLOAT16.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "sha256": "f5a8635981199fa1199007ed6798c61a326288548b74553b3c2ddb932fcdc8de",
            "size": 10222400
          }
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {
          "name": "Apache-2.0",
          "url": "https://www.apache.org/licenses/LICENSE-2.0",
          "tier": "clean_data_encumbered"
        },
        "training_data": "SAM 2.1 was trained by Meta on SA-1B (11 million licensed images with 1.1 billion masks, under the SA-1B Dataset Research License), SA-V (50,900 videos with 642,600 masklets, CC BY 4.0) and Meta's internal licensed video data; the image and video licences are the datasets' own.",
        "inference_bytes": 671088640,
        "packages": [
          "SAM2_1TinyImageEncoderFLOAT16.mlpackage",
          "SAM2_1TinyPromptEncoderFLOAT16.mlpackage",
          "SAM2_1TinyMaskDecoderFLOAT16.mlpackage"
        ]
      },
      {
        "id": "doctored/sky-library",
        "name": "Sky Library (Poly Haven, CC0)",
        "capability": "sky_library",
        "version": "sky-library-1",
        "files": [
          {
            "path": "kloofendal-38d-partly-cloudy.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/kloofendal-38d-partly-cloudy.jpg",
            "sha256": "e453e7f9655499107e1cc4f088d86497a0da6e45272d6ce5a0fbd25e1bd438f6",
            "size": 483407
          },
          {
            "path": "kloofendal-43d-clear.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/kloofendal-43d-clear.jpg",
            "sha256": "8a7dd859a484d73988b54c17e94952eaf19a928c57f7145089376b10ae5dc7f1",
            "size": 239527
          },
          {
            "path": "kloofendal-48d-partly-cloudy.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/kloofendal-48d-partly-cloudy.jpg",
            "sha256": "3d2c8fff07fb068f9f3a713524fc3563c4d50d73454ca8a450964db541672bf5",
            "size": 415412
          },
          {
            "path": "kloppenheim-05.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/kloppenheim-05.jpg",
            "sha256": "117abf06dd8f8d4798cfae5475e7bb4718669e3c1452c2fa1c512ad7bced3fa0",
            "size": 255661
          },
          {
            "path": "sunflowers.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/sunflowers.jpg",
            "sha256": "f7a6467f22229da12f489763818f1083350bc7d0c0e3a522808e4207509b1495",
            "size": 467153
          },
          {
            "path": "rustig-koppie.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/rustig-koppie.jpg",
            "sha256": "22431e2894b54542bc3a2cf1f5b0aa74be7dc58a773d1919d973bfb1c0919d09",
            "size": 255290
          },
          {
            "path": "belfast-sunset.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/belfast-sunset.jpg",
            "sha256": "1d6e252078517d0e670080896d29cf9ad7ae473bf2e3fe81b6051eb082b492b0",
            "size": 318151
          },
          {
            "path": "industrial-sunset.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/industrial-sunset.jpg",
            "sha256": "b5445e704a3ea8cbefe4e191a1e42f49e62f10db5f48c08b23abf0622bd65858",
            "size": 306639
          },
          {
            "path": "qwantani-sunset.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/qwantani-sunset.jpg",
            "sha256": "87b671ca629159638d43e513e73cbf2c4e54c5844d52c937acabf454f96b23c4",
            "size": 245248
          },
          {
            "path": "wasteland-clouds.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/wasteland-clouds.jpg",
            "sha256": "dc1f555dfea54fcdd2576dd66b9335180a60117491f0159674185631079d3317",
            "size": 305122
          },
          {
            "path": "kloppenheim-06.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/kloppenheim-06.jpg",
            "sha256": "0004430bd947b6b1ce0c3a30a06230b3f9a7ad34fb95b5adac0e94e91142c866",
            "size": 421775
          },
          {
            "path": "evening-road-01.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/evening-road-01.jpg",
            "sha256": "a774e056f4cc27d19a8caadcfae5b64b7d555c913c936531c32e1b2d5d6a92de",
            "size": 452198
          },
          {
            "path": "table-mountain-1.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/table-mountain-1.jpg",
            "sha256": "9303cff9724eb4bf5bdd8a54c086366a745cc5c60e52a8c155d7620c7ea102f8",
            "size": 364975
          },
          {
            "path": "table-mountain-2.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/table-mountain-2.jpg",
            "sha256": "82aae768bd435c9812ef5f7bf9897e568073883a0cbe0eeb5e4b1555593a5c6d",
            "size": 317070
          },
          {
            "path": "aristea-wreck.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/aristea-wreck.jpg",
            "sha256": "eb5cec39d0a6595450cb000607acc05d6d458ef37772b19c2988c80db9c52ce1",
            "size": 394233
          },
          {
            "path": "farm-field.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/farm-field.jpg",
            "sha256": "d153ec7b8e82bdc7733bb6c933f5856e7383f859acdd06a8a7df02cdddb0feef",
            "size": 319235
          },
          {
            "path": "kloofendal-overcast.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/kloofendal-overcast.jpg",
            "sha256": "b543ab928bdc0b4fe8386098234ce37f9e3b3487b2e6b66ace5a0f82b38c7d9a",
            "size": 276220
          },
          {
            "path": "kloppenheim-01.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/kloppenheim-01.jpg",
            "sha256": "2cc1f5398e3a26db2369e8a8fd131bcf27b402fc4ea09b4bd969b91928df731a",
            "size": 208196
          },
          {
            "path": "rogland-clear-night.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/rogland-clear-night.jpg",
            "sha256": "3269cb2ffc0ea57558fdd08794ba86a7741bcb8740b82c7dd8f16807ca69d121",
            "size": 1447567
          },
          {
            "path": "kloppenheim-02.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/kloppenheim-02.jpg",
            "sha256": "6a5aeaaebf4b7a086b66652ae87377bfd0b03e733e9aedb11a50a7e292e133ee",
            "size": 906782
          },
          {
            "path": "qwantani-night.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/qwantani-night.jpg",
            "sha256": "677820163253bebcbafaa1773745b1e42f2ffa2daf28bd51b239d45af134a5ab",
            "size": 1062128
          },
          {
            "path": "qwantani-moonrise.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/qwantani-moonrise.jpg",
            "sha256": "6aa1a4a34d4f0ceb7d77a6a0305ba62d502c557038de3d24d451651eb268da5c",
            "size": 480325
          },
          {
            "path": "qwantani-moon-noon.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/qwantani-moon-noon.jpg",
            "sha256": "e25daedaf13cf533cbeae15373b439db1fcdf8fdd66d56d691d0b18f0f080b79",
            "size": 476947
          },
          {
            "path": "kloppenheim-07.jpg",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/kloppenheim-07.jpg",
            "sha256": "11d295f590e5c5d880618a36ab2e5a91621b575be0b18472e3373806443031ec",
            "size": 456898
          },
          {
            "path": "manifest.json",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/manifest.json",
            "sha256": "833a34a0ef1307a275a47a0342c13f9f9336a8ad9528186947ee0d301e567065",
            "size": 6914
          }
        ],
        "runtime": "assets",
        "minimum_macos": "15.0",
        "license": {
          "name": "CC0-1.0",
          "url": "https://creativecommons.org/publicdomain/zero/1.0/",
          "tier": "clean"
        },
        "training_data": "Not a model: 24 sky photographs cropped from Poly Haven's CC0 HDRIs (tone-mapped JPGs); manifest.json names each one's source and author.",
        "inference_bytes": 0,
        "packages": []
      },
      {
        "id": "doctored/migan-512-places2",
        "name": "MI-GAN 512 Places2 (Core ML)",
        "capability": "inpaint",
        "version": "migan-512-places2-coreml-1",
        "files": [
          {
            "path": "migan_512_places2.mlpackage/Manifest.json",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/migan-512-places2-coreml-1/migan_512_places2-Manifest.json",
            "sha256": "9473d4bbbb3785e12b00a94018abc58e62ed82a11d2bf99ec0b3c420104f7037",
            "size": 617
          },
          {
            "path": "migan_512_places2.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/migan-512-places2-coreml-1/migan_512_places2-model.mlmodel",
            "sha256": "22ceb8cfcff0b4c52fbff3d8a989a9114c0ef777ef473ab0983d7e651ccd3b53",
            "size": 210841
          },
          {
            "path": "migan_512_places2.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/migan-512-places2-coreml-1/migan_512_places2-weight.bin",
            "sha256": "ea8aa8a3771c2b02ff84d38b6e9a13635c413bf295fd4449060ba79e74d149fd",
            "size": 13264128
          }
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {
          "name": "MIT",
          "url": "https://github.com/Picsart-AI-Research/MI-GAN/blob/main/LICENSE-WEIGHTS",
          "tier": "clean_data_encumbered"
        },
        "training_data": "Places2 (Places365-Standard, MIT CSAIL; released for non-commercial research and education): Picsart AI Research trained MI-GAN 512 on its scenes at 512×512. Doctored converted the TorchScript export of migan_512_places2 to Core ML (float16 compute) with scripts/convert_migan.py, retraining nothing.",
        "inference_bytes": 342884352,
        "packages": [
          "migan_512_places2.mlpackage"
        ]
      },
      {
        "id": "doctored/realesrgan-x4plus",
        "name": "Real-ESRGAN x4plus (Core ML)",
        "capability": "super_resolution",
        "version": "realesrgan-x4plus-coreml-1",
        "files": [
          {
            "path": "realesrgan_x4plus.mlpackage/Manifest.json",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/realesrgan-x4plus-coreml-1/realesrgan_x4plus-Manifest.json",
            "sha256": "2684abda1cfb9b69f56a8d7fd4ae4a60645359fad24832eaf5e4dce39e42e8e3",
            "size": 617
          },
          {
            "path": "realesrgan_x4plus.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/realesrgan-x4plus-coreml-1/realesrgan_x4plus-model.mlmodel",
            "sha256": "37fb4dec073831768372b58c925cb4d7f57bccbd89500eb470fad33f4c9ad668",
            "size": 655563
          },
          {
            "path": "realesrgan_x4plus.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/realesrgan-x4plus-coreml-1/realesrgan_x4plus-weight.bin",
            "sha256": "b0771b2a3874414cc1bc409300447a3398f8096fd92515b27f348d1137c6db3e",
            "size": 33440896
          }
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {
          "name": "BSD-3-Clause",
          "url": "https://github.com/xinntao/Real-ESRGAN/blob/master/LICENSE",
          "tier": "clean_data_encumbered"
        },
        "training_data": "DF2K (DIV2K and Flickr2K) and OST, with Real-ESRGAN's synthetic second-order degradations (the repository's docs/Training.md); DIV2K is released for academic research. The README states no separate weights licence, so the weights are recorded under the repository's BSD-3-Clause. Doctored converted RealESRGAN_x4plus.pth to Core ML (float16 compute) with scripts/convert_realesrgan.py, retraining nothing.",
        "inference_bytes": 209715200,
        "packages": [
          "realesrgan_x4plus.mlpackage"
        ]
      },
      {
        "id": "doctored/fbcnn-color",
        "name": "FBCNN colour (Core ML)",
        "capability": "jpeg_artifacts",
        "version": "fbcnn-color-coreml-1",
        "files": [
          {
            "path": "fbcnn_color.mlpackage/Manifest.json",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/fbcnn-color-coreml-1/fbcnn_color-Manifest.json",
            "sha256": "daa2df7be6f30a85012346e09c897d1bcd838496f0fd8bbec93ddf7b1eb73a81",
            "size": 617
          },
          {
            "path": "fbcnn_color.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/fbcnn-color-coreml-1/fbcnn_color-model.mlmodel",
            "sha256": "59da75c91548d174fea57346eea1dbb4c8ca5fa01dfc1519548af9d3b1956200",
            "size": 119342
          },
          {
            "path": "fbcnn_color.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/fbcnn-color-coreml-1/fbcnn_color-weight.bin",
            "sha256": "e84039004b0870f1805fe8e423251636f415bfdc80b8d5861a7920f4642b4395",
            "size": 105028608
          }
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {
          "name": "Apache-2.0",
          "url": "https://github.com/jiaxi-jiang/FBCNN/blob/main/LICENSE",
          "tier": "clean_data_encumbered"
        },
        "training_data": "DIV2K and Flickr2K (options/train_fbcnn_color.json's Data_DIV2K_Flickr2K), JPEG-compressed at random quality factors during training (the paper); DIV2K is released for academic research. Doctored converted fbcnn_color.pth to Core ML (float16 compute) with scripts/convert_fbcnn.py, retraining nothing.",
        "inference_bytes": 209715200,
        "packages": [
          "fbcnn_color.mlpackage"
        ]
      },
      {
        "id": "doctored/ddcolor",
        "name": "DDColor-L ModelScope (Core ML)",
        "capability": "colorize",
        "version": "ddcolor-modelscope-coreml-1",
        "files": [
          {
            "path": "ddcolor.mlpackage/Manifest.json",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/ddcolor-modelscope-coreml-1/ddcolor-Manifest.json",
            "sha256": "df88131ded3c248cc114e5d5fe04a49e19367d542478a1d46fcce269fd5d141f",
            "size": 617
          },
          {
            "path": "ddcolor.mlpackage/Data/com.apple.CoreML/model.mlmodel",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/ddcolor-modelscope-coreml-1/ddcolor-model.mlmodel",
            "sha256": "fefce9bc09418058406de25cb813afd1ac111a99bfbc085d06a26aec0d4d6833",
            "size": 362560
          },
          {
            "path": "ddcolor.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
            "url": "https://github.com/kpgalligan/doctored-models/releases/download/ddcolor-modelscope-coreml-1/ddcolor-weight.bin",
            "sha256": "dc28666a5f4343ae00c97ef3d7b2ff8eff197bc6acfdd6a75db1d51b48f940ad",
            "size": 933389376
          }
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {
          "name": "Apache-2.0",
          "url": "https://github.com/piddnad/DDColor/blob/master/LICENSE",
          "tier": "clean_data_encumbered"
        },
        "training_data": "ImageNet (MODEL_ZOO.md: trained on ImageNet), whose terms are non-commercial research. Doctored converted the Hugging Face checkpoint piddnad/ddcolor_modelscope to Core ML (float32 compute) with scripts/convert_ddcolor.py, retraining nothing.",
        "inference_bytes": 1784676352,
        "packages": [
          "ddcolor.mlpackage"
        ]
      }
    ]
    """
}
