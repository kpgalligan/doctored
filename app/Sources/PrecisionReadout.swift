import AppKit

/// Values from the authoritative typed projection. Display bins and CSS/hex
/// strings never stand in for these source measurements.
struct PrecisionReadout {
    let json: [String: Any]
    let x: Int
    let y: Int
    let normalized: [Double]
    let codes: [Double]

    init(_ json: [String: Any]) throws {
        guard let x = (json["x"] as? NSNumber)?.intValue, let y = (json["y"] as? NSNumber)?.intValue,
              let normalized = json["normalized_rgba"] as? [Double], normalized.count == 4,
              let codes = json["mean_rgba16"] as? [Double], codes.count == 4,
              normalized.allSatisfy({ $0.isFinite && (0...1).contains($0) }) else {
            throw DoctoredCoreError(message: "Invalid authoritative 16-bit sample.")
        }
        self.json = json; self.x = x; self.y = y; self.normalized = normalized; self.codes = codes
    }

    var rgbText: String { codes.map { $0 == $0.rounded() ? String(Int($0)) : String(format: "%.5f", $0) }.joined(separator: ", ") + " / 65535" }
    var exactText: String { normalized.map { String(format: "%.9g", $0) }.joined(separator: ", ") }

    func color(in space: NSColorSpace) -> NSColor {
        NSColor(colorSpace: space, components: normalized.map { CGFloat($0) }, count: 4)
    }

    /// The authoritative sample at one canvas point, averaging the
    /// (2·reach+1) square about it with out-of-bounds pixels dropped (the
    /// core's `dr_precision_image_sample`). nil when there is no document, or
    /// when no pixel of the block is on the canvas — checked before any
    /// render. Everything else throws: a stored projection error, a render
    /// error, or the core's sample refusal.
    static func at(_ point: (x: Int, y: Int), document: ImageDocument, reach: Int = 0) throws -> Self? {
        guard let doc = document.doc, doc.sampleBlockTouchesCanvas(point, reach: reach) else { return nil }
        let source = try document.currentPrecisionProjection()
        return try at(point, projection: source, reach: reach)
    }

    /// The same read over a projection the caller already holds — the Color
    /// Samplers' committed list and a 16-bit sheet's preview. nil when
    /// no pixel of the block is on the projection.
    static func at(_ point: (x: Int, y: Int), projection: RasterPrecisionImage,
                   reach: Int) throws -> Self? {
        guard point.x >= -reach, point.x < projection.width + reach,
              point.y >= -reach, point.y < projection.height + reach else { return nil }
        return try Self(projection.precisionSample(x: point.x, y: point.y, reach: reach))
    }
}

extension RasterDocument {
    /// Any pixel of the (2·reach+1)² block centred on `point` is on the
    /// canvas (the core samplers' "outside dropped" rule).
    func sampleBlockTouchesCanvas(_ point: (x: Int, y: Int), reach: Int) -> Bool {
        point.x >= -reach && point.x < width + reach && point.y >= -reach && point.y < height + reach
    }
}

extension AgentServer {
    func precisionSample(_ a: [String: Any], document: ImageDocument) throws -> String {
        // #547 Lab phase 1c: a Lab Color document's L*, a*, b* beside its codes.
        if document.doc?.isLab == true { return try jsonResult(labSampleReport(a, document: document)) }
        // A 32-bit float document answers the float schema through the same
        // scoped path.
        return try scopedSample(a, document: document)
    }

    /// The scoped sample of a Lab Color document with `LabDocument.readingFields` merged in
    /// (`LabDocument.withReading`, shared with the Color Samplers' rows).
    private func labSampleReport(_ a: [String: Any], document: ImageDocument) throws -> [String: Any] {
        LabDocument.withReading(try scopedSampleReport(a, document: document))
    }

    /// 16-bit `sample_color`: the scoped report plus
    /// `srgb_rgba`/`srgb_in_gamut` read from `normalized_rgba`. A 32-bit
    /// float document answers the float schema unchanged.
    func precisionSampleColor(_ a: [String: Any], document: ImageDocument) throws -> String {
        // #547 Lab phase 1c: no sRGB reading — the document's colour space is the P3 view.
        if document.doc?.isLab == true { return try jsonResult(labSampleReport(a, document: document)) }
        var report = try scopedSampleReport(a, document: document)
        if let doc = document.doc, !doc.isFloat,
           let normalized = report["normalized_rgba"] as? [Double], normalized.count == 4,
           let reading = SRGBReading(documentComponents: normalized, in: document.nsColorSpace) {
            report.merge(reading.fields) { _, new in new }
            if !reading.inGamut {
                report["note"] = [report["note"] as? String, SRGBReading.outOfGamutNote]
                    .compactMap { $0 }.joined(separator: " ")
            }
        }
        // #547 phase 1c: an 8/16-bit Grayscale document's K %, the Info panel's K row.
        if let doc = document.doc, doc.isGray, !doc.isFloat,
           let normalized = report["normalized_rgba"] as? [Double], normalized.count == 4 {
            report["gray_k_percent"] = GrayDocument.kPercent(normalized[0])
        }
        return try jsonResult(report)
    }

    func precisionHistogram(_ a: [String: Any], document: ImageDocument) throws -> String {
        // #547 Lab phase 2d: `source scalar` of Lightness, a or b reads the plane of the codes
        // that holds it (L* red, a* green, b* blue; `paintTarget`'s Lab names since phase 2e),
        // renamed (`LabDocument.scalarReport`); an RGB plane name refuses; the alpha plane (phase
        // 2e), a mask or alpha channel passes through unchanged.
        if document.doc?.isLab == true, stringArg(a, "source") == "scalar",
           let name = stringArg(a, "target") {
            if LabDocument.plane(named: name) != nil {
                return try jsonResult(LabDocument.scalarReport(
                    nativeScalarHistogramReport(a, document: document), channel: name))
            }
            if ["red", "green", "blue"].contains(name) {
                throw ToolError(message: LabDocument.scalarTargetRefusal)
            }
        }
        // #547 Lab phase 1c: a Lab Color document's Lightness, a and b rows; a scalar (mask or
        // alpha channel) report is coverage and passes through unchanged. The read is
        // `scopedHistogram`'s own.
        if document.doc?.isLab == true, stringArg(a, "source") != "scalar" {
            guard a["target"] == nil else { throw ToolError(message: "Histogram target requires source scalar.") }
            let stride = intArg(a, "stride") ?? 1
            guard stride > 0, stride <= Int(UInt32.max) else { throw ToolError(message: "Histogram stride must be a positive uint32.") }
            let read = try ScopedAnalysisRead(source: document.doc, scope: analysisScope(a, document: document),
                selection: (boolArg(a, "selection") ?? true) ? document.canvasSelection : nil, stride: stride,
                projection: document.retainedPrecisionProjection,
                byteProjection: document.retainedByteProjection)
            return try jsonResult(LabDocument.histogramReport(read.run()))
        }
        // #547 phase 6b: a Grayscale document's one Gray row (R = G = B, `GrayDocument.
        // histogramReport`); a scalar (mask or alpha channel) report passes through unchanged.
        if document.doc?.isGray == true, stringArg(a, "source") != "scalar" {
            guard a["target"] == nil else { throw ToolError(message: "Histogram target requires source scalar.") }
            let stride = intArg(a, "stride") ?? 1
            guard stride > 0, stride <= Int(UInt32.max) else { throw ToolError(message: "Histogram stride must be a positive uint32.") }
            let read = try ScopedAnalysisRead(source: document.doc, scope: analysisScope(a, document: document),
                selection: (boolArg(a, "selection") ?? true) ? document.canvasSelection : nil, stride: stride,
                projection: document.retainedPrecisionProjection,
                byteProjection: document.retainedByteProjection)
            return try jsonResult(GrayDocument.histogramReport(read.run()))
        }
        // A 32-bit float document answers the float schema through the same
        // scoped path; `source: "scalar"` reaches `nativeScalarHistogram`,
        // whose mask/channel planes are u16 coverage at every depth.
        return try scopedHistogram(a, document: document)
    }
}

extension EditorViewController {
    func samplePrecisionColor(at point: CGPoint) {
        guard let document else { return }
        let reach = [0, 1, 2][min(max(ToolOptionsStore.shared.sample.sampleSizeIndex, 0), 2)]
        let position = (x: Int(floor(point.x)), y: Int(floor(point.y)))
        if document.doc?.isFloat == true {
            sampleFloatColor(at: position, document: document, reach: reach, point: point)
            return
        }
        let sample: PrecisionReadout
        do {
            // The composite, or the active layer alone (Sample from).
            guard document.doc?.sampleBlockTouchesCanvas(position, reach: reach) == true,
                  let read = try PrecisionReadout.at(
                      position, projection: eyedropperProjection(document), reach: reach)
            else { return }
            sample = read
        } catch {
            // Read failures beep on the press.
            eyedropperReadFailed()
            return
        }
        // #547 Lab phase 2e: a Lab document's sample is an L*a*b* ink, not a colour in the view.
        if document.doc?.isLab == true {
            pickLabSample(sample, at: point)
            return
        }
        pickSample(sample.color(in: document.nsColorSpace), text: sample.exactText, at: point)
    }

    /// A 32-bit float document: the block mean, linear light, into
    /// the foreground through `ExtendedRangeColor.eyedropped` (above white
    /// kept); its numbers are the sample text.
    private func sampleFloatColor(at position: (x: Int, y: Int), document: ImageDocument,
                                  reach: Int, point: CGPoint) {
        let sample: FloatReadout
        do {
            guard document.doc?.sampleBlockTouchesCanvas(position, reach: reach) == true,
                  let read = try FloatReadout.at(
                      position, projection: eyedropperProjection(document), reach: reach)
            else { return }
            sample = read
        } catch {
            eyedropperReadFailed()
            return
        }
        guard let doc = document.doc, let color = ExtendedRangeColor.eyedropped(sample, in: doc) else {
            eyedropperReadFailed()
            return
        }
        // The picked colour goes through `pickSample`
        // (EditorViewController+Eyedropper.swift), shared with the 8-bit path.
        pickSample(color, text: sample.meanText, at: point)
    }
}
