import AppKit
import ImageIO

// The ImageIO-fallback and paste ingest paths: a platform-decoded image
// whose embedded profile a document cannot carry is read into sRGB and
// reported, never stored as the refused profile's numbers under an sRGB label.

private let refusedMiB = 16 * 1024 * 1024

private func readBE32(_ data: Data, _ offset: Int) -> Int {
    data[offset..<offset + 4].reduce(0) { $0 << 8 | Int($1) }
}

private func writeBE32(_ data: inout Data, _ offset: Int, _ value: Int) {
    for index in 0..<4 { data[offset + index] = UInt8((value >> (24 - 8 * index)) & 0xff) }
}

/// The built-in Display P3 profile with one private `rzpd` tag appended, so
/// it stays an RGB matrix/TRC profile but is over the 16 MiB a document carries.
private func oversizedProfile() throws -> Data {
    let source = RasterProfile.builtin(.displayP3)
    try require(source.count > 132, "built-in Display P3 profile")
    var data = source
    let count = readBE32(data, 128)
    let tableEnd = 132 + 12 * count
    data.insert(contentsOf: Data(count: 12), at: tableEnd)
    writeBE32(&data, 128, count + 1)
    for index in 0..<count {
        let field = 132 + 12 * index + 4
        writeBE32(&data, field, readBE32(data, field) + 12)
    }
    let offset = (data.count + 3) & ~3
    data.append(Data(count: offset - data.count))
    let size = refusedMiB + 65_536
    var tag = Data("data".utf8)
    tag.append(Data(count: size - tag.count))
    data.append(tag)
    writeBE32(&data, tableEnd, 0x727a_7064) // 'rzpd'
    writeBE32(&data, tableEnd + 4, offset)
    writeBE32(&data, tableEnd + 8, size)
    writeBE32(&data, 0, data.count)
    for index in 84..<100 { data[index] = 0 }
    return data
}

/// Little-endian classic TIFF, 16 × 8, one uncompressed strip of chunky
/// 8-bit YCbCr (BT.601 of RGB 60, 200, 70) carrying `icc` — a layout image
/// 0.25 refuses, so the open reaches the ImageIO fallback.
private func yccTIFF(_ icc: Data) -> Data {
    let (width, height) = (16, 8)
    func le16(_ value: Int) -> [UInt8] { [UInt8(value & 0xff), UInt8((value >> 8) & 0xff)] }
    func le32(_ value: Int) -> [UInt8] { le16(value & 0xffff) + le16((value >> 16) & 0xffff) }
    let (red, green, blue) = (60.0, 200.0, 70.0)
    let luma = 0.299 * red + 0.587 * green + 0.114 * blue
    let sample = [luma, 128 + (blue - luma) / 1.772, 128 + (red - luma) / 1.402]
        .map { UInt8(max(0, min(255, $0.rounded()))) }
    var body = Data()
    for _ in 0..<(width * height) { body.append(contentsOf: sample) }
    let stripBytes = body.count
    func place(_ bytes: [UInt8]) -> Int {
        if body.count % 2 == 1 { body.append(0) }
        let offset = 8 + body.count
        body.append(contentsOf: bytes)
        return offset
    }
    let iccOffset = place([UInt8](icc))
    let bits = place(le16(8) + le16(8) + le16(8))
    let coefficients = place([299, 1000, 587, 1000, 114, 1000].flatMap(le32))
    let reference = place([0, 1, 255, 1, 128, 1, 255, 1, 128, 1, 255, 1].flatMap(le32))
    if body.count % 2 == 1 { body.append(0) }
    let short = { (value: Int) in le16(value) + [0, 0] }
    let entries: [(tag: Int, type: Int, count: Int, value: [UInt8])] = [
        (256, 3, 1, short(width)), (257, 3, 1, short(height)), (258, 3, 3, le32(bits)),
        (259, 3, 1, short(1)), (262, 3, 1, short(6)), (273, 4, 1, le32(8)),
        (277, 3, 1, short(3)), (278, 3, 1, short(height)), (279, 4, 1, le32(stripBytes)),
        (284, 3, 1, short(1)), (529, 5, 3, le32(coefficients)), (530, 3, 2, le16(1) + le16(1)),
        (531, 3, 1, short(1)), (532, 5, 6, le32(reference)),
        (34675, 7, icc.count, le32(iccOffset)),
    ]
    var out = Data([0x49, 0x49, 42, 0] + le32(8 + body.count))
    out.append(body)
    out.append(contentsOf: le16(entries.count))
    for entry in entries {
        out.append(contentsOf: le16(entry.tag) + le16(entry.type) + le32(entry.count) + entry.value)
    }
    out.append(contentsOf: le32(0))
    return out
}

private func drawn(_ image: CGImage, in space: CGColorSpace) throws -> [UInt8] {
    let (width, height) = (image.width, image.height)
    var bytes = [UInt8](repeating: 0, count: width * height * 4)
    let ok = bytes.withUnsafeMutableBytes { raw -> Bool in
        guard let context = CGContext(
            data: raw.baseAddress, width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: width * 4, space: space,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        return true
    }
    try require(ok, "reference context")
    return bytes
}

private func maxChannelDifference(_ a: [UInt8], _ b: [UInt8]) -> Int {
    zip(a, b).map { abs(Int($0) - Int($1)) }.max() ?? Int.max
}

private func platformDecode(_ url: URL) throws -> CGImage {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { throw ProbeFailure(description: "ImageIO cannot decode \(url.lastPathComponent)") }
    return image
}

func verifyRefusedProfiles() throws {
    // (a)–(b) The oversized profile and its preconditions.
    let big = try oversizedProfile()
    let info = RasterProfile.inspect(big)
    try require(info.kind == .rgbMatrix, "precondition: padded profile is RGB matrix/TRC (\(info.kind))")
    try require(big.count > refusedMiB, "precondition: padded profile is over 16 MiB")
    guard let bigSpace = CGColorSpace(iccData: big as CFData), bigSpace.model == .rgb else {
        throw ProbeFailure(description: "precondition: CoreGraphics reads the padded profile as RGB")
    }
    guard let pixel = RasterImage.from(rgba: [70, 80, 90, 255], width: 1, height: 1),
          let fresh = RasterDocument.from(image: pixel)
    else { throw ProbeFailure(description: "precondition: fresh document") }
    var coreRefused = false
    do { _ = try fresh.assigningProfile(big) } catch { coreRefused = true }
    try require(coreRefused, "precondition: a fresh document refuses the padded profile")

    // (c) The one rule, asked directly.
    let refusedIngest = Bitmap.ingestSpace(of: bigSpace)
    try require(CFEqual(refusedIngest.space, ColorProfile.sRGB), "refused ingest decodes into sRGB")
    try require(refusedIngest.profile == nil, "refused ingest carries no profile")
    let sentence = refusedIngest.refusal ?? ""
    try require(sentence.contains("16 MiB") && sentence.contains(info.name),
                "refused ingest names size and profile: \(sentence)")
    let p3Ingest = Bitmap.ingestSpace(of: CGColorSpace(name: CGColorSpace.displayP3))
    try require(p3Ingest.profile != nil && p3Ingest.refusal == nil, "Display P3 ingest is kept")
    try require(Bitmap.ingestRefusal(Data(count: 200)) == "The embedded colour profile is not an "
        + "ICC profile Doctored can read, so the image was converted from it to sRGB as it was read.",
        "not-ICC sentence")

    // (d) Fixtures that reach the ImageIO fallback.
    let refused = probeDirectory.appendingPathComponent("refused-profile.tiff")
    let accepted = probeDirectory.appendingPathComponent("accepted-profile.tiff")
    let refusedBytes = yccTIFF(big)
    try refusedBytes.write(to: refused)
    try yccTIFF(RasterProfile.builtin(.displayP3)).write(to: accepted)
    for url in [refused, accepted] {
        let name = url.lastPathComponent
        try require((try? RasterDocument.open(url: url)) == nil, "precondition: core refuses \(name)")
        try require(!HostSourceInput.claims(url), "precondition: no host source claims \(name)")
        try require(!ImageDocument.hostFallbackWouldLosePrecision(url),
                    "precondition: \(name) is an 8-bit platform decode")
    }
    let decoded = try platformDecode(refused)
    try require(((decoded.colorSpace?.copyICCData() as Data?)?.count ?? 0) > refusedMiB,
                "precondition: ImageIO decodes refused-profile.tiff in the padded profile")

    // (e) The open.
    let document = ImageDocument()
    try document.read(from: refused, ofType: "public.tiff")
    try require(document.doc.iccProfile == RasterProfile.builtin(.sRGB), "refused open is sRGB")
    try require(document.profileAdoption == .unchanged, "refused open adoption unchanged")
    let warnings = document.metadataImportReport?["warnings"] as? [String] ?? []
    let matching = warnings.filter { $0.contains("16 MiB") }
    try require(matching.count == 1 && matching.first == document.openProfileRefusal,
                "refused open reports one refusal: \(warnings)")
    let reference = try drawn(decoded, in: ColorProfile.sRGB)
    let relabel = try drawn(decoded, in: bigSpace)
    try require(maxChannelDifference(reference, relabel) >= 8,
                "reference conversion is distinguishable from a relabel")
    guard let pixels = document.doc.layerPixels(0), pixels.count == reference.count else {
        throw ProbeFailure(description: "refused open pixels")
    }
    try require(maxChannelDifference(pixels, reference) <= 2,
                "refused open converted to sRGB: \(Array(pixels.prefix(4))) vs \(Array(reference.prefix(4)))")

    let kept = ImageDocument()
    try kept.read(from: accepted, ofType: "public.tiff")
    try require(kept.profileAdoption == .converted, "accepted open converts into sRGB")
    try require(kept.openProfileRefusal == nil, "accepted open has no refusal")
    let keptWarnings = kept.metadataImportReport?["warnings"] as? [String] ?? []
    try require(!keptWarnings.contains { $0.contains("16 MiB") }, "accepted open reports no refusal")

    // (f) New from Clipboard.
    let pasteboard = NSPasteboard(name: .init("com.kgalligan.doctored.color-probe.refused-profile"))
    defer { pasteboard.releaseGlobally() }
    pasteboard.clearContents()
    try require(pasteboard.setData(refusedBytes, forType: .tiff), "pasteboard fixture")
    try require(((Bitmap.pasteboardImage(pasteboard)?.image.colorSpace?.copyICCData() as Data?)?.count ?? 0)
                > refusedMiB, "precondition: pasteboard image carries the padded profile")
    guard let made = try NewFromClipboard.make(pasteboard: pasteboard) else {
        throw ProbeFailure(description: "New from Clipboard made nothing")
    }
    guard case .external = made.outcome else {
        throw ProbeFailure(description: "New from Clipboard did not take the external path")
    }
    let pasted = made.document
    try require(pasted.doc.iccProfile == RasterProfile.builtin(.sRGB), "pasted document is sRGB")
    guard let pastedRefusal = pasted.openProfileRefusal else {
        throw ProbeFailure(description: "pasted document reports its refusal")
    }
    try require(pasted.metadataImportReport?["warnings"] as? [String] == [pastedRefusal],
                "pasted document's import report holds the refusal")

    // (g) The Assign Profile sheet's sentence.
    let unchanged = ColorProfileControls.adoptionSentence(.unchanged, profileRefusal: sentence)
    try require(unchanged == sentence, "unchanged + refusal is the refusal alone")
    let converted = ColorProfileControls.adoptionSentence(.converted, profileRefusal: sentence)
    try require(converted == sentence + " " + ColorProfileControls.adoptionSentence(.converted),
                "converted + refusal leads with the refusal")
    print("PASS: refused profiles convert to sRGB and are reported on the ImageIO open and paste paths")
}
