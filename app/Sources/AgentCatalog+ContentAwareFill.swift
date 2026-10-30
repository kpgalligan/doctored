import Foundation

extension AgentServer {
    var contentAwareSourceMaskProperty: [String: Any] {
        ["type": "object", "description": "Optional source eligibility recipe: {version:1, initial:'all'|'none', operations:[{op:'include_rect'|'exclude_rect',x,y,width,height}|{op:'include_brush'|'exclude_brush',radius,points:[[x,y],...]}]}. Canvas pixel centers, round connected brush segments, ordered last-write wins. 512 operations, 4096 total points, 64 KiB request and 256M evaluated bounding-box work maximum. Geometry is clipped to the canvas. The selection is independent destination coverage. Omission keeps all ordinary source pixels eligible.",
         "properties": ["version": ["type": "integer", "enum": [1]],
            "initial": ["type": "string", "enum": ["all", "none"]],
            "operations": ["type": "array", "maxItems": 512,
                "items": ["type": "object"]]],
         "required": ["version", "initial", "operations"], "additionalProperties": false]
    }
}
