import AppKit

// The Info source/selection controls invalidate queued cursor results on target or selection
// changes even when the document owner and bounds are unchanged.

extension InfoPanelViewController {
    func setPrecisionCursor(_ point: (x: Int, y: Int)) {
        // #1 Indexed phase 1a: the palette index under the cursor (the core's plane, one FFI
        // read), beside the colour the readouts below report; "—" off the canvas.
        if let doc = document?.doc, doc.isIndexed, isViewLoaded {
            valueLabels[.index]?.stringValue = doc.precision?.indexedIndex(x: point.x, y: point.y)
                .map(String.init) ?? "—"
        }
        if scopedCursorRequired { requestScopedCursor(point); return }
        // A 32-bit FLOAT document has no codes to report: its readout is
        // linear components, stops, and above-white/below-zero indication
        // (`FloatDocument.swift`).
        if document?.doc?.isFloat == true {
            setFloatCursor(point)
            return
        }
        guard let document else { return }
        let reading: PrecisionReadout
        do {
            // nil is off the canvas: the readout freezes, like `setCursor(nil)`.
            guard let read = try PrecisionReadout.at(point, document: document) else { return }
            reading = read
        } catch {
            valueLabels[.gray]?.stringValue = "—"
            setCursorFailure(point, message: error.localizedDescription)
            return
        }
        clearReadoutFailure()
        lastPixel = point
        lastReadout = nil
        guard isViewLoaded else { return }
        valueLabels[.position]?.stringValue = "\(point.x), \(point.y)"
        // #1 Lab phase 1c: a Lab Color document's samples are L*a*b* codes, never RGB.
        if document.doc?.isLab == true {
            showLabReading(reading.normalized)
            updateFootnote()
            return
        }
        valueLabels[.lab]?.toolTip = nil
        // #1 Bitmap phase 2a-ii: a Bitmap pixel reads as Grayscale's K (0 % white, 100 % black).
        let grayK = document.doc.map { $0.isGray || $0.isBitmap } == true
        showGrayK(grayK ? reading.normalized[0] : nil)
        valueLabels[.rgb]?.stringValue = reading.rgbText
        valueLabels[.rgb]?.toolTip = "Normalized RGBA: " + reading.exactText
        // A byte-derived Lab/HSB reading would pretend to be more precise
        // than its input. Exact document samples remain available above.
        valueLabels[.hsb]?.stringValue = "—"
        valueLabels[.lab]?.stringValue = "—"
        // #1: a CMYK document's own ink under the cursor; an RGB one's colour through the
        // working CMYK (phase 5).
        showCmykInk(at: point, rgb: document.doc?.isCmyk == true ? nil : reading.normalized)
        updateFootnote()
    }

    func requestScopedHistogram(scope: HistogramSource) {
        precisionHistogramControl?.cancel()
        guard let document, let doc = document.doc else { binsArrived(nil); return }
        do {
            let stride = Histogram.liveStride(pixels: doc.width * doc.height, width: doc.width)
            lastStride = stride
            let read = try ScopedAnalysisRead(source: doc, scope: scope,
                selection: analysisUsesSelection ? selectionProvider?() : nil, stride: stride, preview: true,
                projection: document.retainedPrecisionProjection,
                byteProjection: document.retainedByteProjection)
            precisionHistogramControl = read.control
            loader.request {
                ScopedAnalysisRead.bins(try HistogramLoader.reading(under: read.control) { try read.read() })
            }
        } catch {
            // Superseded first, so no older debounced or in-flight scan can
            // land afterwards and replace the reason.
            loader.supersede()
            histogramFailed(error.localizedDescription)
        }
        updateFootnote()
    }

    /// Cursor scopes share the existing debounced statistics worker. Raw layer
    /// reads inspect at most25 source codes; backdrop rendering has the same
    /// admitted full-frame policy as its histogram. It never blocks mouseMoved.
    func requestScopedCursor(_ point: (x: Int, y: Int)) {
        // Off the canvas the readout freezes, like `setCursor(nil)`.
        guard let doc = document?.doc, doc.sampleBlockTouchesCanvas(point, reach: 0) else { return }
        lastPixel = point
        scopedReadoutControl?.cancel()
        scopedReadoutGeneration &+= 1
        let generation = scopedReadoutGeneration
        guard let document, let source = document.doc else { return }
        do {
            let read = try ScopedAnalysisRead(source: source, scope: selectedAnalysisScope,
                selection: analysisUsesSelection ? selectionProvider?() : nil, point: point, preview: true,
                projection: document.retainedPrecisionProjection,
                byteProjection: document.retainedByteProjection)
            scopedReadoutControl = read.control
            // A failed read names its reason in the footnote — including the
            // core's "no positively weighted pixels" for a cursor outside a
            // weighted selection.
            readoutLoader.onFailure = { [weak self, weak document] message in
                guard let self, self.scopedReadoutGeneration == generation,
                      document?.doc === source else { return }
                self.showGrayK(nil)
                self.showReadoutFailure(point, message: message)
            }
            readoutLoader.onReport = { [weak self, weak document] report in
                guard let self, self.scopedReadoutGeneration == generation,
                      document?.doc === source else { return }
                // A 32-bit float document answers the float schema:
                // linear components with stops, as the composite readout.
                if let report, report["storage"] as? String == "rgba32_float" {
                    guard let reading = try? FloatReadout(report) else {
                        self.valueLabels[.position]?.stringValue = "\(point.x), \(point.y)"
                        self.clearReadoutFailure()
                        self.valueLabels[.rgb]?.stringValue = "—"
                        return
                    }
                    self.showFloatReading(reading, at: point)
                    return
                }
                self.valueLabels[.position]?.stringValue = "\(point.x), \(point.y)"
                guard let report, let codes = report["mean_rgba_codes"] as? [Double],
                      let normalized = report["normalized_rgba"] as? [Double],
                      let maximum = report["maximum_code"] as? NSNumber else {
                    self.clearReadoutFailure()
                    self.valueLabels[.rgb]?.stringValue = "—"
                    self.showGrayK(nil)
                    return
                }
                self.clearReadoutFailure()
                if source.isLab, normalized.count == 4 {
                    self.showLabReading(normalized)
                    self.updateFootnote()
                    return
                }
                self.valueLabels[.lab]?.toolTip = nil
                let grayK = source.isGray || source.isBitmap
                self.showGrayK(grayK && !normalized.isEmpty ? normalized[0] : nil)
                self.valueLabels[.rgb]?.stringValue = codes.map { String(format: "%.9g", $0) }.joined(separator: ", ") + " / \(maximum)"
                self.valueLabels[.rgb]?.toolTip = "Normalized RGBA: " + normalized.map { String(format: "%.12g", $0) }.joined(separator: ", ")
                self.valueLabels[.hsb]?.stringValue = "—"
                self.valueLabels[.lab]?.stringValue = "—"
                self.updateFootnote()
            }
            readoutLoader.requestReport { try HistogramLoader.reading(under: read.control) { try read.read() } }
        } catch {
            showGrayK(nil)
            showReadoutFailure(point, message: error.localizedDescription)
        }
    }

    /// The extended-range cursor readout of the composite (the synchronous
    /// read; every other source and selection weighting go through
    /// `requestScopedCursor`).
    func setFloatCursor(_ point: (x: Int, y: Int)) {
        guard let document else { return }
        let reading: FloatReadout
        do {
            // nil is off the canvas: the readout freezes, like `setCursor(nil)`.
            guard let read = try FloatReadout.at(point, document: document) else { return }
            reading = read
        } catch {
            setCursorFailure(point, message: error.localizedDescription)
            return
        }
        showFloatReading(reading, at: point)
    }

    /// One extended-range reading on the RGB row — linear components with
    /// stops and the range note — shared by the composite read and the
    /// scoped float sample.
    func showFloatReading(_ reading: FloatReadout, at point: (x: Int, y: Int)) {
        clearReadoutFailure()
        lastPixel = point
        lastReadout = nil
        guard isViewLoaded else { return }
        valueLabels[.position]?.stringValue = "\(point.x), \(point.y)"
        valueLabels[.rgb]?.stringValue = reading.linearText
            + (reading.rangeNote.map { " (\($0))" } ?? "")
        valueLabels[.rgb]?.toolTip = "Linear RGBA, extended range. Stops: " + reading.stopsText
        // Lab/HSB are defined on display-referred numbers; a scene-linear
        // extended-range sample has no honest reading for them.
        valueLabels[.hsb]?.stringValue = "—"
        valueLabels[.lab]?.stringValue = "—"
        showGrayK(nil)
        showCmykInk(at: nil)
        updateFootnote()
    }

    /// #1 Lab phase 1c: a Lab Color reading on the Lab row (L*, a*, b* at one decimal, four in
    /// the tooltip); RGB, HSB, K and the CMYK ink read "—".
    func showLabReading(_ normalized: [Double]) {
        let value = LabDocument.lab(normalized)
        valueLabels[.lab]?.stringValue = LabDocument.readoutText(normalized)
        valueLabels[.lab]?.toolTip = String(format: "L*, a*, b*: %.4f, %.4f, %.4f",
                                            value.l, value.a, value.b)
        valueLabels[.rgb]?.stringValue = "—"
        valueLabels[.rgb]?.toolTip = nil
        valueLabels[.hsb]?.stringValue = "—"
        showGrayK(nil)
        showCmykInk(at: nil)
    }

    /// #1 phase 1c: the K row of an 8/16-bit Grayscale reading (`normalizedGray`, the sample's
    /// R = G = B), the K % of the code, and since Bitmap phase 2a-ii a Bitmap pixel's (0 %
    /// or 100 %); "—" for nil (another document, 32 bits, a failure).
    func showGrayK(_ normalizedGray: Double?) {
        valueLabels[.gray]?.stringValue = normalizedGray.map {
            String(format: "%.0f%%", GrayDocument.kPercent($0))
        } ?? "—"
    }
}
