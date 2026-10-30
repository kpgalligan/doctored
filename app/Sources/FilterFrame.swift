import AppKit

// `FilterFrame` is the host side of the FRAME — the selection's bounding box on the target in plane
// pixels, never recorded: `OperationCapture.selectionFrame`/`framedParameters` add it as `region_*`
// at 8 bits (scaled outward to a proxy), the 16-bit core derives it from coverage — and
// `kernelParameters` is the ONE translation of recorded host-level Offset arguments (Set to
// Background → `transparent` on a layer, one scalar `fill` on a mask/plane/channel) used by the
// sheets, `SelectedAdjustment.edit`, `PrecisionScalarOperations.filter`, the 16-bit proxy and
// `framedByteFilter` (`AgentServer+ByteFilters.swift`, wired by one line in `applyFilter`).

/// The host side of Radial Blur's and Offset's FRAME and of Offset's
/// Undefined Areas.
///
/// The frame is the rectangle, in the target plane's own pixels, that Offset
/// shifts and wraps within and that Radial Blur's centre is a fraction of:
/// the selection's bounding box or the whole plane. It is never recorded — a
/// replayed or ⌃F step uses the selection and placement at run time. At 16
/// bits the core derives it from the coverage; at 8 bits the host adds it as
/// `region_*` from the capture (`OperationCapture.framedParameters`).
///
/// Offset's first option is Set to Background. Doctored has no Background
/// layer kind, so on a LAYER it leaves the vacated area transparent; on a
/// mask it fills white, on a red/green/blue plane that component of the
/// background colour, and on the alpha plane or a saved channel the colour's
/// Rec. 709 gray. On a Lab Color document's Lightness, a or b plane (`lab`,
/// #1 Lab phase 2f) the fill is that plane's code of the background
/// colour's L*a*b* (Mode ▸ Lab Color's colour, `RasterLab.lab(srgb:)`).
/// `kernelParameters` is the ONE translation of the recorded host-level
/// arguments into the kernel's (`transparent` or one scalar `fill`), so the
/// sheets, `apply_filter`, Actions and ⌃F agree.
enum FilterFrame {
    static let framedOps: Set<String> = ["radial_blur", "offset"]

    /// Host-level `params` of `op` as the core kernel takes them on `target`
    /// (`lab`: of a Lab Color document). Only Offset is translated; every
    /// other op passes through. Throws, naming the key, on an Undefined Areas
    /// value or colour it cannot read.
    static func kernelParameters(_ op: String, _ params: [String: Any], target: PaintTarget,
                                 lab: Bool = false) throws -> [String: Any] {
        guard op == "offset" else { return params }
        var kernel = params
        kernel.removeValue(forKey: "background_color")
        guard let mode = params["undefined_areas"] else { return kernel }
        guard let name = mode as? String, ["background", "transparent", "repeat", "wrap"].contains(name) else {
            throw DoctoredCoreError(message: "undefined_areas must be background, transparent, repeat or wrap.")
        }
        guard name == "background" || name == "transparent" else { return kernel }
        guard target != .layer else {
            kernel["undefined_areas"] = "transparent"
            return kernel
        }
        let rgb = try backgroundBytes(params["background_color"])
        if lab, case .plane(let plane) = target,
           let index = [RasterPlane.red, .green, .blue].firstIndex(of: plane) {
            // L*/100, (a* + 128)/255, (b* + 128)/255: the plane's unit code.
            guard let values = RasterLab.lab(srgb: rgb.map { Double($0) / 255 }) else {
                throw DoctoredCoreError(message: "background_color could not be read as L*a*b*.")
            }
            let unit = index == 0 ? values[0] / 100 : (values[index] + 128) / 255
            kernel["undefined_areas"] = "fill"
            kernel["fill"] = min(max(unit, 0), 1)
            return kernel
        }
        let fill: Double
        switch target {
        case .mask: fill = 1
        case .plane(.red): fill = Double(rgb[0]) / 255
        case .plane(.green): fill = Double(rgb[1]) / 255
        case .plane(.blue): fill = Double(rgb[2]) / 255
        case .plane, .channel, .layer, .ink:
            // The gray the core reads off a colour (`PlaneAlgebra.coverageColor`):
            // the luma coefficients have one home, `blend.rs`'s `LUMA_*`.
            guard let gray = PlaneAlgebra.coverageColor(rgb + [255])?.first else {
                throw DoctoredCoreError(message: "background_color could not be reduced to a gray.")
            }
            fill = Double(gray) / 255
        }
        kernel["undefined_areas"] = "fill"
        kernel["fill"] = fill
        return kernel
    }

    /// `#rrggbb` (or `#rrggbbaa`, alpha ignored) as bytes; white when absent.
    private static func backgroundBytes(_ value: Any?) throws -> [UInt8] {
        guard let value else { return [255, 255, 255] }
        guard var hex = (value as? String)?.trimmingCharacters(in: .whitespaces) else {
            throw DoctoredCoreError(message: "background_color must be a #rrggbb colour.")
        }
        if hex.hasPrefix("#") { hex.removeFirst() }
        guard hex.count == 6 || hex.count == 8, let bits = UInt64(hex, radix: 16) else {
            throw DoctoredCoreError(message: "background_color must be a #rrggbb colour.")
        }
        let rgb = hex.count == 8 ? bits >> 8 : bits
        return [UInt8((rgb >> 16) & 0xFF), UInt8((rgb >> 8) & 0xFF), UInt8(rgb & 0xFF)]
    }

    /// The Radial Blur picker's frame in CANVAS pixels: the selection's bounds
    /// within the target's canvas extent, or that extent alone. The extent of
    /// a layer or colour plane is the layer's rect; of a mask its own rect
    /// (the layer's when the mask's storage is not readable, as on the 16-bit
    /// facade); of a channel the canvas. With `selectionBounds` false the
    /// selection is ignored and the frame is the target's extent — for a
    /// filter that takes no frame (Tilt-Shift), whose centre is a
    /// fraction of the whole target.
    static func pickerFrame(document: ImageDocument, target: PaintTarget, layer: Int,
                            selectionBounds: Bool = true) -> CGRect {
        guard let doc = document.doc else { return .zero }
        let canvas = CGRect(x: 0, y: 0, width: doc.width, height: doc.height)
        let placed = doc.layerPlacement(layer).map {
            CGRect(x: $0.x, y: $0.y, width: $0.width, height: $0.height)
        }
        let extent: CGRect
        switch target {
        case .layer, .plane, .ink: extent = placed ?? canvas
        case .mask:
            if let mask = doc.layerMaskStorage(layer) {
                extent = CGRect(x: mask.offsetX, y: mask.offsetY, width: mask.image.width, height: mask.image.height)
            } else {
                extent = placed ?? canvas
            }
        case .channel: extent = canvas
        }
        guard selectionBounds, let bounds = document.canvasSelection?.bounds else { return extent }
        let framed = bounds.intersection(extent)
        return framed.isNull || framed.isEmpty ? extent : framed
    }
}

extension OperationCapture {
    /// The bounding box of the nonzero selection bytes inside the target's
    /// rect (`placement`, the canvas for a channel), in PLANE pixels. nil with
    /// no selection, or when no selected pixel lies on the target: the whole
    /// plane, as the 16-bit core decides from its coverage.
    func selectionFrame() -> (x: Int, y: Int, width: Int, height: Int)? {
        guard let selection else { return nil }
        let (cw, ch) = (doc.width, doc.height)
        let rect = placement.map { (x: $0.x, y: $0.y, width: $0.width, height: $0.height) }
            ?? (x: 0, y: 0, width: cw, height: ch)
        let (x0, y0) = (max(0, rect.x), max(0, rect.y))
        let (x1, y1) = (min(cw, rect.x + rect.width), min(ch, rect.y + rect.height))
        guard x0 < x1, y0 < y1, selection.count == cw * ch else { return nil }
        var (minX, minY, maxX, maxY) = (Int.max, Int.max, -1, -1)
        selection.withUnsafeBufferPointer { bytes in
            for y in y0..<y1 {
                let row = y * cw
                var first = -1, last = -1
                for x in x0..<x1 where bytes[row + x] != 0 {
                    if first < 0 { first = x }
                    last = x
                }
                guard first >= 0 else { continue }
                minX = min(minX, first); maxX = max(maxX, last)
                minY = min(minY, y); maxY = y
            }
        }
        guard maxX >= 0 else { return nil }
        return (x: minX - rect.x, y: minY - rect.y, width: maxX - minX + 1, height: maxY - minY + 1)
    }

    /// `kernelParams` with the frame's `region_*` added for a framed `op`,
    /// scaled outward to `image` when it is a proxy of `source` (floor the
    /// start, ceil the end). Unchanged for any other op, or with no frame.
    func framedParameters(_ kernelParams: [String: Any], op: String, for image: RasterImage) -> [String: Any] {
        guard FilterFrame.framedOps.contains(op), let frame = selectionFrame(),
              source.width > 0, source.height > 0, image.width > 0, image.height > 0
        else { return kernelParams }
        let sx = Double(image.width) / Double(source.width), sy = Double(image.height) / Double(source.height)
        let x0 = min(image.width - 1, max(0, Int((Double(frame.x) * sx).rounded(.down))))
        let y0 = min(image.height - 1, max(0, Int((Double(frame.y) * sy).rounded(.down))))
        let x1 = min(image.width, max(x0 + 1, Int((Double(frame.x + frame.width) * sx).rounded(.up))))
        let y1 = min(image.height, max(y0 + 1, Int((Double(frame.y + frame.height) * sy).rounded(.up))))
        var params = kernelParams
        params["region_x"] = x0
        params["region_y"] = y0
        params["region_width"] = x1 - x0
        params["region_height"] = y1 - y0
        return params
    }
}
