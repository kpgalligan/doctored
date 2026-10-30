import AppKit

/// `PrecisionGeometryWork.compute`'s merged-key branch on an Indexed Color document (#1
/// Indexed phase 3b). An 8 × 4 sRGB PNG of four 2-px colour columns opens (`RasterDocument.open`,
/// promoted by `promotedStructure`) and converts to an Exact Indexed document with no
/// transparent index; the Move tool's `.move`, Free Transform's `.transform` and the Crop tool's
/// `.crop` past the canvas, each built with the background swatch `indexedBackground`, compute
/// under a fresh job control. Every pixel the edit uncovers holds the entry nearest the swatch
/// (least squared RGB distance, ties to the lowest index: this case's own oracle, red's entry),
/// and the `indexedBackground: nil` twin gives white's nearest (gray's entry), which differs. No
/// event is synthesized and no window is opened.
func verifyIndexedBackgroundKey() throws {
    let columns: [[UInt8]] = [[200, 30, 30], [30, 160, 60], [40, 60, 200], [128, 128, 128]]
    let (width, height) = (8, 4)
    var rgba = [UInt8]()
    for _ in 0..<height {
        for x in 0..<width { rgba += columns[x / 2] + [255] }
    }
    let url = directory.appendingPathComponent("indexed-background-key.png")
    try uiWritePNG(rgba, width: width, height: height, to: url)
    // `openPreserving` takes 16-bit input only, so the 8-bit file opens as the app opens it and
    // is promoted as Image ▸ Mode ▸ Indexed Color's RGB stage promotes it (`rgbStage`).
    let context = try PrecisionWorkContext()
    let opened = try RasterDocument.open(url: url).promotedStructure(control: nil)
    let converted = try opened.convertToIndexed(
        options: ["palette": "exact", "transparency": false, "dither": "none"],
        workingLimit: context.limit, otherRetained: context.otherRetained, control: nil)
    let source = try RasterDocument(precision: converted)
    try require(source.isIndexed, "(a) the PNG converted to an Indexed Color document")
    let table = try converted.indexedColorTable()
    try require(table.transparentIndex == nil && table.colors.count == 4,
                "(a) an Exact table of four entries, no transparent index: \(table.colors)")

    // The oracle: the entry nearest a colour, least squared distance, ties to the lowest index.
    func nearest(_ rgb: [Double]) -> Int {
        var best = 0
        var bestDistance = Double.infinity
        for (index, entry) in table.colors.enumerated() {
            let distance = zip(entry, rgb).reduce(0.0) { $0 + pow(Double($1.0) - $1.1, 2) }
            if distance < bestDistance {
                best = index
                bestDistance = distance
            }
        }
        return best
    }
    let swatch = NSColor(srgbRed: 0.9, green: 0.1, blue: 0.1, alpha: 1)
    let swatchIndex = nearest([0.9 * 255, 0.1 * 255, 0.1 * 255])
    let whiteIndex = nearest([255, 255, 255])
    try require(table.colors[swatchIndex] == columns[0] && table.colors[whiteIndex] == columns[3]
                && swatchIndex != whiteIndex,
                "(b) the swatch's nearest is red's entry and white's is gray's")

    let cases: [(String, PrecisionGeometryRequest)] = [
        ("move", .move(layers: [0], dx: 2, dy: 0)),
        ("transform", .transform(layer: 0, matrix: CGAffineTransform(translationX: 2, y: 0),
                                 quad: nil, sampler: DR_FILTER_NEAREST)),
        ("crop", .crop(rect: CGRect(x: -2, y: 0, width: 10, height: 4), angle: 0,
                       sampler: DR_FILTER_NEAREST)),
    ]
    for (name, request) in cases {
        for (background, expected) in [(Optional(swatch), swatchIndex), (nil, whiteIndex)] {
            let label = "\(name) \(background == nil ? "without" : "with") the swatch"
            let work = try PrecisionGeometryWork(source: source, request: request,
                                                 indexedBackground: background)
            guard let result = try work.compute(control: RasterJobControl()),
                  let precision = result.precision else {
                throw ProbeFailure(description: "(c) \(label): the edit changed nothing")
            }
            try require(result.isIndexed, "(c) \(label): the result stays Indexed")
            // Each request uncovers the two leftmost columns of its result.
            for y in 0..<result.height {
                for x in 0..<2 {
                    let index = precision.indexedIndex(x: x, y: y)
                    try require(index == expected,
                                "(c) \(label): (\(x), \(y)) is \(String(describing: index)), "
                                    + "not the nearest entry \(expected)")
                }
                // The covered pixels are the source's, two to the left.
                for x in 2..<min(result.width, width + 2) {
                    let index = precision.indexedIndex(x: x, y: y)
                    let want = converted.indexedIndex(x: x - 2, y: y)
                    try require(index == want, "(c) \(label): (\(x), \(y)) moved exactly")
                }
            }
        }
    }
    print("PASS: Indexed background key: move, transform and crop past the canvas take the "
        + "swatch's nearest entry \(swatchIndex) with it and white's \(whiteIndex) without "
        + "(PrecisionGeometryWork.compute's merged-key branch)")
}
