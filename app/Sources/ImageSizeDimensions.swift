import Foundation

enum ImageSizeUnit: Int, CaseIterable {
    case pixels = 0
    case percent = 1

    var title: String {
        switch self {
        case .pixels: return "Pixels"
        case .percent: return "Percent"
        }
    }
}

enum ImageSizeAxis {
    case width
    case height
}

/// Checked dimension conversion for Image > Image Size. The sheet keeps the
/// resolved integer result in `PrintSize`; this type only converts one edit.
enum ImageSizeDimensions {
    static func displayedValue(
        pixels: Int, originalPixels: Int, unit: ImageSizeUnit
    ) -> Double {
        switch unit {
        case .pixels:
            return Double(pixels)
        case .percent:
            guard originalPixels > 0 else { return .nan }
            return Double(pixels) * 100 / Double(originalPixels)
        }
    }

    static func resolve(
        value: Double, axis: ImageSizeAxis, unit: ImageSizeUnit,
        original: (width: Int, height: Int),
        current: (width: Int, height: Int), lockAspect: Bool
    ) -> (width: Int, height: Int)? {
        guard value.isFinite, value > 0,
              original.width > 0, original.height > 0,
              current.width > 0, current.height > 0 else { return nil }

        let result: (width: Double, height: Double)
        switch unit {
        case .pixels:
            guard value.rounded(.towardZero) == value else { return nil }
            if lockAspect {
                switch axis {
                case .width:
                    result = (value,
                              value * Double(original.height) / Double(original.width))
                case .height:
                    result = (value * Double(original.width) / Double(original.height),
                              value)
                }
            } else {
                result = axis == .width
                    ? (value, Double(current.height))
                    : (Double(current.width), value)
            }
        case .percent:
            let scale = value / 100
            let width = Double(original.width) * scale
            let height = Double(original.height) * scale
            if lockAspect {
                result = (width, height)
            } else {
                result = axis == .width
                    ? (width, Double(current.height))
                    : (Double(current.width), height)
            }
        }

        guard let width = checkedPixels(result.width),
              let height = checkedPixels(result.height),
              width <= PrintSize.maxPixelsPerAxis,
              height <= PrintSize.maxPixelsPerAxis,
              Int64(width) * Int64(height) <= Int64(RasterImage.maxResizePixels)
        else { return nil }
        return (width, height)
    }

    private static func checkedPixels(_ value: Double) -> Int? {
        guard value.isFinite, value > 0 else { return nil }
        let rounded = value.rounded(.toNearestOrAwayFromZero)
        // Positive percentages always resolve to at least one pixel. Pixel
        // input is integral before it reaches this helper, so it is unchanged.
        let bounded = max(1, rounded)
        guard bounded <= Double(PrintSize.maxPixelsPerAxis) else { return nil }
        return Int(bounded)
    }
}
