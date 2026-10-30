import Foundation

// `ImageDocument.applyEdit/updateLiveEdit` and `AgentServer.performGroupedEdit` reject loss of a
// surviving effective field unless the dedicated warp Set/Clear/Bake path authorizes it.

/// A surviving layer may discard its original only through Warp Set/Clear/Bake.
enum PersistedWarpPolicy {
    static let bakeSentence = "Bake the warp first (Layer > Warp…, Bake Warp, or warp_layer operation:bake)."

    static func publicationRefusal(from old: RasterDocument, to new: RasterDocument) -> String? {
        let surviving = Set((0..<new.layerCount).compactMap { new.layerInfo($0)?.id })
        for index in 0..<old.layerCount {
            guard let id = old.layerInfo(index)?.id, surviving.contains(id),
                  let before = try? old.warpInfo(index),
                  before["field"] is [String: Any],
                  let landing = (0..<new.layerCount).first(where: { new.layerInfo($0)?.id == id }),
                  let after = try? new.warpInfo(landing),
                  let oldField = before["field"] as? [String: Any]
            else { continue }
            guard let newField = after["field"] as? [String: Any],
                  NSDictionary(dictionary: oldField).isEqual(to: newField)
            else { return bakeSentence }
        }
        return nil
    }
}
