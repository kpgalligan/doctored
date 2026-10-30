import Foundation

extension AgentServer {
    func persistedWarpCatalog() -> [[String: Any]] {
        [tool("get_warp",
              "Read one layer's re-editable displacement lattice. The layer index is bottom-first; "
                + "an absent layer means the active layer.",
              ["layer": ["type": "integer", "minimum": 0],
               "document_id": Self.docIDProperty]),
         tool("warp_layer",
              "Set, clear or bake one persisted warp at native depth. A set has a 2…16 by 2…16 "
                + "row-major lattice in the original plane's pixel-edge rectangle. Each finite "
                + "dx/dy is within 30000 pixels and every cell obeys the non-folding contraction "
                + "bound. Pixels move a linked mask; mask-only accepts raster, group and adjustment "
                + "masks. An unlinked mask stays exact. The whole plane is warped, regardless of "
                + "selection. In a 32-bit float document a set resamples premultiplied "
                + "extended-range samples and is applied at once: it keeps no re-editable field, "
                + "so get_warp reports null and clear and bake change nothing. Each set warps the "
                + "current pixels again, its lattice over the current layer bounds; undo to adjust "
                + "or remove a warp. Bake before other geometry or paint. Encoded-space "
                + "bilinear or bicubic sampling preserves the original for re-edit and native save.",
              ["layer": ["type": "integer", "minimum": 0],
               "operation": ["type": "string", "enum": ["set", "clear", "bake"]],
               "target": ["type": "string", "enum": ["pixels", "mask"]],
               "columns": ["type": "integer", "minimum": 2, "maximum": 16],
               "rows": ["type": "integer", "minimum": 2, "maximum": 16],
               "displacements": ["type": "array", "items": ["type": "array",
                   "items": ["type": "number"], "minItems": 2, "maxItems": 2]],
               "filter": ["type": "string", "enum": ["bilinear", "bicubic"]],
               "document_id": Self.docIDProperty], required: ["operation"])]
    }
}
