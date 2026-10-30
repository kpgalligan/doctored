import AppKit
import ImageIO

// File ▸ Load Files into Stack…: several image files become the raster layers of ONE new
// document. `StackLoad.plan` sizes every file from its header and admits the whole stack before
// anything is decoded; `StackLoad.build` opens the first file the way File ▸ Open does (its depth,
// its profile, the working-space adoption of a byte open) and lands each later file through the
// core's own `place` + `rasterize`, exactly as Layer ▸ Place Embedded… lands a file, so a file of
// another depth or profile is converted by the one conversion Place already has. `ImageDocument
// .makeStack` turns the result into a document the way `makeDuplicate` does. No Rust, FFI or `.rz`
// change: every step is an existing core call.

/// The stack a set of files becomes: probe, plan, build.
enum StackLoad {
    /// One file, sized without decoding it.
    struct Probe {
        let url: URL
        /// The layer name: the file name without its extension.
        let name: String
        let width: Int
        let height: Int
        /// The depth the file is expected to open at: 8, 16 or 32.
        let depth: Int
        /// The file's own decode charge, the figure its open or place admits.
        let transient: UInt64
    }

    /// What `plan` admitted.
    struct Plan {
        let probes: [Probe]
        let canvasWidth: Int
        let canvasHeight: Int
        /// The stack's depth: the first file's.
        let depth: Int
        /// `planBytes` for these files at that depth.
        let bytes: UInt64
    }

    /// One landed layer, bottom-first.
    struct LayerReport {
        let index: Int
        let name: String
        let path: String
        let width: Int
        let height: Int
        /// The depth the file decoded at: 8, 16 or 32.
        let sourceDepth: Int
        /// The file's depth differs from the stack's, so its samples were converted.
        let convertedDepth: Bool
        /// The file's profile differs from the stack's, so its colours were converted.
        let convertedProfile: Bool
        /// What the file's own open reported (a profile it could not keep).
        let disclosures: [String]
    }

    struct Outcome {
        let document: RasterDocument
        let plan: Plan
        let layers: [LayerReport]
        let profileName: String

        /// What the load did to the files' depth and colour, for the UI's
        /// notice and MCP's `note`; nil when no file was converted and
        /// nothing was disclosed.
        var notice: String? {
            let converted = layers.filter { $0.convertedDepth || $0.convertedProfile }
            let disclosed = layers.flatMap { layer in
                layer.disclosures.map { "“\(layer.name)”: \($0)" }
            }
            guard !converted.isEmpty || !disclosed.isEmpty else { return nil }
            var sentences: [String] = []
            if !converted.isEmpty {
                let first = layers.first?.name ?? "the first file"
                let names = converted.map { layer -> String in
                    let what = layer.convertedDepth && layer.convertedProfile
                        ? "depth and colour profile"
                        : layer.convertedDepth ? "depth" : "colour profile"
                    return "“\(layer.name)” (\(what))"
                }
                sentences.append(
                    "The stack is \(StackLoad.depthName(plan.depth)) in \(profileName), as File ▸ "
                        + "Open gives “\(first)”. Converted to match it, as Place converts a "
                        + "placed file: \(names.joined(separator: ", ")).")
            }
            sentences.append(contentsOf: disclosed)
            return sentences.joined(separator: " ")
        }
    }

    static let minimumFiles = 2
    static let remedy = "Load fewer or smaller files."

    static func depthName(_ depth: Int) -> String {
        depth == 32 ? "32 bits/channel" : depth == 16 ? "16 bits/channel" : "8 bits/channel"
    }

    static func bytesPerPixel(_ depth: Int) -> UInt64 {
        depth == 32 ? 16 : depth == 16 ? 8 : 4
    }

    /// What every layer pixel is charged beside the layer's own bytes. At 8
    /// bits it is the place charge, `ImageSourceFile.scratchPerPixel8`:
    /// Load Files into Stack is one place per file, and the first projection
    /// of an N-layer byte document materialises a contiguous view of every
    /// layer (a second Σ wᵢ·hᵢ·4 beside the layers themselves). A 16-bit or
    /// float stack has no such views, but its layers stay resident at their
    /// full size, so a plan that charged a layer only its own bytes would
    /// have no margin on its per-layer term and would sink toward 1.0 × the
    /// peak as layers are added. Its charge is the margin on the measured
    /// cost of one more layer: `typedLayerCharge16` and `typedLayerCharge32`
    /// (`planBytes`' record). Charged per layer pixel, the plan keeps its
    /// margin at every layer count, at every depth.
    static func placeChargePerPixel(_ depth: Int) -> UInt64 {
        depth == 8 ? ImageSourceFile.scratchPerPixel8
            : depth == 16 ? typedLayerCharge16 : typedLayerCharge32
    }

    /// What a 16-bit stack charges each layer pixel beside the layer's own 8
    /// bytes: 11 in all, the least whole number of bytes at or above 1.25 ×
    /// the 8.07 bytes a pixel one more 16-bit layer measured. Until
    /// 2026-10-12 it was the 16-bit place figure, 19, which over-charged a
    /// stack by 16 bytes a layer pixel.
    static let typedLayerCharge16: UInt64 = 3
    /// The float twin, beside the layer's own 16 bytes: 21 in all. One more
    /// float layer measured 15.92 bytes a pixel, and 1.25 × that is 19.9, so
    /// 4 would do; 5 keeps the charge at 1.25 × a slope of up to 16.8. Until
    /// 2026-10-12 it was the float place figure, 60.
    static let typedLayerCharge32: UInt64 = 5

    /// The stack's working-memory plan: every layer at the stack's depth
    /// (Σ wᵢ·hᵢ·bpp), the stack's charge per layer pixel (Σ wᵢ·hᵢ·
    /// `placeChargePerPixel`), the largest single file's decode charge
    /// (`transient`; the files are decoded one after the other, so the
    /// charges never overlap), and the canvas's whole open — its decoded
    /// image, document and first projection — as
    /// `FlatOpenAdmission.openScratch(canvas, encoded: 0)` scaled by bpp/4,
    /// since that figure is fitted at 4 bytes a pixel. Overflow throws.
    ///
    /// MEASUREMENT (2026-10-12; `python3 -B
    /// scripts/stack-admission/measure.py --app
    /// build/Build/Products/Release/Doctored.app`, a fresh process per case
    /// at an 8 GiB working limit, `MallocLargeCache=0`, three runs; 4032 ×
    /// 3024 files unless a size is given; E is the interval's peak
    /// phys_footprint over its start around the one `load_files_into_stack`
    /// call; every baseline twin read E ≤ 49,152):
    ///   jpeg8 × 8 (8-bit): plan 1,934,680,753; E 1,023,165,136 /
    ///     1,023,017,680 / 1,022,984,912; 1.891 ×
    ///   jpeg8 × 16 (8-bit): plan 3,300,270,769; E 1,803,945,232 /
    ///     1,803,830,544 / 1,803,683,088; 1.829 ×
    ///   png16 × 2 (16-bit): plan 1,501,518,094; E 487,457,080 /
    ///     487,260,472 / 487,194,936; 3.080 ×
    ///   png16 × 4 (16-bit): plan 1,769,758,990; E 684,196,296 /
    ///     683,835,848 / 684,163,528; 2.587 ×
    ///   png16 then jpeg8 × 3 (16-bit): plan 1,769,758,990; E 679,559,672 /
    ///     679,707,128 / 679,870,968; 2.603 ×
    ///   png16 × 8 (16-bit): plan 2,306,240,782; E 1,077,363,480 /
    ///     1,077,084,928 / 1,077,232,384; 2.141 ×
    ///   png16 × 16 (16-bit): plan 3,379,204,366; E 1,864,172,864 /
    ///     1,864,156,480 / 1,864,107,352; 1.813 ×
    ///   png16 × 40 at 2016 × 1512 (16-bit): plan 1,662,923,704;
    ///     E 1,113,555,856 / 1,113,293,712 / 1,113,326,456; 1.493 ×
    ///   float scene (48 × 32) then jpeg8 × 1 (float): plan 2,068,833,457;
    ///     E 589,545,928 / 589,496,776 / 590,070,216; 3.506 ×
    ///   float scene then jpeg8 × 5 (float): plan 3,093,025,969;
    ///     E 1,214,858,200 / 1,214,989,248 / 1,215,562,688; 2.545 ×
    ///   float scene then jpeg8 × 7 (float): plan 3,605,122,225;
    ///     E 1,602,929,912 / 1,602,815,200 / 1,602,454,752; 2.249 ×
    ///   float scene then jpeg8 × 15 (float): plan 5,653,507,249;
    ///     E 3,154,217,336 / 3,155,364,216 / 3,155,610,000; 1.792 ×
    /// Each added 8-bit layer raises E by 97.6 MB (8.01 bytes a pixel: the
    /// layer and its view) and the plan by 14 bytes a pixel, so an 8-bit plan
    /// stays above 1.74 × at any layer count. Before the views were charged,
    /// jpeg8 × 8 planned 959,259,313, 0.936 × its peak.
    /// Each added 16-bit layer raises E by 98.3 MB (8.07 bytes a pixel, the
    /// same from 2 to 8 layers as from 8 to 16) and the plan by 11, so a
    /// 16-bit plan falls toward 1.36 × as layers are added and is above it
    /// at every count measured; the forty-layer case, whose whole peak is
    /// 9.13 bytes a layer pixel, is at 1.493 ×. Charged only its own 8 bytes
    /// a pixel, a 16-bit layer left the plan's margin to its fixed terms:
    /// png16 × 40 would have planned 1,297,140,664, 1.165 × its peak, a
    /// miss. Charged the 16-bit place figure, 19, the same plans were
    /// 2,550,096,142 (png16 × 4), 6,500,552,974 (png16 × 16) and
    /// 3,613,766,584 (png16 × 40): 3.7, 3.5 and 3.2 × their peaks.
    /// Each added float layer raises E by 194.1 MB between 8 and 16 layers
    /// (15.92 bytes a pixel; 13.85 between 2 and 8) and the plan by 21. A
    /// float layer is 16 bytes a pixel and stays resident, so the slope
    /// cannot settle under 16, and the plan falls toward about 1.3 × (21
    /// against 16); the sixteen-layer case is at 1.792 ×.
    /// Charged the float place figure, 60, the six-layer case planned
    /// 6,446,121,649, 5.3 × its peak.
    static func planBytes(sizes: [(width: Int, height: Int)], canvas: (width: Int, height: Int),
                          depth: Int, transient: UInt64) throws -> UInt64 {
        let bpp = bytesPerPixel(depth)
        let perPixel = try PrecisionDocumentDescriptor.sum(bpp, placeChargePerPixel(depth))
        var total = transient
        for size in sizes {
            let (pixels, overflow) = UInt64(max(size.width, 0)).multipliedReportingOverflow(
                by: UInt64(max(size.height, 0)))
            let (bytes, bytesOverflow) = pixels.multipliedReportingOverflow(by: perPixel)
            guard size.width > 0, size.height > 0, !overflow, !bytesOverflow else {
                throw DoctoredCoreError(message: "Working-memory calculation overflow.")
            }
            total = try PrecisionDocumentDescriptor.sum(total, bytes)
        }
        let open = try FlatOpenAdmission.openScratch(
            width: canvas.width, height: canvas.height, encoded: 0)
        let (scaled, scaledOverflow) = open.multipliedReportingOverflow(by: bpp / 4)
        guard !scaledOverflow else {
            throw DoctoredCoreError(message: "Working-memory calculation overflow.")
        }
        return try PrecisionDocumentDescriptor.sum(total, scaled)
    }

    /// Sizes every file from its header, refuses what cannot be planned and
    /// admits the whole stack — before anything is decoded. Main thread.
    static func plan(_ urls: [URL]) throws -> Plan {
        assert(Thread.isMainThread)
        guard urls.count >= minimumFiles else {
            throw DoctoredCoreError(
                message: "Load Files into Stack needs at least \(minimumFiles) files.")
        }
        let probes = try urls.map(probe)
        let width = probes.map(\.width).max() ?? 0
        let height = probes.map(\.height).max() ?? 0
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(
                message: "The stack's canvas would be \(width) × \(height) pixels, past the "
                    + "100-megapixel limit. \(remedy)")
        }
        let depth = probes.first?.depth ?? 8
        let plan = try planned(probes, canvas: (width, height), depth: depth)
        _ = try RasterResources.shared.admit(scratch: plan.bytes, remedy: remedy)
        return plan
    }

    private static func planned(_ probes: [Probe], canvas: (width: Int, height: Int),
                                depth: Int) throws -> Plan {
        let bytes = try planBytes(
            sizes: probes.map { (width: $0.width, height: $0.height) }, canvas: canvas,
            depth: depth, transient: probes.map(\.transient).max() ?? 0)
        return Plan(probes: probes, canvasWidth: canvas.width, canvasHeight: canvas.height,
                    depth: depth, bytes: bytes)
    }

    /// One file's size, expected depth and decode charge, from its header.
    /// The order is `ImageDocument.openDocument(at:)`'s: camera RAW, a host
    /// still or movie, a PDF or SVG, then everything ImageIO can size.
    private static func probe(_ url: URL) throws -> Probe {
        let file = url.lastPathComponent
        let name = url.deletingPathExtension().lastPathComponent
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw DoctoredCoreError(message: "“\(file)” was not found at \(url.path).")
        }
        // Nothing sizes a native document before decoding it, so it cannot be
        // planned.
        guard !ImageDocument.hasNativeSignature(at: url) else {
            throw DoctoredCoreError(
                message: "“\(file)” is a Doctored document, which Load Files into Stack "
                    + "cannot load: its size is not known before it is decoded, so the stack "
                    + "cannot be planned. Export it as an image first.")
        }
        if RawImage.isRawFile(url), let raw = RawImage.inspect(url: url) {
            let width = Int(raw.pixelSize.width), height = Int(raw.pixelSize.height)
            let transient = try RawSourceWork.scratch(
                width: width, height: height, encoded: RawSourceSnapshot.size(url))
            return Probe(url: url, name: name, width: width, height: height, depth: 16,
                         transient: transient)
        }
        if HostSourceInput.claims(url) {
            let input = try HostSourceInput(url: url)
            guard let entry = input.entries.first(where: { $0.role == "still" })
                ?? input.entries.first else {
                throw unsizeable(file)
            }
            return Probe(url: url, name: name, width: entry.width, height: entry.height,
                         depth: (entry.depth ?? 8) > 8 ? 16 : 8, transient: input.scratch)
        }
        let size: (Int, Int)
        if VectorImport.claims(url) {
            size = try VectorImport.pixelSize(VectorImport.inspect(url), choice: VectorImportChoice())
        } else {
            guard let measured = Bitmap.imageSize(url) else { throw unsizeable(file) }
            size = (Int(measured.width), Int(measured.height))
        }
        guard size.0 > 0, size.1 > 0 else { throw unsizeable(file) }
        let depth = PrecisionImportRequest.isFloatImage(url) ? 32
            : (imageIODepth(url) ?? 8) > 8 ? 16 : 8
        let transient: UInt64
        if let source = try? ImageSourceFile(url: url) {
            transient = source.scratchEstimate
        } else {
            let encoded = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? 0
            transient = try FlatOpenAdmission.openScratch(
                width: size.0, height: size.1, encoded: UInt64(max(encoded, 0)))
        }
        return Probe(url: url, name: name, width: size.0, height: size.1, depth: depth,
                     transient: transient)
    }

    private static func unsizeable(_ file: String) -> DoctoredCoreError {
        DoctoredCoreError(
            message: "“\(file)” is not an image Load Files into Stack can size before opening it.")
    }

    /// The file's bits per sample as ImageIO's header probe reports it (no decode).
    private static func imageIODepth(_ url: URL) -> Int? {
        guard let source = CGImageSourceCreateWithURL(
                  url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary),
              let info = CGImageSourceCopyPropertiesAtIndex(
                  source, 0, [kCGImageSourceShouldCache: false] as CFDictionary) as? [CFString: Any]
        else { return nil }
        return (info[kCGImagePropertyDepth] as? NSNumber)?.intValue
    }

    /// `ImageDocument.openDocument(at:)` with every import choice answered
    /// headlessly, so no dialog appears between files: a camera RAW takes the
    /// development `raw` (the default one unless the caller gives another, as
    /// Merge to HDR does), a high-depth file keeps its depth, a PSD or
    /// layered TIFF opens as its composite and a PDF or SVG at the default
    /// page and resolution. Each mode is restored on the way out
    /// (`AgentServer.openDocument`'s pattern).
    static func opened(_ url: URL, raw: RawDevelopSettings = RawDevelopSettings()) throws
        -> ImageDocument.Opened
    {
        let previousRaw = RawImportRequest.mode
        RawImportRequest.mode = .headless(raw)
        defer { RawImportRequest.mode = previousRaw }
        let previousPrecision = PrecisionImportRequest.mode
        PrecisionImportRequest.mode = .preserve
        defer { PrecisionImportRequest.mode = previousPrecision }
        let previousPSD = PSDImportRequest.mode
        PSDImportRequest.mode = .explicit(.composite, fingerprint: nil)
        defer { PSDImportRequest.mode = previousPSD }
        let previousVector = VectorImportRequest.mode
        VectorImportRequest.mode = .headless(VectorImportChoice())
        defer { VectorImportRequest.mode = previousVector }
        // An HDR photo enters a stack as its SDR base (a gain map) or its
        // refusal (PQ/HLG), as before v1.x: the stack's depth plan is
        // 8 or 16 bits.
        let previousHDR = HDRPhotoRequest.mode
        HDRPhotoRequest.mode = .sdr
        defer { HDRPhotoRequest.mode = previousHDR }
        return try ImageDocument.openDocument(at: url)
    }

    /// `opened` as ONE ordinary raster, the shape the core's source input
    /// takes (`SourceInput::asset`): every retained source (a camera RAW's
    /// development) is rasterized, and a document left with more than one
    /// layer, or whose lone layer carries a mask or a description, is
    /// flattened.
    static func plainRaster(_ opened: RasterDocument) throws -> RasterDocument {
        var current = opened
        for layer in 0..<opened.layerCount where current.hasRetainedSource(layer) {
            let request = try PrecisionPixelCommand(["op": "rasterize", "layer": layer]).request
            if let raster = try current.editingRetainedSource(request: request) {
                current = raster
            }
        }
        if current.layerCount > 1 || current.layerHasMask(0) || current.layerMeta(0) != nil {
            current = try current.flattening() ?? current
        }
        return current
    }

    private static func depth(of document: RasterDocument) -> Int {
        document.isFloat ? 32 : document.isHighDepth ? 16 : 8
    }

    /// Plans, then decodes the files one after the other into the stack.
    /// Nothing is published here: a refusal or a cancel leaves no document.
    /// A camera RAW among the files is developed with `raw`. Main thread.
    static func build(_ urls: [URL], raw: RawDevelopSettings = RawDevelopSettings()) throws
        -> Outcome
    {
        assert(Thread.isMainThread)
        var admitted = try plan(urls)
        var reports: [LayerReport] = []
        var stack: RasterDocument?
        for (index, probe) in admitted.probes.enumerated() {
            let file = probe.url.lastPathComponent
            do {
                let source = try opened(probe.url, raw: raw)
                let raster = try plainRaster(source.doc)
                let disclosures = [source.profileRefusal].compactMap { $0 }
                // A file that decoded at another size than its header promised
                // re-plans the stack on the real sizes, before the first layer
                // is built on the plan's canvas; the running stack then takes
                // the recomputed canvas, larger or smaller, before the file
                // lands.
                admitted = try replanned(admitted, decoded: raster, at: index,
                                         depth: stack.map(depth(of:)) ?? admitted.depth)
                if let running = stack, running.width != admitted.canvasWidth
                    || running.height != admitted.canvasHeight {
                    guard let resized = try running.canvasResized(
                        w: admitted.canvasWidth, h: admitted.canvasHeight, originX: 0, originY: 0)
                    else {
                        throw DoctoredCoreError(
                            message: "the stack's canvas could not be resized to "
                                + "\(admitted.canvasWidth) × \(admitted.canvasHeight) pixels.")
                    }
                    stack = resized
                }
                guard let current = stack else {
                    let first = try firstLayer(raster, opened: source, probe: probe, plan: admitted)
                    // A file that opened at another depth than its header
                    // promised is planned again at the real one — every depth
                    // has its own bytes per pixel and its own place charge —
                    // and a larger plan is admitted before the next decode.
                    let real = depth(of: first)
                    if real != admitted.depth {
                        let canvas = (width: admitted.canvasWidth, height: admitted.canvasHeight)
                        let replanned = try planned(admitted.probes, canvas: canvas, depth: real)
                        if replanned.bytes > admitted.bytes {
                            _ = try RasterResources.shared.admit(
                                scratch: replanned.bytes, remedy: remedy)
                        }
                        admitted = replanned
                    }
                    reports.append(LayerReport(
                        index: 0, name: probe.name, path: probe.url.path,
                        width: raster.width, height: raster.height, sourceDepth: depth(of: raster),
                        convertedDepth: false,
                        convertedProfile: raster.iccProfile != first.iccProfile,
                        disclosures: disclosures))
                    stack = first
                    continue
                }
                let landed = try placing(raster, named: probe.name, onto: current)
                reports.append(LayerReport(
                    index: index, name: probe.name, path: probe.url.path,
                    width: raster.width, height: raster.height, sourceDepth: depth(of: raster),
                    convertedDepth: depth(of: raster) != depth(of: current),
                    convertedProfile: raster.iccProfile != current.iccProfile,
                    disclosures: disclosures))
                stack = landed
            } catch let cancel as JobCancelled {
                throw cancel
            } catch let error as CocoaError where error.code == .userCancelled {
                throw error
            } catch {
                throw DoctoredCoreError(
                    message: "“\(file)” could not be added to the stack: "
                        + error.localizedDescription)
            }
        }
        guard let document = stack else {
            throw DoctoredCoreError(message: "No file could be loaded into the stack.")
        }
        return Outcome(document: document, plan: admitted, layers: reports,
                       profileName: document.profileName)
    }

    /// `plan` after file `index` decoded as `raster`: unchanged when the
    /// decoded size is the probe's. Otherwise the probe takes the real size,
    /// the canvas becomes the largest width and the largest height over the
    /// real sizes of the files decoded so far and the probes of the rest, the
    /// stack is planned again at `depth`, and a larger plan is admitted.
    ///
    /// The known cause is a host still (HEIC, AVIF, JPEG XL, JPEG 2000)
    /// stored turned: `HostStillDecoder.inspect` reports the STORED width and
    /// height, and an orientation of 5–8 swaps them in the decode
    /// (`RasterDocument.fromHostPixels(orientation:)`). Nothing here is
    /// specific to that cause: any decode that differs from its probe takes
    /// the same path.
    private static func replanned(_ plan: Plan, decoded raster: RasterDocument, at index: Int,
                                  depth: Int) throws -> Plan {
        guard plan.probes.indices.contains(index) else { return plan }
        let probe = plan.probes[index]
        guard raster.width != probe.width || raster.height != probe.height else { return plan }
        var probes = plan.probes
        probes[index] = Probe(url: probe.url, name: probe.name, width: raster.width,
                              height: raster.height, depth: probe.depth,
                              transient: probe.transient)
        let canvas = (width: probes.map(\.width).max() ?? raster.width,
                      height: probes.map(\.height).max() ?? raster.height)
        let next = try planned(probes, canvas: canvas, depth: depth)
        if next.bytes > plan.bytes {
            _ = try RasterResources.shared.admit(scratch: next.bytes, remedy: remedy)
        }
        return next
    }

    /// The stack's bottom layer: the first file as File ▸ Open leaves it (a
    /// byte open that is not a PSD import adopts the working space, the rule
    /// of `ImageDocument.read`), named after its file, on the stack's canvas.
    private static func firstLayer(_ raster: RasterDocument, opened: ImageDocument.Opened,
                                   probe: Probe, plan: Plan) throws -> RasterDocument {
        var first = raster
        if first.precision == nil, opened.psdImport == nil,
           let adopted = first.adoptingWorkingSpace(ColorSettings.workingSpace.profileData).document {
            first = adopted
        }
        first = try first.withLayerName(0, probe.name) ?? first
        if plan.canvasWidth > first.width || plan.canvasHeight > first.height {
            first = try first.canvasResized(
                w: max(plan.canvasWidth, first.width), h: max(plan.canvasHeight, first.height),
                originX: 0, originY: 0) ?? first
        }
        return first
    }

    /// One later file landed on top of `stack` at the canvas's top-left,
    /// unscaled: the core's `place` (which renders the file into the stack's
    /// profile and depth, `convert_fallback`) and the `rasterize` of what it
    /// placed — `LinkedImageWork.importImage`'s pair. Each runs in its own
    /// cancellable job with its own plan (`RetainedImageWork.edit`'s rule: a
    /// control starts once and its reservation is the latest figure, not a
    /// sum), so the rasterize is never held to the place's scratch limit,
    /// which refused it on a small canvas. On a Grayscale
    /// stack (#547 phase 6b) the file is first converted into the stack's Gray
    /// profile and depth (`GrayLayerClip.converted`, Convert), so the place
    /// lands R = G = B; that place keeps no source, so the rasterize is skipped.
    private static func placing(_ file: RasterDocument, named name: String,
                                onto stack: RasterDocument) throws -> RasterDocument {
        let asset = stack.isGray
            ? try GrayLayerClip.converted(file, into: stack, mode: .convert)?.clip ?? file
            // #547 Lab phase 6b-i: a Lab stack converts the file by the Mode rules first.
            : stack.isLab
            ? try LabLayerClip.converted(file, into: stack, mode: .convert)?.clip ?? file : file
        let top = max(stack.layerCount - 1, 0)
        let landing = LayerInsertPosition.above.landingIndex(in: stack, anchor: top)
        let id = UUID().uuidString
        let place = try PrecisionPixelCommand(["op": "place", "source_id": id, "revision_id": id,
            "depth_conversion": ImageSourceDepthConversion.convertFallback.rawValue,
            "name": name, "x": 0, "y": 0,
            "anchor": top, "position": LayerInsertPosition.above.rawValue]).request
        let rasterize = try PrecisionPixelCommand(["op": "rasterize", "layer": landing]).request
        let placePlan = try stack.retainedSourcePlan(asset: asset, request: place)
        let placeScratch = try PrecisionDocumentDescriptor.integer(placePlan, "scratch_bytes")
        let placed = try PrecisionCapturedWork.run(
            scratchEstimate: placeScratch, title: "Load Files into Stack"
        ) { control -> RasterDocument in
            guard let edited = try stack.editingRetainedSource(
                asset: asset, request: place, control: control) else {
                throw DoctoredCoreError(message: "the file could not be placed.")
            }
            return edited
        }
        // A Grayscale place keeps no retained source (#547, the core's Gray arm): the placed
        // layer is already plain folded pixels, so there is nothing to rasterize.
        if stack.isGray { return placed }
        let rasterizePlan = try placed.retainedSourcePlan(request: rasterize)
        let rasterizeScratch = try PrecisionDocumentDescriptor.integer(rasterizePlan, "scratch_bytes")
        return try PrecisionCapturedWork.run(
            scratchEstimate: rasterizeScratch, title: "Load Files into Stack"
        ) { control -> RasterDocument in
            guard let raster = try placed.editingRetainedSource(
                request: rasterize, control: control) else {
                throw DoctoredCoreError(message: "the placed file could not be rasterized.")
            }
            return raster
        }
    }
}

extension ImageDocument {
    /// The new document, not yet shown: `makeDuplicate`'s shape around the
    /// built stack — native `fileType`, dirty through `countEditChange`, its
    /// history "Initial State" with no undo step, its active layer the
    /// topmost. A byte stack first admits its first projection beside its own
    /// retained bytes, as an open whose charge omitted it does.
    static func makeStack(_ outcome: StackLoad.Outcome, name: String?) throws -> ImageDocument {
        let stack = outcome.document
        if stack.precision == nil {
            _ = try RasterResources.shared.admit(
                scratch: FlatOpenAdmission.firstProjectionBytes(stack, adopts: false),
                own: stack.ownRetainedBytes(), remedy: StackLoad.remedy)
        }
        let document = ImageDocument()
        document.doc = stack
        document.activeLayerIndex = max(0, stack.layerCount - 1)
        document.refreshProjection()
        document.fileType = Self.nativeTypeIdentifier
        if let name = name?.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty {
            document.displayName = name
        }
        document.countEditChange(.changeDone)
        return document
    }

    /// `StackLoad.build`, `makeStack`, then the window, as
    /// `AppDelegate.newDocument` shows a new document.
    @discardableResult
    static func openStack(_ urls: [URL], name: String?) throws
        -> (document: ImageDocument, outcome: StackLoad.Outcome)
    {
        let outcome = try StackLoad.build(urls)
        let document = try makeStack(outcome, name: name)
        NSDocumentController.shared.addDocument(document)
        document.makeWindowControllers()
        document.showWindows()
        return (document, outcome)
    }
}
