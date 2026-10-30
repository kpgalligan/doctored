import CoreVideo
import Foundation
import ImageIO
import Vision

// The people-instances oracle: what Vision's own person-instance request answers for
// one photo, with no Doctored code. `scripts/verify_people_instances.py`
// builds it into its output folder and compares the app's people with it.
// `peopleOracle` in `scripts/selection-probe/people.swift` is an in-process
// copy of the same request and rule; change both together.
//
// Usage: people-instances-oracle <image>
// Prints one JSON object: the upright image size, the instance mask size, and
// each nonzero Vision instance in left-to-right order (the x centroid of its
// core, then the y centroid, then the id) with its cell count, centroid and
// `core`: base64 of one byte per mask cell, 1 where that instance's coverage
// is at least 0.5 and the highest of every instance's there.
//
// Build: xcrun swiftc -O -target arm64-apple-macos15.0 main.swift -o people-instances-oracle

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

guard CommandLine.arguments.count == 2 else { fail("usage: people-instances-oracle <image>") }
let url = URL(fileURLWithPath: CommandLine.arguments[1])
// Upright, at full size: the orientation the app's document has.
guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
      let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
          kCGImageSourceCreateThumbnailFromImageAlways: true,
          kCGImageSourceCreateThumbnailWithTransform: true,
          kCGImageSourceThumbnailMaxPixelSize: 100_000] as CFDictionary) else {
    fail("cannot open \(url.path)")
}
let request = VNGeneratePersonInstanceMaskRequest()
do {
    try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
} catch {
    fail("vision: \(error.localizedDescription)")
}

struct Instance {
    var id: Int, cells: Int, cx: Double, cy: Double, core: [UInt8]
}
var planes: [(id: Int, values: [Float])] = []
var maskWidth = 0, maskHeight = 0
if let observation = request.results?.first {
    for id in observation.allInstances.sorted() where id != 0 {
        guard let buffer = try? observation.generateMask(forInstances: IndexSet(integer: id)) else {
            fail("no mask for instance \(id)")
        }
        guard CVPixelBufferGetPixelFormatType(buffer) == kCVPixelFormatType_OneComponent32Float else {
            fail("instance \(id) mask is not one-component float")
        }
        CVPixelBufferLockBaseAddress(buffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
        let width = CVPixelBufferGetWidth(buffer), height = CVPixelBufferGetHeight(buffer)
        let stride = CVPixelBufferGetBytesPerRow(buffer)
        guard let base = CVPixelBufferGetBaseAddress(buffer),
              planes.isEmpty || (width == maskWidth && height == maskHeight) else {
            fail("instance \(id) mask is unreadable")
        }
        maskWidth = width
        maskHeight = height
        var values = [Float](repeating: 0, count: width * height)
        for y in 0..<height {
            let row = base.advanced(by: y * stride).assumingMemoryBound(to: Float.self)
            for x in 0..<width { values[y * width + x] = row[x] }
        }
        planes.append((id, values))
    }
}
// The argmax step: a cell belongs to the instance whose raw coverage is
// highest there, if that is at least 0.5; ties go to the lower id. Vision's
// raw instance masks overlap where people touch.
let cellCount = maskWidth * maskHeight
var instances = planes.map {
    Instance(id: $0.id, cells: 0, cx: 0, cy: 0, core: [UInt8](repeating: 0, count: cellCount))
}
for cell in 0..<cellCount {
    var best = -1
    for (index, plane) in planes.enumerated() where plane.values[cell] >= 0.5 {
        if best < 0 || plane.values[cell] > planes[best].values[cell] { best = index }
    }
    guard best >= 0 else { continue }
    instances[best].core[cell] = 1
    instances[best].cells += 1
    instances[best].cx += Double(cell % maskWidth)
    instances[best].cy += Double(cell / maskWidth)
}
for index in instances.indices {
    let cells = Double(instances[index].cells)
    instances[index].cx = cells > 0 ? instances[index].cx / cells / Double(maskWidth) : 2
    instances[index].cy = cells > 0 ? instances[index].cy / cells / Double(maskHeight) : 2
}
instances.sort { a, b in
    if a.cx != b.cx { return a.cx < b.cx }
    if a.cy != b.cy { return a.cy < b.cy }
    return a.id < b.id
}
let report: [String: Any] = [
    "width": image.width, "height": image.height,
    "mask_width": maskWidth, "mask_height": maskHeight,
    "instances": instances.map {
        ["id": $0.id, "cells": $0.cells, "cx": $0.cx, "cy": $0.cy,
         "core": Data($0.core).base64EncodedString()] as [String: Any]
    },
]
guard let data = try? JSONSerialization.data(withJSONObject: report, options: [.sortedKeys]) else {
    fail("cannot encode the report")
}
FileHandle.standardOutput.write(data)
FileHandle.standardOutput.write(Data("\n".utf8))
