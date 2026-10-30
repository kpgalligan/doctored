import AppKit

/// Filters ▸ Develop Filter…'s Masks section: ordered local adjustments over the
/// global controls, the core's `masks` request (`develop_filter_masks.rs` states the order and the
/// mappings; `DevelopFilterMasks.swift` makes the subject, people and sky shapes).
///
/// A mask popup and New Radial / Linear / Brush / Range / Subject / People / Sky and Remove; a
/// tool segment (Radial, Linear, Add, Subtract, Sample) drawn on `DevelopFilterMaskCanvas`, the
/// target layer's thumbnail; brush Size (a share of the longer side) and Hardness; Density,
/// Feather and Invert; Develop's range rows (`RawLocalRangeControls`); the twelve local
/// adjustments (the global controls but Vibrance); then Sharpness and Noise Reduction
/// (`local_detail.rs`). Every coordinate is a share of the target's width and height, so the
/// preview's proxy takes the shapes unchanged; the two detail controls run at radii fixed in
/// target pixels, so the proxy leaves them out (`DevelopFilterMaskStore.params(_:revision:proxy:)`).
///
/// Each edit publishes a new revision to the sheet's `DevelopFilterMaskStore`; `values` is that
/// revision, so the worker-side preview reads the array it was asked for. The overlay is the
/// selected mask's shape coverage at thumbnail size (the native local mask's own preview); the
/// range is not overlaid. The section is disabled, with a note, off a layer's pixels, at 32 bits
/// and in CMYK.
final class DevelopFilterMaskPanel: NSObject {
    let view: NSView
    var onChange: (() -> Void)?
    let store: DevelopFilterMaskStore
    private(set) var revision = 0
    private weak var document: ImageDocument?
    private let layer: Int
    private let target: (width: Int, height: Int)
    private let available: Bool
    private var masks: [DevelopFilterMask] = []
    private var selected: Int?
    private let canvas: DevelopFilterMaskCanvas
    private let stack = NSStackView()
    private let list = NSPopUpButton()
    private let tools = NSSegmentedControl(labels: ["Radial", "Linear", "Add", "Subtract", "Sample"],
                                           trackingMode: .selectOne, target: nil, action: nil)
    private var buttons: [NSButton] = []
    private var rows: [String: CardControlRow] = [:]
    private let rangeRows = RawLocalRangeControls()
    private let status = NSTextField(wrappingLabelWithString: "")
    private var size = 5.0, hardness = 0.7

    private static let width: CGFloat = 400
    private static let tools: [RawLocalGestureView.Tool] = [.radial, .linear, .add, .subtract, .sample]

    init(document: ImageDocument) {
        self.document = document
        let doc = document.doc
        layer = document.activeLayerIndex
        let source = doc?.precision?.layerSource(document.activeLayerIndex)
        let info = doc?.layerInfo(document.activeLayerIndex)
        target = source.map { ($0.width, $0.height) } ?? (info?.width ?? 0, info?.height ?? 0)
        let reason: String?
        if document.planeEditTarget != .layer {
            reason = "Masks edit a layer's own pixels; choose the layer, not its mask or a channel."
        } else if doc?.isFloat == true {
            reason = "Masks are not available on a 32-bit float layer."
        } else if doc?.isCmyk == true {
            reason = "Masks are not available in a CMYK document."
        } else if target.width <= 0 || target.height <= 0 {
            reason = "Masks need a layer with pixels."
        } else {
            reason = nil
        }
        available = reason == nil
        store = DevelopFilterMaskStore(rangeProfile: doc.flatMap(DevelopFilterMasks.rangeProfile))
        canvas = DevelopFilterMaskCanvas(thumbnail: reason == nil ? doc.flatMap {
            DevelopFilterMaskCanvas.thumbnail($0, layer: document.activeLayerIndex)
        } : nil, target: target)
        let scroll = NSScrollView()
        view = scroll
        super.init()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 0, bottom: 8, right: 8)
        stack.translatesAutoresizingMaskIntoConstraints = false
        build(reason)
        scroll.documentView = stack
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        let clip = scroll.contentView
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: clip.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: clip.trailingAnchor),
            stack.topAnchor.constraint(equalTo: clip.topAnchor),
            scroll.widthAnchor.constraint(equalToConstant: Self.width + 20),
            scroll.heightAnchor.constraint(equalToConstant: 620),
        ])
        refresh()
    }

    /// The revision the sheet's `values` carry.
    var values: [Double] { [Double(revision)] }

    /// Reset: no masks.
    func reset() {
        masks = []
        selected = nil
        rangeRows.selectionChanged()
        publish()
    }

    private func build(_ reason: String?) {
        let heading = NSTextField(labelWithString: "Masks")
        heading.font = DS.sans(12, weight: .semibold)
        heading.textColor = DS.textMuted
        stack.addArrangedSubview(heading)
        if let reason {
            let note = NSTextField(wrappingLabelWithString: reason)
            note.font = DS.sans(11)
            note.textColor = DS.textMuted
            note.preferredMaxLayoutWidth = Self.width
            stack.addArrangedSubview(note)
        }
        list.target = self
        list.action = #selector(listChanged(_:))
        list.setAccessibilityIdentifier("develop-filter-mask-list")
        list.widthAnchor.constraint(equalToConstant: Self.width).isActive = true
        stack.addArrangedSubview(list)
        stack.addArrangedSubview(line([button("New Radial", "new-radial", #selector(newRadial(_:))),
                                       button("New Linear", "new-linear", #selector(newLinear(_:))),
                                       button("New Brush", "new-brush", #selector(newBrush(_:)))]))
        stack.addArrangedSubview(line([button("New Range", "new-range", #selector(newRange(_:))),
                                       button("Remove", "remove", #selector(remove(_:)))]))
        stack.addArrangedSubview(line([button("New Subject", "new-subject", #selector(newSubject(_:))),
                                       button("New People", "new-people", #selector(newPeople(_:))),
                                       button("New Sky", "new-sky", #selector(newSky(_:)))]))
        canvas.gesture.onCommand = { [weak self] in self?.drew($0, dabs: $1) }
        canvas.gesture.onSample = { [weak self] in self?.sampled($0) }
        canvas.gesture.onError = { [weak self] in self?.report($0) }
        stack.addArrangedSubview(canvas)
        tools.target = self
        tools.action = #selector(toolChanged(_:))
        tools.selectedSegment = 0
        tools.setAccessibilityIdentifier("develop-filter-mask-tool")
        stack.addArrangedSubview(tools)
        slider("Size %:", "size", 0.5...50, size, "%.1f") { $0.size = $1; $0.brushChanged() }
        slider("Hardness:", "hardness", 0...1, hardness, "%.2f") { $0.hardness = $1; $0.brushChanged() }
        slider("Density:", "density", 0...1, 1, "%.2f") { panel, value in panel.edit { $0.density = value } }
        slider("Feather:", "feather", 0...100, 0, "%.0f") { panel, value in panel.edit { $0.feather = value } }
        let invert = cardCheckboxRow(label: "Invert:", title: "Invert the mask", on: false) { [weak self] on in
            self?.edit { $0.invert = on }
        }
        invert.setAccessibilityIdentifier("develop-filter-mask-invert")
        rows["invert"] = invert
        stack.addArrangedSubview(invert)
        rangeRows.onChange = { [weak self] in self?.rangeChanged() }
        rangeRows.onSampleTool = { [weak self] in self?.selectTool(4) }
        rangeRows.pendingReadout = { [weak self] in self?.sampleReadout($0) ?? "" }
        stack.addArrangedSubview(rangeRows.view)
        for key in DevelopFilterMask.keys {
            let range = DevelopFilterControls.ranges[key] ?? -100.0...100.0
            let label = key.prefix(1).uppercased() + key.dropFirst() + ":"
            let format: (Double) -> String = key == "exposure"
                ? { String(format: "%+.2f EV", DevelopFilterControls.recorded(key, $0)) }
                : { String(format: "%+.0f", $0.rounded()) }
            let row = cardSliderRow(label: label, min: range.lowerBound, max: range.upperBound, value: 0,
                                    format: format) { [weak self] value in
                self?.edit { $0.adjustments[key] = DevelopFilterControls.recorded(key, value) }
            }
            row.setAccessibilityIdentifier("develop-filter-mask-\(key)")
            rows[key] = row
            stack.addArrangedSubview(row)
        }
        for key in DevelopFilterMask.detailKeys {
            let range = DevelopFilterMask.detailRanges[key] ?? 0.0...100.0
            let row = cardSliderRow(label: DevelopFilterMask.detailLabels[key] ?? key, min: range.lowerBound,
                                    max: range.upperBound, value: 0,
                                    format: { String(format: "%+.0f", $0.rounded()) }) { [weak self] value in
                self?.edit { $0.adjustments[key] = value.rounded() }
            }
            row.setAccessibilityIdentifier("develop-filter-mask-\(key)")
            rows[key] = row
            stack.addArrangedSubview(row)
        }
        status.font = DS.sans(11)
        status.textColor = DS.textMuted
        status.preferredMaxLayoutWidth = Self.width
        status.isHidden = true
        stack.addArrangedSubview(status)
    }

    // MARK: - Masks

    private func add(_ mask: DevelopFilterMask, tool: Int?) {
        guard masks.count < DevelopFilterMasks.maximumMasks else {
            report("The Develop Filter takes at most 16 masks.")
            return
        }
        var mask = mask
        mask.name = "\(mask.name) \(masks.count + 1)"
        masks.append(mask)
        selected = masks.count - 1
        rangeRows.selectionChanged()
        if let tool { tools.selectedSegment = tool }
        status.isHidden = true
        publish()
    }

    @objc private func newRadial(_ sender: Any?) {
        add(DevelopFilterMask(name: "Radial", shape: ["op": "radial", "center": [0.5, 0.5],
                                                        "radius": [0.25, 0.25], "hardness": hardness]), tool: 0)
    }
    @objc private func newLinear(_ sender: Any?) {
        add(DevelopFilterMask(name: "Linear", shape: ["op": "linear", "from": [0.5, 0.0], "to": [0.5, 1.0]]),
            tool: 1)
    }
    @objc private func newBrush(_ sender: Any?) {
        add(DevelopFilterMask(name: "Brush", shape: ["op": "empty"]), tool: 2)
    }
    @objc private func newRange(_ sender: Any?) {
        var mask = DevelopFilterMask(name: "Range", shape: ["op": "full"])
        mask.range = RawLocalRange(luminance: RawLuminanceRange(low: 0, high: 1)).json
        add(mask, tool: nil)
    }
    @objc private func newSubject(_ sender: Any?) { newGenerated(.subject) }
    @objc private func newPeople(_ sender: Any?) { newGenerated(.people) }
    @objc private func newSky(_ sender: Any?) {
        guard ModelGate.ready("sky") != nil else {
            ModelGate.offerSettings(ModelGate.notInstalled("sky"))
            return
        }
        newGenerated(.sky)
    }

    /// Resolves at once; nothing found says so on the status line and adds nothing.
    private func newGenerated(_ kind: DevelopFilterGeneratedKind) {
        guard let document, masks.count < DevelopFilterMasks.maximumMasks else { NSSound.beep(); return }
        do {
            let shape = try DevelopFilterMasks.mapShape(kind, document: document, layer: layer)
            add(DevelopFilterMask(name: kind.title, shape: shape), tool: nil)
        } catch is JobCancelled {
            return
        } catch {
            report(error.localizedDescription)
        }
    }

    @objc private func remove(_ sender: Any?) {
        guard let selected, masks.indices.contains(selected) else { return }
        masks.remove(at: selected)
        self.selected = masks.isEmpty ? nil : min(selected, masks.count - 1)
        rangeRows.selectionChanged()
        publish()
    }

    @objc private func listChanged(_ sender: NSPopUpButton) {
        let index = sender.indexOfSelectedItem - 1
        selected = masks.indices.contains(index) ? index : nil
        rangeRows.selectionChanged()
        refresh()
    }

    /// Applies `change` to the selected mask and publishes it.
    private func edit(_ change: (inout DevelopFilterMask) -> Void) {
        guard let selected, masks.indices.contains(selected) else { return }
        change(&masks[selected])
        publish(fill: false)
    }

    private func rangeChanged() {
        let json = rangeRows.rangeJSON
        edit { $0.range = json }
    }

    // MARK: - Gestures (target pixels from the canvas's mapper)

    private func drew(_ command: [String: Any], dabs: Int) {
        guard selected != nil else { report("Select or create a mask first."); return }
        let (w, h) = (Double(target.width), Double(target.height))
        let long = max(w, h)
        func point(_ value: Any?) -> [Double]? {
            guard let p = value as? [Any], p.count == 2, let x = (p[0] as? NSNumber)?.doubleValue,
                  let y = (p[1] as? NSNumber)?.doubleValue else { return nil }
            return [min(1, max(0, x / w)), min(1, max(0, y / h))]
        }
        switch command["op"] as? String {
        case "radial":
            guard let center = point(command["center"]), let radius = command["radius"] as? [Any],
                  radius.count == 2, let rx = (radius[0] as? NSNumber)?.doubleValue,
                  let ry = (radius[1] as? NSNumber)?.doubleValue else { return }
            edit {
                $0.shape = ["op": "radial", "center": center, "radius": [rx / w, ry / h], "hardness": hardness]
                $0.strokes = []
            }
        case "linear":
            guard let from = point(command["from"]), let to = point(command["to"]), from != to else { return }
            edit { $0.shape = ["op": "linear", "from": from, "to": to]; $0.strokes = [] }
        case "brush":
            guard let mode = command["mode"] as? String, let raw = command["dabs"] as? [[String: Any]] else { return }
            let dabs: [[String: Any]] = raw.compactMap { dab in
                guard let center = point(dab["center"]), let radius = (dab["radius"] as? NSNumber)?.doubleValue
                else { return nil }
                return ["center": center, "radius": radius / long, "hardness": dab["hardness"] ?? hardness,
                        "opacity": dab["opacity"] ?? 1]
            }
            guard !dabs.isEmpty else { return }
            edit { $0.strokes.append(["mode": mode, "dabs": dabs]) }
        default:
            return
        }
    }

    /// The Sample Colour readout while the point is pending: the point in target pixels.
    private func sampleReadout(_ sample: [Double]) -> String {
        guard sample.count == 2 else { return "" }
        return String(format: "at %.0f, %.0f", (sample[0] * Double(target.width)).rounded(),
                      (sample[1] * Double(target.height)).rounded())
    }

    private func sampled(_ point: CGPoint) {
        guard selected != nil else { return }
        rangeRows.sampled(at: CGPoint(x: min(1, max(0, point.x / CGFloat(target.width))),
                                      y: min(1, max(0, point.y / CGFloat(target.height)))))
    }

    @objc private func toolChanged(_ sender: Any?) { applyTool() }

    private func selectTool(_ index: Int) {
        tools.selectedSegment = index
        applyTool()
    }

    private func applyTool() {
        let index = tools.selectedSegment
        canvas.gesture.tool = selected != nil && Self.tools.indices.contains(index) ? Self.tools[index] : .pan
    }

    private func brushChanged() {
        canvas.gesture.radius = size / 100 * Double(max(target.width, target.height))
        canvas.gesture.hardness = hardness
    }

    // MARK: - State

    /// A new revision for the store, then the controls and overlay, then the preview.
    private func publish(fill: Bool = true) {
        revision += 1
        store.publish(revision, masks.map(\.json))
        if fill { refresh() } else { updateOverlay() }
        onChange?()
    }

    private func refresh() {
        list.removeAllItems()
        list.addItem(withTitle: masks.isEmpty ? "No masks" : "No mask selected")
        list.addItems(withTitles: masks.map(\.name))
        list.selectItem(at: selected.map { $0 + 1 } ?? 0)
        let mask = selected.flatMap { masks.indices.contains($0) ? masks[$0] : nil }
        rows["density"]?.setValue(mask?.density ?? 1)
        rows["feather"]?.setValue(mask?.feather ?? 0)
        rows["invert"]?.setOn(mask?.invert ?? false)
        for key in DevelopFilterMask.keys + DevelopFilterMask.detailKeys {
            rows[key]?.setValue(mask?.adjustments[key] ?? 0)
        }
        rangeRows.fill((mask?.range as? [String: Any]).flatMap { try? RawLocalRange(json: $0) })
        let has = mask != nil
        list.isEnabled = available
        for button in buttons {
            let identifier = button.accessibilityIdentifier()
            button.isEnabled = available
                && (identifier.contains("new-") ? masks.count < DevelopFilterMasks.maximumMasks : has)
        }
        for (key, row) in rows where !["size", "hardness"].contains(key) { row.isEnabled = available && has }
        rows["size"]?.isEnabled = available
        rows["hardness"]?.isEnabled = available
        tools.isEnabled = available && has
        rangeRows.setEnabled(available && has)
        canvas.gesture.enabled = available
        brushChanged()
        applyTool()
        updateOverlay()
    }

    /// The selected mask's shape coverage at the thumbnail's (or its map's) size.
    private func updateOverlay() {
        guard available, let selected, masks.indices.contains(selected) else { canvas.gesture.overlay = nil; return }
        do {
            canvas.gesture.overlay = try masks[selected].overlay(thumbnail: canvas.planeSize)
        } catch {
            canvas.gesture.overlay = nil
            report(error.localizedDescription)
        }
    }

    private func report(_ message: String) {
        status.stringValue = message
        status.isHidden = false
        NSSound.beep()
    }

    private func line(_ views: [NSView]) -> NSStackView {
        let row = NSStackView(views: views)
        row.spacing = 8
        return row
    }

    private func button(_ title: String, _ id: String, _ action: Selector) -> NSButton {
        let button = StickerButton(title: title, style: .secondary, target: self, action: action)
        button.setAccessibilityIdentifier("develop-filter-mask-" + id)
        buttons.append(button)
        return button
    }

    private func slider(_ label: String, _ id: String, _ range: ClosedRange<Double>, _ value: Double,
                        _ format: String, _ write: @escaping (DevelopFilterMaskPanel, Double) -> Void) {
        let row = cardSliderRow(label: label, min: range.lowerBound, max: range.upperBound, value: value,
                                format: { String(format: format, $0) }) { [weak self] value in
            guard let self else { return }
            write(self, value)
        }
        row.setAccessibilityIdentifier("develop-filter-mask-" + id)
        rows[id] = row
        stack.addArrangedSubview(row)
    }
}

/// One mask as the sheet holds it, and as the core's `masks` entry (`json`).
struct DevelopFilterMask {
    /// The local adjustment keys: the global controls but Vibrance.
    static let keys = DevelopFilterControls.keys.filter { $0 != "vibrance" }
    /// The local detail keys (`local_detail.rs`), in whole units, beside `keys`.
    static let detailKeys = ["sharpness", "noise_reduction"]
    static let detailRanges: [String: ClosedRange<Double>] = ["sharpness": -100...100, "noise_reduction": 0...100]
    static let detailLabels = ["sharpness": "Sharpness:", "noise_reduction": "Noise Reduction:"]

    var name: String
    var shape: [String: Any]
    var strokes: [[String: Any]] = []
    var invert = false
    var density = 1.0
    var feather = 0.0
    /// `RawLocalRangeControls.rangeJSON`: the range object, or NSNull.
    var range: Any = NSNull()
    var adjustments: [String: Double] = [:]

    init(name: String, shape: [String: Any]) {
        self.name = name
        self.shape = shape
    }

    var json: [String: Any] {
        var object: [String: Any] = ["shape": shape, "density": density, "feather": feather]
        if !strokes.isEmpty { object["brush"] = strokes }
        if invert { object["invert"] = true }
        if !(range is NSNull) { object["range"] = range }
        for key in Self.keys {
            let value = DevelopFilterControls.recorded(key, adjustments[key] ?? 0)
            if value != 0 { object[key] = key == "exposure" ? value as Any : Int(value) as Any }
        }
        for key in Self.detailKeys {
            let value = (adjustments[key] ?? 0).rounded()
            if value != 0 { object[key] = Int(value) }
        }
        return object
    }

    /// The shape, strokes and Invert as native local coverage on a `thumbnail`-sized plane (a
    /// map's own size for a map shape), feathered and weighted as the core does, as the red
    /// overlay image; nil when it covers nothing.
    func overlay(thumbnail: (width: Int, height: Int)) throws -> CGImage? {
        var plane = thumbnail
        let mask: RasterRawLocalMask?
        let (fw, fh) = (Double(plane.width), Double(plane.height))
        func pixels(_ value: Any?) -> [Double] {
            let p = (value as? [Any])?.compactMap { ($0 as? NSNumber)?.doubleValue } ?? [0, 0]
            return p.count == 2 ? [p[0] * fw, p[1] * fh] : [0, 0]
        }
        switch shape["op"] as? String {
        case "map":
            guard let width = (shape["width"] as? NSNumber)?.intValue,
                  let height = (shape["height"] as? NSNumber)?.intValue,
                  let text = shape["samples"] as? String, let data = Data(base64Encoded: text) else { return nil }
            plane = (width, height)
            mask = try RasterRawLocalMask.fromCoverage([UInt8](data), width: width, height: height)
        case "radial":
            let radius = (shape["radius"] as? [Any])?.compactMap { ($0 as? NSNumber)?.doubleValue } ?? [0.25, 0.25]
            mask = try RasterRawLocalMask.create(width: plane.width, height: plane.height, command: [
                "op": "radial", "center": pixels(shape["center"]),
                "radius": [max(0.5, radius[0] * fw), max(0.5, radius[1] * fh)],
                "hardness": shape["hardness"] ?? 0.5])
        case "linear":
            let from = pixels(shape["from"]), to = pixels(shape["to"])
            guard from != to else { return nil }
            mask = try RasterRawLocalMask.create(width: plane.width, height: plane.height,
                                                 command: ["op": "linear", "from": from, "to": to])
        case "full":
            mask = try RasterRawLocalMask.create(width: plane.width, height: plane.height, command: ["op": "full"])
        default:
            mask = try RasterRawLocalMask.create(width: plane.width, height: plane.height, command: ["op": "empty"])
        }
        guard var coverage = mask else { return nil }
        let (pw, ph) = (Double(plane.width), Double(plane.height))
        let long = max(pw, ph)
        for stroke in strokes {
            let dabs = (stroke["dabs"] as? [[String: Any]] ?? []).map { dab -> [String: Any] in
                let center = (dab["center"] as? [Any])?.compactMap { ($0 as? NSNumber)?.doubleValue } ?? [0, 0]
                let radius = (dab["radius"] as? NSNumber)?.doubleValue ?? 0.01
                return ["center": [center[0] * pw, center[1] * ph], "radius": max(0.5, radius * long),
                        "hardness": dab["hardness"] ?? 0.7, "opacity": dab["opacity"] ?? 1]
            }
            coverage = try coverage.edited(command: ["op": "brush", "mode": stroke["mode"] ?? "add", "dabs": dabs])
        }
        if invert { coverage = try coverage.edited(command: ["op": "invert"]) }
        let feather = min(256, feather / 100 * 0.04 * long)
        let image = try coverage.preview(x: 0, y: 0, width: plane.width, height: plane.height, divisor: 1,
                                         density: density, feather: feather, effective: true)
        return image.makeCGImage(in: ColorProfile.sRGB)
    }
}

/// The target layer's thumbnail (at most 360 px) under a `RawLocalGestureView` whose `mapper`
/// maps the view to the target's own pixels, so a drag reports target pixels.
final class DevelopFilterMaskCanvas: NSView {
    static let side: CGFloat = 360
    let gesture = RawLocalGestureView(frame: .zero)
    private let image: CGImage?
    private let target: (width: Int, height: Int)

    /// The overlay plane: the thumbnail's own size.
    var planeSize: (width: Int, height: Int) {
        guard let image else { return (max(1, target.width), max(1, target.height)) }
        return (image.width, image.height)
    }

    init(thumbnail: CGImage?, target: (width: Int, height: Int)) {
        image = thumbnail
        self.target = target
        super.init(frame: NSRect(x: 0, y: 0, width: Self.side, height: Self.side))
        translatesAutoresizingMaskIntoConstraints = false
        widthAnchor.constraint(equalToConstant: Self.side).isActive = true
        heightAnchor.constraint(equalToConstant: Self.side).isActive = true
        gesture.frame = bounds
        gesture.autoresizingMask = [.width, .height]
        gesture.mapper = { [weak self] in self?.mapping() }
        gesture.tool = .pan
        addSubview(gesture)
        setAccessibilityIdentifier("develop-filter-mask-canvas")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("DevelopFilterMaskCanvas does not support NSCoder") }

    override var isFlipped: Bool { true }

    /// The aspect-fit rect of the target in this view, and the target's pixel frame.
    private func mapping() -> RawLocalGestureView.Mapping? {
        guard target.width > 0, target.height > 0 else { return nil }
        let scale = min(bounds.width / CGFloat(target.width), bounds.height / CGFloat(target.height))
        let size = CGSize(width: CGFloat(target.width) * scale, height: CGFloat(target.height) * scale)
        let rect = CGRect(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2,
                          width: size.width, height: size.height)
        return RawLocalGestureView.Mapping(rect: rect, source: CGRect(x: 0, y: 0, width: target.width,
                                                                      height: target.height))
    }

    override func draw(_ dirtyRect: NSRect) {
        DS.canvasVoid.setFill()
        bounds.fill()
        guard let image, let map = mapping(), let context = NSGraphicsContext.current?.cgContext else { return }
        context.saveGState()
        context.translateBy(x: map.rect.minX, y: map.rect.maxY)
        context.scaleBy(x: 1, y: -1)
        context.interpolationQuality = .high
        context.draw(image, in: CGRect(origin: .zero, size: map.rect.size))
        context.restoreGState()
    }

    /// The thumbnail of `layer` (at most `side` px): an 8-bit layer resized, a 16-bit one the
    /// core's thumbnail; nil when it cannot be read.
    static func thumbnail(_ doc: RasterDocument, layer: Int) -> CGImage? {
        let maxSide = Int(side)
        let image: RasterImage?
        if doc.isHighDepth {
            guard let source = doc.precision?.layerSource(layer), let context = try? PrecisionWorkContext(image: source) else {
                return nil
            }
            image = try? source.thumbnailRGBA8(maxSide: maxSide, workingLimit: context.limit,
                                               otherRetained: context.otherRetained)
        } else if let layerImage = doc.layerImage(layer), layerImage.width > 0, layerImage.height > 0 {
            let scale = min(1, Double(maxSide) / Double(max(layerImage.width, layerImage.height)))
            image = scale == 1 ? layerImage : layerImage.resized(
                w: max(1, Int((Double(layerImage.width) * scale).rounded())),
                h: max(1, Int((Double(layerImage.height) * scale).rounded())), filter: DR_FILTER_BILINEAR)
        } else {
            image = nil
        }
        return image?.makeCGImage(in: doc.colorSpace)
    }
}
