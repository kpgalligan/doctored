import AppKit

// Metadata inspection is admitted and leased.

/// Immutable UI metadata, admitted before JSON expansion and counted while any
/// facade/history state retains it. It contains no rendered pixel surrogate.
final class PrecisionFacadeMetadata {
    struct Layer {
        let info: RasterDocument.LayerInfo
        let subtreeStart: Int
        let adjustment: Bool
        let mask: LayerMaskProperties?
        let maskEnabled: Bool
        let clipped: Bool
        let blendIf: BlendIf?
        /// The core's canonical merged style JSON (`NativeLayerStyle`); nil for none.
        let style: String?
        /// The colour label, 0 none … 7 gray (`LayerLabel`).
        let label: UInt8
    }
    let raw: [String: Any]
    let width: Int
    let height: Int
    let layers: [Layer]
    let channels: [RasterDocument.ChannelInfo]
    let profile: Data
    let resolution: (x: Double, y: Double)
    /// The document's global light, the info object's `global_light`.
    let globalLight: GlobalLight
    let payload: PrecisionEditPayload

    static func capture(_ document: RasterPrecisionDocument) throws -> PrecisionFacadeMetadata {
        let descriptor = try document.descriptor()
        let retained = try PrecisionDocumentDescriptor.integer(descriptor, "inspection_bytes")
        let (temporary, overflow) = retained.multipliedReportingOverflow(by: 3)
        guard !overflow else { throw failure("High-depth inspection estimate overflowed.") }
        _ = try RasterResources.shared.admit(scratch: temporary)
        let info = try document.info()
        let profile = document.profileData
        return try PrecisionFacadeMetadata(info, profile: profile,
                                            payload: PrecisionEditPayload.admitted(bytes: retained))
    }

    private init(_ raw: [String: Any], profile: Data, payload: PrecisionEditPayload) throws {
        let descriptor = try PrecisionDocumentDescriptor(raw)
        width = descriptor.width
        height = descriptor.height
        guard !profile.isEmpty, let rows = raw["layers"] as? [[String: Any]],
              rows.count == Int(try PrecisionDocumentDescriptor.integer(raw, "layer_count")),
              let channelRows = raw["channels"] as? [[String: Any]],
              channelRows.count == Int(try PrecisionDocumentDescriptor.integer(raw, "channel_count")),
              let ppi = raw["resolution"] as? [String: Any],
              let xPPI = ppi["x_ppi"] as? Double, let yPPI = ppi["y_ppi"] as? Double,
              xPPI.isFinite, yPPI.isFinite, xPPI > 0, yPPI > 0,
              let light = raw["global_light"] as? [String: Any],
              let angle = light["angle"] as? Double, angle.isFinite,
              let altitude = light["altitude"] as? Double, altitude.isFinite else {
            throw Self.failure("High-depth metadata is incomplete.")
        }
        self.raw = raw
        self.profile = profile
        self.payload = payload
        resolution = (xPPI, yPPI)
        globalLight = GlobalLight(angle: angle, altitude: altitude)
        layers = try rows.enumerated().map { index, row in
            guard row["index"] as? Int == index, let depth = row["depth"] as? Int, depth >= 0,
                  let start = row["subtree_start"] as? Int, (0...index).contains(start),
                  // The stable identity beside the renumberable index, read
                  // as strictly as the channel row's own id below: a typed
                  // layer always has one and 0 is never a live value.
                  let identity = row["id"] as? NSNumber, identity.uint64Value != 0,
                  let kind = row["kind"] as? String,
                  ["raster", "adjustment", "isolated_normal_group", "isolated_blend_group", "pass_through_group"].contains(kind),
                  let name = row["name"] as? String,
                  let opacity = row["opacity"] as? Double, opacity.isFinite, (0...1).contains(opacity),
                  let visible = row["visible"] as? Bool, let clipped = row["clipped"] as? Bool,
                  let label = row["label"] as? Int, (0...Int(LayerLabel.maximumCode)).contains(label),
                  // The LOCK bits and the LINK-GROUP id (12E-1b). Reserved
                  // lock bits are masked off in the core wherever a value
                  // enters, so anything above `LockFlags.all` here is a
                  // metadata the host does not understand and refuses.
                  let locks = row["locks"] as? Int,
                  (0...Int(LockFlags.all.rawValue)).contains(locks),
                  let link = row["link"] as? Int, link >= 0,
                  // The group DISCLOSURE; true on every non-group row.
                  let open = row["open"] as? Bool,
                  let offset = row["offset"] as? [Int], offset.count == 2, row.keys.contains("blend_if"),
                  row.keys.contains("style") else {
                throw Self.failure("High-depth layer metadata is invalid.")
            }
            let blendIf = try NativeLayerStyle.parseBlendIf(row["blend_if"])
            let style = try NativeLayerStyle.parseStyle(row["style"])
            let group = kind.hasSuffix("group")
            guard let blend = row["blend_mode"] as? Int, let rawBlend = UInt32(exactly: blend),
                  NativeLayerBlending.allows(DrBlendMode(rawValue: rawBlend), group: group,
                                             adjustment: kind == "adjustment") else {
                throw Self.failure("Unsupported high-depth layer blend metadata.")
            }
            let dimensions = row["dimensions"] as? [Int]
            let bounds = row["bounds"] as? [Int]
            guard group || kind == "adjustment" || (dimensions?.count == 2) else {
                throw Self.failure("High-depth raster extent is missing.")
            }
            let rect = group ? (bounds?.count == 4 ? bounds : nil) : nil
            var mask: LayerMaskProperties?
            var maskEnabled = false
            if let value = row["mask"] as? [String: Any] {
                guard let size = value["dimensions"] as? [Int], size.count == 2,
                      let position = value["offset"] as? [Int], position.count == 2,
                      let linked = value["linked"] as? Bool, let enabled = value["enabled"] as? Bool,
                      let density = value["density"] as? Double, density.isFinite,
                      let feather = value["feather"] as? Double, feather.isFinite,
                      let outside = value["outside_coverage"] as? Int, [0, 255].contains(outside) else {
                    throw Self.failure("High-depth mask metadata is invalid.")
                }
                mask = LayerMaskProperties(linked: linked, density: density, feather: feather,
                    outsideCoverage: UInt8(outside), offsetX: position[0], offsetY: position[1], width: size[0], height: size[1])
                maskEnabled = enabled
            }
            return Layer(info: RasterDocument.LayerInfo(id: identity.uint64Value,
                name: name, opacity: opacity,
                blendMode: DrBlendMode(rawValue: UInt32(blend)),
                visible: visible, offsetX: rect?[0] ?? offset[0], offsetY: rect?[1] ?? offset[1],
                width: rect?[2] ?? dimensions?.first ?? 0,
                height: rect?[3] ?? dimensions?.last ?? 0,
                kind: group ? DR_LAYER_GROUP : DR_LAYER_RASTER, depth: depth,
                locks: UInt32(locks), link: UInt32(link), open: open),
                subtreeStart: start, adjustment: kind == "adjustment", mask: mask, maskEnabled: maskEnabled,
                clipped: clipped, blendIf: blendIf, style: style, label: UInt8(label))
        }
        channels = try channelRows.enumerated().map { index, row in
            guard row["index"] as? Int == index, let name = row["name"] as? String,
                  let id = row["id"] as? NSNumber, id.uint64Value != 0 else {
                throw Self.failure("High-depth channel metadata is invalid.")
            }
            guard let color = row["overlay_color"] as? [Int], color.count == 3,
                  color.allSatisfy({ (0...255).contains($0) }),
                  let opacity = row["overlay_opacity"] as? Double, opacity.isFinite, (0...1).contains(opacity),
                  let selected = row["color_indicates_selected"] as? Bool else {
                throw Self.failure("High-depth channel display metadata is invalid.")
            }
            // A spot channel's ink (#1): `null` for an alpha channel.
            var spot: SpotInk?
            if let value = row["spot"], !(value is NSNull) {
                guard let ink = SpotInk(json: value) else {
                    throw Self.failure("High-depth spot channel metadata is invalid.")
                }
                spot = ink
            }
            return RasterDocument.ChannelInfo(id: id.uint64Value, name: name,
                red: UInt8(color[0]), green: UInt8(color[1]), blue: UInt8(color[2]),
                opacity: opacity, colorIndicatesSelected: selected, spot: spot)
        }
    }

    func layer(_ index: Int) -> Layer? { layers.indices.contains(index) ? layers[index] : nil }
    func channel(_ index: Int) -> RasterDocument.ChannelInfo? {
        channels.indices.contains(index) ? channels[index] : nil
    }
    private static func failure(_ text: String) -> DoctoredCoreError { DoctoredCoreError(message: text) }
}

extension RasterDocument {
    static func isPrecisionNative(at url: URL) throws -> Bool {
        let file = try FileHandle(forReadingFrom: url)
        defer { try? file.close() }
        guard let prefix = try file.read(upToCount: 8), prefix.count == 8,
              ImageDocument.isNativeMagic(prefix) else { return false }
        let bytes = Array(prefix.dropFirst(4))
        let version = UInt32(bytes[0]) | UInt32(bytes[1]) << 8 | UInt32(bytes[2]) << 16 | UInt32(bytes[3]) << 24
        switch version {
        // Phase 11E: 39 is the TYPED evaluation wrapper and 38 its byte
        // twin, so the first word alone still tells the two readers apart.
        // Phase 11F: 40 is the TYPED float wrapper, outermost of all.
        // Phase 12C: 42 is the TYPED colour-label wrapper and 41 its byte
        // twin, outermost of all — the first word alone still tells the two
        // readers apart.
        // Phase 12D-1: 44 is the TYPED layer-comp wrapper and 43 its byte
        // twin, outermost of all — the first word alone still tells the two
        // readers apart.
        // Phase 12E-1: 46 is the TYPED identity wrapper and 45 its byte twin,
        // outermost of all AND always written, so from this build on the
        // first word of EVERY `.rz` is one of these two — which is exactly
        // what this sniffer needs it to be.
        // Phase 12E-3 / 12E-3b: 49 (the BYTE knockout word) and 50 (the TYPED
        // one) are deliberately NOT here. 49 is nested INSIDE 45 and 50 is
        // nested INSIDE 46, after 47 and 48 — neither is ever a first word,
        // so this sniffer is unchanged, and either of them arriving as a
        // first word is a malformed file the core refuses by name.
        case 11, 12, 13, 15, 17, 19, 20, 22, 24, 25, 26, 27, 29, 30, 31, 32, 33, 34, 35, 36, 37,
             39, 40, 42, 44, 46:
            return true
        case 0...10, 14, 16, 18, 21, 23, 28, 38, 41, 43, 45: return false
        default:
            throw DoctoredCoreError(message: "This native document version is not supported by this build.")
        }
    }

    /// Authoritative typed rendering. The caller owns worker admission and must
    /// retain this result independently from its explicitly lossy display image.
    func renderPrecision(control: RasterJobControl? = nil,
                         additionalRetainedBytes: UInt64 = 0) throws -> RasterPrecisionImage {
        guard let precision else { throw DoctoredCoreError(message: "This document stores byte pixels.") }
        let context = try PrecisionWorkContext(source: precision, additionalRetainedBytes: additionalRetainedBytes)
        return try precision.render(workingLimit: context.limit, otherRetained: context.otherRetained,
                                    control: control)
    }
}
