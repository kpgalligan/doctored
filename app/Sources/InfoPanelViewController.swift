import AppKit

// The Info panel's column scrolls (`InfoColumnDocumentView`).

/// The right panel's fourth tab: the document's histogram over a readout of
/// the pixel under the cursor, the selection's bounds and area, and the
/// document's size, resolution and profile.
///
/// The readout follows the cursor live and FREEZES with its last value when
/// the cursor leaves the canvas, which is why nothing here clears it.
///
/// Two rules the rest of the file keeps. **The histogram is canvas-sized
/// work and never runs on the main thread**: every scan goes through the
/// panel's own `HistogramLoader`, and everything the scan needs — the image
/// handle, the selection struct, the stride — is resolved here, on the main
/// thread, and captured by VALUE, so the background queue never reaches back
/// into the document. And **the numbers are the document's own**: the RGB is
/// what the pixels hold, converted nowhere, and Lab goes through the
/// document's own profile, reading "—" rather than an sRGB guess when this
/// build cannot model that profile.
final class InfoPanelViewController: NSViewController {
    /// Which pixels the histogram counts. The current selection GATES
    /// whichever of these is chosen — it is not a third source — so the
    /// popup stays two items and the footnote says what the gate does.
    private enum Source: Int {
        case composite
        case activeLayer
        case backdrop

        var title: String {
            switch self {
            case .composite: return "Composite"
            case .activeLayer: return "Active layer"
            case .backdrop: return "Below active layer"
            }
        }
    }

    /// One line of the readout table. An enum rather than nine stored
    /// labels: the grid, the captions and the "clear it all" pass are then
    /// one loop each, and a row cannot exist in one of them and not the
    /// others.
    enum Row: Int, CaseIterable {
        case position
        case rgb
        case hsb
        case lab
        /// #1 phase 1c: a Grayscale document's K % under the cursor, hidden on every other
        /// document (`InfoPanelViewController+Precision.swift`).
        case gray
        /// #1 Indexed phase 1a: an Indexed Color document's palette index under the cursor,
        /// hidden on every other document (`InfoPanelViewController+Precision.swift`).
        case index
        /// #1: a CMYK document's ink under the cursor and its total ink;
        /// "—" on an RGB document (`InfoPanelViewController+Cmyk.swift`).
        case cmyk
        case totalInk
        case selection
        case area
        case size
        case resolution
        case profile

        var caption: String {
            switch self {
            case .position: return "Position"
            case .rgb: return "RGB"
            case .hsb: return "HSB"
            case .lab: return "Lab"
            case .gray: return "K"
            case .index: return "Index"
            case .cmyk: return "CMYK"
            case .totalInk: return "Total ink"
            case .selection: return "Selection"
            case .area: return "Area"
            case .size: return "Size"
            case .resolution: return "Resolution"
            case .profile: return "Profile"
            }
        }
    }

    /// Weak, like the other three panels: the document owns the window that
    /// owns the editor that owns this panel, so a strong reference here is a
    /// cycle that keeps a closed document alive.
    weak var document: ImageDocument?

    /// Tab switches, mirroring the other three panels.
    var onShowLayers: (() -> Void)?
    var onShowChannels: (() -> Void)?
    var onShowAssistant: (() -> Void)?

    /// Called when the user clicks the Navigator tab.
    var onShowNavigator: (() -> Void)?

    /// Called when the user clicks the Paths tab.
    var onShowPaths: (() -> Void)?

    /// The canvas's live selection, PULLED at the moment a scan starts
    /// (installed by `EditorViewController+Info.showInfoTab`).
    ///
    /// A pull rather than a push because a `CanvasSelection` is only wanted
    /// on the two occasions a scan begins, and pulling on the main thread
    /// and capturing the struct by value is what lets `maskBytes()` — a
    /// canvas-sized rasterization — run on the loader's queue with the rest
    /// of the scan. The editor pushes the cheap half (bounds, and the count
    /// when it knows it for free) through `setSelectionState` instead.
    var selectionProvider: (() -> CanvasSelection?)?

    private let histogramView = HistogramView()
    private let sourcePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let displayPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    let selectionWeightCheckbox = NSButton(checkboxWithTitle: "Weight by selection", target: nil, action: nil)
    var analysisUsesSelection: Bool { selectionWeightCheckbox.state == .on }
    var scopedCursorRequired: Bool {
        selectedAnalysisScope != .composite || (analysisUsesSelection && document?.canvasSelection != nil)
    }
    let clippingLabel = NSTextField(labelWithString: "")
    private let footnote = NSTextField(wrappingLabelWithString: "")
    var valueLabels: [Row: NSTextField] = [:]
    /// The Color Samplers section, its last committed readings and, while
    /// a tone sheet previews, the after readings the editor pushed.
    private let colorSamplerView = ColorSamplerReadoutView()
    private var colorSamplerReadings: [ColorSamplerReading] = []
    private var colorSamplerAfter: [ColorSamplerReading]?

    let loader = HistogramLoader()
    var precisionHistogramControl: RasterJobControl?
    let readoutLoader = HistogramLoader()
    /// #1: the CMYK rows' ink sample, off the main thread (a feathered mask
    /// is prepared per sample, `InfoPanelViewController+Cmyk.swift`).
    let cmykLoader = HistogramLoader()
    var scopedReadoutControl: RasterJobControl?
    var scopedReadoutGeneration: UInt64 = 0
    var selectedAnalysisScope: HistogramSource {
        switch source {
        case .composite: return .composite
        case .activeLayer: return .layer(document?.activeLayerIndex ?? 0)
        case .backdrop: return .backdrop(below: document?.activeLayerIndex ?? 0)
        }
    }
    private var source: Source = .composite
    /// The plot a Lab Color document moved to Lightness (Luminosity or RGB), restored on the next
    /// document that is not Lab (#1 Lab phase 1c).
    private var displayBeforeLab: HistogramView.Display?
    private var isPanelVisible = false
    /// A scan that was asked for while the panel was hidden or while a
    /// gesture was live; consumed by `setPanelVisible` and by the gesture's
    /// final non-live post.
    private var needsHistogram = false
    /// Increments on every scan request, so a measurement that comes back
    /// after the selection has moved on is dropped rather than shown
    /// against the wrong marquee.
    private var scanGeneration = 0

    /// The last pixel the cursor reported, kept so a document change can
    /// re-read the same pixel — the readout follows an edit under a
    /// stationary cursor, which is how you watch a slider move a value.
    var lastPixel: (x: Int, y: Int)?
    /// The last readout shown. Never cleared (the freeze rule), except by a
    /// failed composite read, which shows no colour.
    var lastReadout: PixelReadout?
    /// The core's sentence when the read under the cursor failed; the
    /// footnote carries it until the next readout.
    private(set) var readoutFailure: String?
    /// The sentence when the histogram's read failed; the footnote
    /// carries it until the next bins arrive.
    private(set) var histogramFailure: String?
    private var selectionBounds: CGRect?
    /// The exact selected-pixel count: the editor's free answer when it has
    /// one, otherwise the loader's measurement, and nil while neither has
    /// arrived.
    private var selectedArea: Int?
    /// The stride the last scan used, for the "sampled" note.
    var lastStride = 1

    private static let counts: NumberFormatter = {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.maximumFractionDigits = 0
        return formatter
    }()

    init() {
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("InfoPanelViewController does not support NSCoder")
    }

    deinit {
        precisionHistogramControl?.cancel()
        scopedReadoutControl?.cancel()
        readoutLoader.cancel()
        cmykLoader.cancel()
        NotificationCenter.default.removeObserver(self)
        // Waits out an in-flight scan: it holds an image handle this panel
        // is the last owner of.
        loader.cancel()
    }

    // MARK: - View construction

    override func loadView() {
        let root = NSView(frame: NSRect(x: 0, y: 0, width: DS.panelWidth, height: 400))
        root.wantsLayer = true
        root.layer?.backgroundColor = DS.chromeBackground.cgColor

        let tabs = PanelTabsView(titles: EditorPanelTab.titles, activeIndex: 3) { [weak self] index in
            if index == 0 { self?.onShowLayers?() }
            if index == 1 { self?.onShowChannels?() }
            if index == 2 { self?.onShowAssistant?() }
            if index == 4 { self?.onShowNavigator?() }
            if index == 5 { self?.onShowPaths?() }
        }
        tabs.translatesAutoresizingMaskIntoConstraints = false

        // Both popups carry their choice in `tag`, never in the item's
        // position, and are given their own menus rather than appending to
        // whatever a fresh NSPopUpButton happens to hold.
        let sourceMenu = NSMenu()
        for item in [Source.composite, Source.activeLayer, Source.backdrop] {
            let menuItem = NSMenuItem(title: item.title, action: nil, keyEquivalent: "")
            menuItem.tag = item.rawValue
            sourceMenu.addItem(menuItem)
        }
        sourcePopup.menu = sourceMenu
        sourcePopup.selectItem(withTag: source.rawValue)
        sourcePopup.controlSize = .small
        sourcePopup.font = DS.sans(11)
        sourcePopup.target = self
        sourcePopup.action = #selector(sourceChanged(_:))
        sourcePopup.toolTip = "Source for histogram and RGB readout; Below active layer keeps enclosing group gates"
        sourcePopup.setAccessibilityIdentifier("info.analysis.source")

        let displayMenu = NSMenu()
        for display in HistogramView.Display.allCases {
            let menuItem = NSMenuItem(title: display.displayName, action: nil, keyEquivalent: "")
            menuItem.tag = display.rawValue
            displayMenu.addItem(menuItem)
        }
        displayPopup.menu = displayMenu
        displayPopup.selectItem(withTag: histogramView.display.rawValue)
        displayPopup.controlSize = .small
        displayPopup.font = DS.sans(11)
        displayPopup.target = self
        displayPopup.action = #selector(displayChanged(_:))
        displayPopup.toolTip = "Which counts the plot draws"

        let popups = NSStackView(views: [sourcePopup, displayPopup])
        popups.orientation = .horizontal
        popups.distribution = .fillEqually
        popups.spacing = 6

        selectionWeightCheckbox.state = .on
        selectionWeightCheckbox.controlSize = .small
        selectionWeightCheckbox.font = DS.sans(11)
        selectionWeightCheckbox.target = self
        selectionWeightCheckbox.action = #selector(selectionWeightChanged(_:))
        selectionWeightCheckbox.setAccessibilityIdentifier("info.analysis.selection")
        histogramView.setAccessibilityIdentifier("info.analysis.histogram")
        histogramView.translatesAutoresizingMaskIntoConstraints = false

        clippingLabel.font = DS.mono(10)
        clippingLabel.textColor = DS.textFaint
        clippingLabel.lineBreakMode = .byTruncatingTail
        clippingLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        footnote.font = DS.sans(10)
        footnote.textColor = DS.textFaint

        let grid = NSGridView(views: Row.allCases.map { row in
            let caption = NSTextField(labelWithString: "")
            caption.attributedStringValue = DS.microLabel(row.caption)
            let value = NSTextField(labelWithString: "—")
            value.font = DS.mono(11)
            value.textColor = DS.textStrong
            value.lineBreakMode = .byTruncatingTail
            // The value column truncates rather than widening the grid past
            // the panel: a Display P3 profile name is longer than the panel
            // is wide, and a scroll bar for one row would be worse.
            value.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            if row == .rgb { value.setAccessibilityIdentifier("info.analysis.rgb") }
            valueLabels[row] = value
            return [caption, value]
        })
        grid.rowSpacing = 6
        grid.columnSpacing = 10
        grid.column(at: 0).xPlacement = .leading
        grid.column(at: 0).width = 72
        grid.column(at: 1).xPlacement = .fill

        let column = NSStackView(views: [
            popups, selectionWeightCheckbox, histogramView, clippingLabel, grid, colorSamplerView, footnote,
        ])
        column.translatesAutoresizingMaskIntoConstraints = false
        column.orientation = .vertical
        column.alignment = .leading
        column.spacing = 8
        column.setCustomSpacing(4, after: histogramView)
        column.setCustomSpacing(14, after: clippingLabel)
        column.setCustomSpacing(14, after: grid)
        column.setCustomSpacing(14, after: colorSamplerView)

        // Ten sampler rows can outgrow a short window, so the column
        // scrolls; the flipped document view keeps it pinned to the top.
        let scrollView = NSScrollView()
        scrollView.translatesAutoresizingMaskIntoConstraints = false
        scrollView.drawsBackground = false
        scrollView.borderType = .noBorder
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        let documentView = InfoColumnDocumentView()
        documentView.translatesAutoresizingMaskIntoConstraints = false
        scrollView.documentView = documentView
        documentView.addSubview(column)

        root.addSubview(tabs)
        root.addSubview(scrollView)

        NSLayoutConstraint.activate([
            tabs.topAnchor.constraint(equalTo: root.topAnchor),
            tabs.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            tabs.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            tabs.heightAnchor.constraint(equalToConstant: DS.tabHeight),

            scrollView.topAnchor.constraint(equalTo: tabs.bottomAnchor),
            scrollView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            documentView.topAnchor.constraint(equalTo: scrollView.contentView.topAnchor),
            documentView.leadingAnchor.constraint(equalTo: scrollView.contentView.leadingAnchor),
            documentView.trailingAnchor.constraint(equalTo: scrollView.contentView.trailingAnchor),

            // The column sets the document view's height; the scroll view,
            // not a bottom anchor on the panel, absorbs a short window.
            column.topAnchor.constraint(equalTo: documentView.topAnchor, constant: 12),
            column.leadingAnchor.constraint(equalTo: documentView.leadingAnchor, constant: 12),
            column.trailingAnchor.constraint(equalTo: documentView.trailingAnchor, constant: -12),
            column.bottomAnchor.constraint(equalTo: documentView.bottomAnchor, constant: -12),

            popups.widthAnchor.constraint(equalTo: column.widthAnchor),
            histogramView.widthAnchor.constraint(equalTo: column.widthAnchor),
            histogramView.heightAnchor.constraint(equalToConstant: 96),
            clippingLabel.widthAnchor.constraint(equalTo: column.widthAnchor),
            grid.widthAnchor.constraint(equalTo: column.widthAnchor),
            colorSamplerView.widthAnchor.constraint(equalTo: column.widthAnchor),
            footnote.widthAnchor.constraint(equalTo: column.widthAnchor),
        ])

        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        NotificationCenter.default.addObserver(
            self, selector: #selector(documentDidChange(_:)),
            name: .imageDocumentImageDidChange, object: document)
        NotificationCenter.default.addObserver(
            self, selector: #selector(colorSamplersDidChange(_:)),
            name: .imageDocumentColorSamplersDidChange, object: document)
        loader.onBins = { [weak self] bins in self?.binsArrived(bins) }
        loader.onFailure = { [weak self] in self?.histogramFailed($0) }
        reloadDocumentRows()
        updateSelectionRows()
        updateFootnote()
        requestHistogram()
    }

    /// The window closed (`EditorViewController.releaseDisplayForWindowClose`):
    /// stop observing the document.
    func releaseForWindowClose() {
        NotificationCenter.default.removeObserver(
            self, name: .imageDocumentImageDidChange, object: nil)
        NotificationCenter.default.removeObserver(
            self, name: .imageDocumentColorSamplersDidChange, object: nil)
    }

    // MARK: - Document and panel state

    @objc private func documentDidChange(_ note: Notification) {
        // The Layers panel's rule, inverted for what this panel holds: a
        // live tick (a Move drag, an opacity scrub, a slider preview) is
        // exactly when a reader wants the one-pixel readout to follow, so
        // the cheap half refreshes every tick and only the canvas-sized
        // scan waits for the gesture's final non-live post.
        let isLive = (note.userInfo?["isLive"] as? Bool)
            ?? document?.isLiveEditing
            ?? false
        reloadDocumentRows()
        refreshReadout()
        refreshColorSamplers()
        guard !isLive else {
            needsHistogram = true
            return
        }
        requestHistogram()
    }

    /// Called whenever the panel is shown or hidden; a scan that was
    /// deferred while hidden happens here, and nothing canvas-sized runs
    /// while nobody can see the result.
    func setPanelVisible(_ visible: Bool) {
        guard isPanelVisible != visible else { return }
        isPanelVisible = visible
        if visible { refreshColorSamplers() }
        guard visible, needsHistogram else { return }
        requestHistogram()
    }

    /// The active layer moved, so the "Active layer" source is stale.
    ///
    /// The editor forwards its `layersPanel.onActiveLayerChange` to the
    /// Channels panel's twin of this method; the same one-line forward is
    /// what keeps this source live.
    func activeLayerChanged() {
        guard source != .composite else { return }
        scopedReadoutGeneration &+= 1
        scopedReadoutControl?.cancel()
        requestHistogram()
        refreshReadout()
    }

    /// Pushed from the editor's `updateStatus()`, where every selection
    /// change already lands: the marquee's bounds and, when it is worth
    /// counting, the number of selected pixels.
    ///
    /// `area` is nil whenever the editor cannot answer for free (an
    /// ellipse, a lasso, a wand mask), and the loader then measures it in
    /// the same background pass that builds the mask the histogram is
    /// gated by — one canvas scan for both answers, per completed gesture.
    ///
    /// A nil `area` therefore CLEARS the count, whether or not the bounds
    /// moved. Keeping the last exact one while the bounds happen to match
    /// printed a confident lie: an ellipse dragged inside the box a
    /// rectangle just vacated covers about 7,854 of its 10,000 px, and the
    /// row would have gone on claiming 10,000 until the background scan
    /// landed. "—" until the measurement arrives is what the row below
    /// promises.
    func setSelectionState(bounds: CGRect?, area: Int?) {
        scopedReadoutGeneration &+= 1
        scopedReadoutControl?.cancel()
        let changed = bounds != selectionBounds
        selectionBounds = bounds
        selectedArea = area
        guard isViewLoaded else { return }
        updateSelectionRows()
        updateFootnote()
        // A selection GATES the histogram, so a live one re-scans on every
        // push rather than only when its bounds move: bounds cannot tell a
        // rectangle from an ellipse inside the same box, nor one wand mask
        // from the next. The loader's debounce turns a burst — a marquee
        // drag posts one of these per mouse event — into a single scan.
        if changed || selectionBounds != nil { requestHistogram() }
        refreshReadout()
    }

    /// The pixel under the cursor, or nil to leave the last one showing.
    func setCursor(_ readout: PixelReadout?) {
        guard let readout = readout else { return }
        lastPixel = (x: readout.x, y: readout.y)
        if scopedCursorRequired {
            clearReadoutFailure()
            requestScopedCursor((readout.x, readout.y))
            return
        }
        show(readout)
    }

    /// The composite read under `point` failed with `message`: the position
    /// shows, the colour rows show "—", and the footnote names the reason.
    func setCursorFailure(_ point: (x: Int, y: Int), message: String) {
        if scopedCursorRequired {
            clearReadoutFailure()
            requestScopedCursor(point)
            return
        }
        showReadoutFailure(point, message: message)
    }

    /// The read under `point` failed with `message`, whichever path read it:
    /// the position shows, the colour rows show "—", and the footnote names
    /// the reason until the next readout.
    func showReadoutFailure(_ point: (x: Int, y: Int), message: String) {
        lastPixel = point
        lastReadout = nil
        readoutFailure = message
        guard isViewLoaded else { return }
        valueLabels[.position]?.stringValue = "\(point.x), \(point.y)"
        valueLabels[.rgb]?.stringValue = "—"
        valueLabels[.rgb]?.toolTip = nil
        valueLabels[.hsb]?.stringValue = "—"
        valueLabels[.lab]?.stringValue = "—"
        showCmykInk(at: nil)
        updateFootnote()
    }

    func clearReadoutFailure() {
        guard readoutFailure != nil else { return }
        readoutFailure = nil
        if isViewLoaded { updateFootnote() }
    }

    // MARK: - The Color Samplers

    @objc private func colorSamplersDidChange(_ note: Notification) {
        refreshColorSamplers()
    }

    /// Re-reads every sampler from the committed composite, only while the
    /// panel is visible (a hidden panel refreshes when it is shown).
    func refreshColorSamplers() {
        guard isViewLoaded, isPanelVisible, let document else { return }
        let set = document.colorSamplers
        colorSamplerReadings = set.items.isEmpty ? [] : ColorSamplerReader.read(set, in: document)
        colorSamplerView.update(samplers: set, readings: colorSamplerReadings, after: colorSamplerAfter)
    }

    /// The editor's after readings while a tone sheet previews; nil clears
    /// the after column.
    func setColorSamplerAfter(_ readings: [ColorSamplerReading]?) {
        colorSamplerAfter = readings
        guard isViewLoaded, isPanelVisible, let document else { return }
        colorSamplerView.update(samplers: document.colorSamplers, readings: colorSamplerReadings,
                                after: readings)
    }

    // MARK: - The readout rows

    private func show(_ readout: PixelReadout) {
        lastReadout = readout
        readoutFailure = nil
        guard isViewLoaded else { return }
        valueLabels[.rgb]?.toolTip = nil
        valueLabels[.position]?.stringValue = "\(readout.x), \(readout.y)"
        // The hex carries the alpha byte when the pixel is not opaque —
        // the app's one canonical spelling of a sampled colour.
        valueLabels[.rgb]?.stringValue =
            "\(readout.r), \(readout.g), \(readout.b) · \(readout.hex)"
        let hsb = readout.hsbRounded
        valueLabels[.hsb]?.stringValue = "\(hsb.h)°, \(hsb.s)%, \(hsb.b)%"
        if let lab = readout.lab {
            valueLabels[.lab]?.stringValue = String(
                format: "%.1f, %.1f, %.1f", lab.l, lab.a, lab.b)
        } else {
            // Never an sRGB guess: the footnote says which profile the
            // core cannot model.
            valueLabels[.lab]?.stringValue = "—"
        }
        // #1 phase 5: the working CMYK's separation of this RGB colour, "!" out of gamut.
        showCmykInk(at: (readout.x, readout.y),
                    rgb: [Double(readout.r), Double(readout.g), Double(readout.b)].map { $0 / 255 })
        updateFootnote()
    }

    /// Re-reads the pixel the cursor last reported. An edit under a
    /// stationary cursor changes the pixel without moving it, and the
    /// panel is often open precisely to watch that number move.
    private func refreshReadout() {
        guard isViewLoaded, let pixel = lastPixel, let document = document else { return }
        if scopedCursorRequired {
            clearReadoutFailure()
            requestScopedCursor(pixel)
            return
        }
        if document.doc?.isHighDepth == true { setPrecisionCursor(pixel); return }
        do {
            guard let readout = try PixelReadout.at(pixel, in: document, reach: 0) else {
                clearReadoutFailure()
                return
            }
            show(readout)
        } catch {
            setCursorFailure(pixel, message: error.localizedDescription)
        }
    }

    private func reloadDocumentRows() {
        guard isViewLoaded else { return }
        reloadDisplayMenu()
        if let k = valueLabels[.gray], let grid = k.superview as? NSGridView {
            // #1 Bitmap phase 2a-ii: a Bitmap pixel reads as Grayscale's K, 0 % or 100 %.
            grid.cell(for: k)?.row?.isHidden = document?.doc.map { $0.isGray || $0.isBitmap } != true
        }
        if let index = valueLabels[.index], let grid = index.superview as? NSGridView {
            grid.cell(for: index)?.row?.isHidden = document?.doc?.isIndexed != true
        }
        guard let doc = document?.doc else {
            valueLabels[.size]?.stringValue = "—"
            valueLabels[.resolution]?.stringValue = "—"
            valueLabels[.profile]?.stringValue = "—"
            return
        }
        valueLabels[.size]?.stringValue = "\(doc.width) × \(doc.height) px"
        valueLabels[.resolution]?.stringValue = PrintSize.resolutionText(doc.resolution)
        valueLabels[.profile]?.stringValue = doc.profileName
    }

    /// #1 Lab phase 1c: on a Lab Color document the histogram menu offers Lightness, a and b
    /// (`LabDocument.histogramTitle`; Luminosity and RGB hidden, and the plot moved off them to
    /// Lightness) drawn neutral; on every other document each item is shown under its own name.
    /// #1 phase 6b: on a Grayscale document the menu offers Gray alone (`GrayDocument.
    /// histogramTitle`), the Luminosity plot, restored as Lab's is on leaving it.
    private func reloadDisplayMenu() {
        let lab = document?.doc?.isLab == true
        let gray = document?.doc?.isGray == true
        for item in displayPopup.itemArray {
            guard let display = HistogramView.Display(rawValue: item.tag) else { continue }
            let title = lab ? LabDocument.histogramTitle(display)
                : gray ? GrayDocument.histogramTitle(display) : display.displayName
            item.title = title ?? display.displayName
            item.isHidden = title == nil
        }
        if lab, histogramView.display == .luminosity || histogramView.display == .rgb {
            displayBeforeLab = histogramView.display
            histogramView.display = .red
        } else if gray, histogramView.display != .luminosity {
            if displayBeforeLab == nil { displayBeforeLab = histogramView.display }
            histogramView.display = .luminosity
        } else if !lab, !gray, let restored = displayBeforeLab {
            histogramView.display = restored
            displayBeforeLab = nil
        }
        displayPopup.selectItem(withTag: histogramView.display.rawValue)
        if histogramView.neutralPlanes != (lab || gray) { histogramView.neutralPlanes = lab || gray }
    }

    private func updateSelectionRows() {
        guard isViewLoaded else { return }
        guard let bounds = selectionBounds else {
            valueLabels[.selection]?.stringValue = "None"
            valueLabels[.area]?.stringValue = "—"
            return
        }
        valueLabels[.selection]?.stringValue =
            "\(Int(bounds.width)) × \(Int(bounds.height)) px at \(Int(bounds.minX)), "
            + "\(Int(bounds.minY))"
        guard let area = selectedArea else {
            // The measurement is on its way from the loader; a bounding-box
            // estimate would overstate an ellipse by a fifth, so the row
            // waits rather than lies.
            valueLabels[.area]?.stringValue = "—"
            return
        }
        var text = "\(Self.number(area)) px"
        if let doc = document?.doc, doc.width > 0, doc.height > 0 {
            let share = Double(area) / Double(doc.width * doc.height) * 100
            text += String(format: " · %.1f%% of canvas", share)
        }
        valueLabels[.area]?.stringValue = text
    }

    func updateFootnote() {
        var parts: [String]
        if document?.doc?.isLab == true {
            // #1 Lab phase 1c.
            parts = [LabDocument.infoFootnote]
        } else if document?.doc?.isFloat == true {
            parts = ["32-bit float linear samples. The histogram counts the chosen source in stops from −12 to +6 EV and weights selection coverage; its end markers count values below zero and above white over the complete scope. RGB readout shows linear components with stops and uses the chosen source and selection-weighting switch. HSB/Lab are unavailable in this mode."]
        } else if document?.doc?.isHighDepth == true {
            parts = ["16-bit authoritative samples. Histogram weights selection coverage; endpoints count the complete scope independently of plot sampling. RGB readout uses the chosen source and selection-weighting switch. HSB/Lab are unavailable in this mode."]
        } else {
            parts = [
                "RGB readings use the chosen source. Histograms weight selection coverage; endpoints count the complete scope."
            ]
            if selectionBounds != nil {
                parts.append("The histogram counts the selection only.")
            }
            if let readout = lastReadout, readout.lab == nil {
                parts.append(
                    "Lab is unavailable: this build cannot model \(readout.labSpace), and an sRGB "
                        + "reading would be a made-up number.")
            }
            if let readout = lastReadout, !readout.paintExact {
                parts.append(
                    "This pixel is outside sRGB, so \(readout.paintHex) is the closest a colour "
                        + "argument elsewhere can name it.")
            }
        }
        if let readoutFailure {
            parts.append("RGB readout unavailable: \(readoutFailure)")
        }
        if let histogramFailure {
            parts.append("Histogram unavailable: \(histogramFailure)")
        }
        // Only on a real change: the footnote is a wrapping label, and
        // re-setting it would relay out the column on every cursor move.
        let text = parts.joined(separator: " ")
        guard footnote.stringValue != text else { return }
        footnote.stringValue = text
    }

    // MARK: - The histogram

    @objc private func sourceChanged(_ sender: NSPopUpButton) {
        guard let tag = sender.selectedItem?.tag, let next = Source(rawValue: tag) else { return }
        guard next != source else { return }
        scopedReadoutGeneration &+= 1
        scopedReadoutControl?.cancel()
        source = next
        requestHistogram()
        refreshReadout()
    }

    @objc private func selectionWeightChanged(_ sender: NSButton) {
        scopedReadoutGeneration &+= 1
        scopedReadoutControl?.cancel()
        requestHistogram()
        refreshReadout()
    }

    @objc private func displayChanged(_ sender: NSPopUpButton) {
        guard let tag = sender.selectedItem?.tag,
              let display = HistogramView.Display(rawValue: tag)
        else { return }
        // A display change is a redraw of counts already in hand: no scan.
        histogramView.display = display
    }

    /// Starts one background scan. Everything the scan reads is resolved
    /// HERE and captured by value — the image handle, the selection struct,
    /// the stride — so the queue never touches the document, and the panel
    /// pays nothing at all while it is hidden or mid-gesture.
    private func requestHistogram() {
        guard isViewLoaded else { return }
        guard isPanelVisible, let document = document, document.doc != nil else {
            needsHistogram = true
            return
        }
        guard !document.isLiveEditing else {
            needsHistogram = true
            return
        }
        needsHistogram = false
        requestScopedHistogram(scope: selectedAnalysisScope)
    }

    private func areaMeasured(_ area: Int, generation: Int) {
        // A measurement of a marquee that has since moved is not an answer
        // about this one; the newer scan is already on its way.
        guard generation == scanGeneration, selectionBounds != nil else { return }
        selectedArea = area
        updateSelectionRows()
    }

    func binsArrived(_ bins: HistogramBins?) {
        let failureWasShowing = histogramFailure != nil
        histogramFailure = nil
        guard isViewLoaded else { return }
        clippingLabel.toolTip = nil
        if failureWasShowing { updateFootnote() }
        histogramView.bins = bins
        guard let bins = bins, bins.total > 0 else {
            clippingLabel.stringValue = "No pixels to count"
            return
        }
        var text = document?.doc?.isFloat == true
            ? String(format: "Below zero: %.6g · above white: %.6g", bins.shadowClipped, bins.highlightClipped)
            : "Clipped: \(String(format: "%.6g", bins.shadowClipped)) shadow · "
                + "\(String(format: "%.6g", bins.highlightClipped)) highlight"
        if bins.sampled {
            // The shape is the image's; the counts are a sample of it, and
            // a reader comparing two numbers deserves to know which.
            text += bins.completeEndpoints ? " · complete endpoints; plot 1 in \(lastStride)" : " · 1 in \(lastStride) sampled"
        }
        clippingLabel.stringValue = text
    }

    /// The histogram's read failed with `message`: no plot, the
    /// clipping line says so with the sentence as its tooltip, and the
    /// footnote names it until the next bins arrive.
    func histogramFailed(_ message: String) {
        histogramFailure = message
        guard isViewLoaded else { return }
        histogramView.bins = nil
        clippingLabel.stringValue = "Histogram unavailable"
        clippingLabel.toolTip = message
        updateFootnote()
    }

    private static func number(_ value: Int) -> String {
        counts.string(from: NSNumber(value: value)) ?? String(value)
    }
}

/// The Info column's scroll document: flipped, so a column shorter than the
/// panel sits at the top.
private final class InfoColumnDocumentView: NSView {
    override var isFlipped: Bool { true }

    init() {
        super.init(frame: .zero)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("InfoColumnDocumentView does not support NSCoder")
    }
}
