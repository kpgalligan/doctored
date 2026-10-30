import AppKit

/// Image ▸ Adjustments ▸ Replace Color… and its adjustment layer:
/// Hue/Saturation applied where the pixels are close to the sampled colours
/// (`core/src/adjust_replace_color.rs`: full strength within half the
/// Fuzziness, fading to nothing at it).
///
/// The colours are the DOCUMENT's numbers, sampled from the adjustment's
/// input with "Sample from image" on: a click replaces the samples, ⇧-click
/// adds one (up to eight), ⌥-click removes the nearest. The dialog opens on
/// the foreground swatch; Clear drops every sample, after which the
/// foreground stands in until the next click.
///
/// On a CMYK document (#1) the samples are INKS (`inks`, four fractions):
/// the composite's ink under the pointer (`cmykSample`), the foreground's
/// stored ink (else its colour through the profile) standing in for none, and
/// the distance is over C, M, Y and K (`cmyk_adjust_ink`).
///
/// On a Lab Color document (#1 phase 2d) the samples are the document's own
/// 8-bit `#LLaabb` codes, which the core converts for the kernel; the
/// foreground stands in as its codes (`LabDocument.codeHex`), and the label
/// reads L*, a*, b* (`LabDocument.readoutText`), never the hex.
final class ReplaceColorSheet: AdjustmentSheet {
    private static let maxColors = 8

    /// Sampled colours, `"#rrggbb"` in the document's numbers.
    private var samples: [String] = []
    private var fuzziness = 40.0
    private var hue = 0.0
    private var saturation = 0.0
    private var lightness = 0.0
    private var monitor: CanvasPointerMonitor?
    private let sampleLabel = NSTextField(labelWithString: "")
    /// A CMYK document's sampled inks (`cmyk`).
    private var inkSamples: [[Double]] = []

    /// The CMYK document's typed handle when this sheet samples ink.
    private var cmyk: RasterPrecisionDocument? {
        document.doc?.isCmyk == true ? document.doc?.precision : nil
    }

    override func loadState() {
        inkSamples = (initial.params["inks"] as? [Any] ?? []).compactMap { entry in
            let values = (entry as? [Any])?.compactMap { ($0 as? NSNumber)?.doubleValue } ?? []
            return values.count == 4 ? values.map { min(max($0, 0), 1) } : nil
        }.prefix(Self.maxColors).map { $0 }
        samples = (initial.params["colors"] as? [Any] ?? []).compactMap {
            guard let text = $0 as? String else { return nil }
            return AdjustmentLayerPayload(op: op, params: ["c": text]).colorHex("c", default: "")
        }.filter { !$0.isEmpty }.prefix(Self.maxColors).map { $0 }
        fuzziness = clamped(initial.number("fuzziness", default: 40), 0...200).rounded()
        hue = clamped(initial.number("hue", default: 0), -180...180)
        saturation = clamped(initial.number("saturation", default: 0), -1...1)
        lightness = clamped(initial.number("lightness", default: 0), -1...1)
    }

    override func makeContent() -> NSView {
        sampleLabel.font = DS.sans(12)
        sampleLabel.textColor = DS.textMuted
        sampleLabel.setAccessibilityIdentifier("replace-color-samples")
        updateSampleLabel()
        let clear = StickerButton(
            title: "Clear", style: .secondary, target: self, action: #selector(clearClicked(_:)))
        let sampleRow = NSStackView(views: [sampleLabel, clear])
        sampleRow.orientation = .horizontal
        sampleRow.alignment = .centerY
        sampleRow.spacing = 8
        let grid = AdjustmentSheet.grid([
            [fieldLabel("Colors:"), sampleRow],
            checkboxRow(
                "Sample from image", value: monitor != nil, changesValues: false
            ) { [weak self] on in self?.setSampling(on) },
            sliderRow(
                "Fuzziness:", range: 0...200, value: fuzziness,
                format: { String(format: "%.0f", $0) }
            ) { [weak self] value in self?.fuzziness = value.rounded() },
            sliderRow(
                "Hue:", range: -180...180, value: hue, format: { String(format: "%.0f°", $0) }
            ) { [weak self] value in self?.hue = value.rounded() },
            sliderRow(
                "Saturation:", range: -100...100, value: saturation * 100, format: percentText
            ) { [weak self] value in self?.saturation = value.rounded() / 100 },
            sliderRow(
                "Lightness:", range: -100...100, value: lightness * 100, format: percentText
            ) { [weak self] value in self?.lightness = value.rounded() / 100 },
        ])
        return grid
    }

    override func currentParams() -> [String: Any] {
        if cmyk != nil {
            return ["inks": effectiveInks, "fuzziness": fuzziness, "hue": hue,
                    "saturation": saturation, "lightness": lightness]
        }
        return ["colors": effectiveColors, "fuzziness": fuzziness, "hue": hue,
                "saturation": saturation, "lightness": lightness]
    }

    /// The sampled inks, or the foreground's ink while there are none.
    private var effectiveInks: [[Double]] {
        guard inkSamples.isEmpty else { return inkSamples }
        let shared = ToolOptionsStore.shared.sharedState
        let color = AdjustmentColor.color(fromHex: String(shared.foreground.prefix(7)), in: .sRGB) ?? .black
        guard let doc = cmyk,
              let ink = try? CmykPaint.ink(color, stored: shared.foregroundInk, in: doc) else {
            return [[0, 0, 0, 1]]
        }
        return [Array(ink.prefix(4))]
    }

    private static func inkText(_ ink: [Double]) -> String {
        "C\(Int((ink[0] * 100).rounded())) M\(Int((ink[1] * 100).rounded())) "
            + "Y\(Int((ink[2] * 100).rounded())) K\(Int((ink[3] * 100).rounded()))"
    }

    override var footnote: String { "⇧ adds · ⌥ removes · one undo step" }

    override func viewWillDisappear() {
        super.viewWillDisappear()
        monitor?.stop()
        monitor = nil
    }

    /// The samples, or the foreground swatch in the document's numbers while
    /// there are none — `colors` is required, so the op never runs empty.
    private var effectiveColors: [String] {
        guard samples.isEmpty else { return samples }
        if isLab {
            let foreground = String(ToolOptionsStore.shared.sharedState.foreground.prefix(7))
            let lab = RasterLab.lab(srgb: Self.channels(foreground).map { $0 / 255 }) ?? [0, 0, 0]
            return [LabDocument.codeHex((l: lab[0], a: lab[1], b: lab[2]))]
        }
        return [AdjustmentColor.documentHex(
            forSRGBHex: ToolOptionsStore.shared.sharedState.foreground, in: documentColorSpace)]
    }

    private func updateSampleLabel() {
        if cmyk != nil {
            sampleLabel.stringValue = inkSamples.isEmpty
                ? "Foreground " + Self.inkText(effectiveInks[0])
                : inkSamples.map(Self.inkText).joined(separator: " · ")
            return
        }
        if isLab {
            let text = { (hex: String) in LabDocument.readoutText(Self.channels(hex).map { $0 / 255 }) }
            sampleLabel.stringValue = samples.isEmpty
                ? "Foreground " + text(effectiveColors[0])
                : samples.map(text).joined(separator: " · ")
            return
        }
        sampleLabel.stringValue = samples.isEmpty
            ? "Foreground \(effectiveColors[0])"
            : samples.joined(separator: " ")
    }

    /// A Lab Color document's samples are its own `#LLaabb` codes (#1 phase 2d).
    private var isLab: Bool { document.doc?.isLab == true }

    /// The three 0…255 channels of `"#rrggbb"` (the Lab arms').
    private static func channels(_ hex: String) -> [Double] {
        let value = UInt32(hex.dropFirst(), radix: 16) ?? 0
        return [Double((value >> 16) & 0xff), Double((value >> 8) & 0xff), Double(value & 0xff)]
    }

    @objc private func clearClicked(_ sender: Any?) {
        samples = []
        inkSamples = []
        updateSampleLabel()
        valuesChanged()
    }

    private func setSampling(_ on: Bool) {
        monitor?.stop()
        monitor = nil
        guard on, let canvas else { return }
        monitor = CanvasPointerMonitor(canvas: canvas) { [weak self] in self?.pointer($0) }
    }

    /// One read per press, of the adjustment's input under the pointer.
    private func pointer(_ event: CanvasPointerEvent) {
        guard event.phase == .down else { return }
        if let doc = cmyk {
            inkPointer(event, doc)
            return
        }
        guard let rgb = CanvasSample.rgb(
            document, mode: mode, destructiveLayer: aboveIndex, at: event.point)
        else { NSSound.beep(); return }
        let hex = String(
            format: "#%02x%02x%02x", Int(rgb[0].rounded()), Int(rgb[1].rounded()),
            Int(rgb[2].rounded()))
        if event.modifiers.contains(.option) {
            guard let nearest = nearestSample(to: rgb) else { NSSound.beep(); return }
            samples.remove(at: nearest)
        } else if event.modifiers.contains(.shift) {
            guard !samples.contains(hex) else { return }
            guard samples.count < Self.maxColors else { NSSound.beep(); return }
            samples.append(hex)
        } else {
            samples = [hex]
        }
        updateSampleLabel()
        valuesChanged()
    }

    /// One press on a CMYK document: the composite's ink under the pointer.
    private func inkPointer(_ event: CanvasPointerEvent, _ doc: RasterPrecisionDocument) {
        guard let context = try? PrecisionWorkContext(source: doc),
              let reply = try? doc.cmykSample(x: Int(event.point.x.rounded(.down)),
                                              y: Int(event.point.y.rounded(.down)), reach: 0,
                                              workingLimit: context.limit,
                                              otherRetained: context.otherRetained),
              let percent = (reply["ink_percent"] as? [Any])?.compactMap({ ($0 as? NSNumber)?.doubleValue }),
              percent.count == 4
        else { NSSound.beep(); return }
        let ink = percent.map { min(max($0 / 100, 0), 1) }
        if event.modifiers.contains(.option) {
            let distances = inkSamples.map { zip($0, ink).map { ($0 - $1) * ($0 - $1) }.reduce(0, +) }
            guard let nearest = distances.indices.min(by: { distances[$0] < distances[$1] }) else {
                NSSound.beep(); return
            }
            inkSamples.remove(at: nearest)
        } else if event.modifiers.contains(.shift) {
            guard !inkSamples.contains(ink) else { return }
            guard inkSamples.count < Self.maxColors else { NSSound.beep(); return }
            inkSamples.append(ink)
        } else {
            inkSamples = [ink]
        }
        updateSampleLabel()
        valuesChanged()
    }

    private func nearestSample(to rgb: [Double]) -> Int? {
        let distances = samples.map { hex -> Double in
            let value = UInt32(hex.dropFirst(), radix: 16) ?? 0
            let channels = [Double((value >> 16) & 0xff), Double((value >> 8) & 0xff),
                            Double(value & 0xff)]
            return zip(channels, rgb).map { ($0 - $1) * ($0 - $1) }.reduce(0, +)
        }
        return distances.indices.min { distances[$0] < distances[$1] }
    }
}

/// The ±100 readout for a value stored over 100.
private let percentText: (Double) -> String = { String(format: "%.0f%%", $0) }

/// Prefill defence: a meta may hold anything a host wrote.
private func clamped(_ value: Double, _ range: ClosedRange<Double>) -> Double {
    min(max(value, range.lowerBound), range.upperBound)
}
