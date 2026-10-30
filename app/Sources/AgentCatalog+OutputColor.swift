import Foundation

/// The Phase 11C catalog: the five new output-colour tools, plus the
/// annotations that teach `convert_profile`, `assign_profile`,
/// `get_color_profile` and the two export tools what this phase added.
///
/// Kept in its own file behind an explicit non-inlining boundary, for the
/// reason `AgentCatalog.swift`'s header gives: one giant literal made Release
/// WMO spend minutes in `COWArrayOpts` / `ColdBlockInfo`.
extension AgentServer {
    @inline(never) func outputColorCatalog() -> [[String: Any]] {
        let docID: [String: Any] = [
            "type": "string",
            "description": "Which open document; defaults to the frontmost.",
        ]
        let intent: [String: Any] = [
            "type": "string",
            "enum": ["perceptual", "relative", "saturation", "absolute"],
            "description":
                "Rendering intent. relative (the default) keeps in-gamut colours exactly and "
                + "clips the rest; perceptual compresses the whole gamut; saturation favours "
                + "vividness; absolute keeps the source white so the medium's own tint shows. "
                + "A profile without its own table for an intent falls back to A2B0/B2A0.",
        ]
        let bpc: [String: Any] = [
            "type": "boolean",
            "description":
                "Black point compensation (ISO 18619): maps the source's darkest colour onto "
                + "the destination's so shadows do not block up. Default false.",
        ]
        let proofPreset: [String: Any] = [
            "type": "string",
            "description":
                "A saved proof preset's name (list_proof_presets): its profile, read back now "
                + "and accepted only when its sha256 is the saved one, and its simulation "
                + "fields. Refused beside profile. Explicit fields below still apply on top.",
        ]
        let proofProfile: [String: Any] = [
            "type": "string", "enum": ["file", "installed", "working_cmyk"],
            "description":
                "Where the proof profile comes from: \"file\" with an absolute path, "
                + "\"installed\" with a name from list_installed_profiles, or \"working_cmyk\" "
                + "(Edit ▸ Color Settings' working CMYK, View ▸ Proof Setup ▸ Working CMYK).",
        ]
        // #547 phase 5: Preserve CMYK Numbers and the Working plates.
        let preserveNumbers: [String: Any] = [
            "type": "boolean",
            "description": "A CMYK document only (refused on RGB): send its ink to the proof "
                + "profile unconverted — Proof Setup's Preserve CMYK Numbers.",
        ]
        let plate: [String: Any] = [
            "type": "string", "enum": ["cyan", "magenta", "yellow", "black", "none"],
            "description": "Show one plate of the proof device as grey, ink dark — View ▸ Proof "
                + "Setup ▸ Working Cyan/Magenta/Yellow/Black Plate. Needs a CMYK proof profile; "
                + "none clears it.",
        ]
        return [
            tool(
                "get_proof_setup",
                "What this window is soft-proofing through: the profile name, the intent, "
                    + "the two simulate flags and whether Proof Colors and Gamut Warning are "
                    + "on. VIEW STATE — proofing changes no pixel, writes no undo step and "
                    + "leaves every render and export untouched. failure is the refusal that "
                    + "last turned proofing off by itself (null otherwise); the next toggle or "
                    + "setup clears it. default_preset is the saved proof preset a window with "
                    + "no setup proofs through (⌘Y, ⇧⌘Y, set_proof_setup and render_proof with "
                    + "no profile), or null.",
                ["document_id": docID]),
            tool(
                "set_proof_setup",
                "Chooses the medium the canvas is simulated on and turns Proof Colors / "
                    + "Gamut Warning on or off — View ▸ Proof Setup ▸ Custom… (or a saved "
                    + "preset in that submenu) and the two View toggles. Every key is "
                    + "optional and an absent one keeps its current value; with no profile, "
                    + "no preset and no setup on the window, the default proof preset is "
                    + "used, and with none of those the call is refused. preset turns "
                    + "proof_colors on unless proof_colors is passed. This is VIEW STATE: no "
                    + "undo step, no dirty flag, no history entry and nothing a save_copy "
                    + "would carry. Turning gamut_warning on turns proof_colors on, because a "
                    + "warning with no proof has nothing to warn about. With no setup, no preset "
                    + "and no default preset, turning proof_colors or gamut_warning on seeds the "
                    + "Working CMYK proof, as ⌘Y and ⇧⌘Y do. On a CMYK document the proof "
                    + "profile must be CMYK, and gamut_warning flags the inks it cannot hold "
                    + "(none when preserve_numbers) (#547).",
                [
                    "preset": proofPreset,
                    "profile": proofProfile,
                    "path": ["type": "string", "description": "Absolute path, with profile:\"file\"."],
                    "name": ["type": "string", "description": "Profile name, with profile:\"installed\"."],
                    "proof_colors": ["type": "boolean"],
                    "gamut_warning": ["type": "boolean"],
                    "intent": intent,
                    "black_point_compensation": bpc,
                    "simulate_paper_white": [
                        "type": "boolean",
                        "description": "Show the medium's own white instead of the display's.",
                    ],
                    "simulate_ink_black": [
                        "type": "boolean",
                        "description": "Show how far from black the medium's darkest tone is.",
                    ],
                    "gamut_color": [
                        "type": "string",
                        "description": "The warning overlay, \"#RRGGBB\"; default \"#808080\".",
                    ],
                    "preserve_numbers": preserveNumbers,
                    "plate": plate,
                    "document_id": docID,
                ]),
            tool(
                "render_proof",
                "The proofed picture as a PNG, plus how many pixels fall outside the proof "
                    + "gamut. Uses preset when given, else the window's current setup, else "
                    + "the default proof preset, with any key below overriding "
                    + "it FOR THIS RENDER ONLY — the window's own setup is not changed. The "
                    + "gamut rule is a CIE76 ΔE*ab above 5.0 between a colour and its own "
                    + "round trip through the proof at RELATIVE COLORIMETRIC, independent of "
                    + "the intent being rendered. At 8 bits the composite is proofed. At 16 bits "
                    + "the composite's 8-bit display codes are proofed — what the canvas shows; "
                    + "exact:true proofs the full 16-bit composite instead, on the background "
                    + "worker (about 17 s at 24 MP; mask adds a second pass). source in the "
                    + "reply says which: \"composite\" or \"display\". A 32-bit float document "
                    + "refuses. A CMYK document (#547) proofs its ink through a CMYK profile into "
                    + "Display P3 (space \"display-p3\", source \"cmyk\").",
                [
                    "exact": [
                        "type": "boolean",
                        "description": "16 bits only: proof the full 16-bit composite rather "
                            + "than its display codes. Default false; no effect at 8 bits.",
                    ],
                    "mask": [
                        "type": "boolean",
                        "description": "Also report the mask's own out-of-gamut count.",
                    ],
                    "max_side": [
                        "type": "integer", "minimum": 64, "maximum": 4096,
                        "description": "Longest side of the returned PNG; default 1024.",
                    ],
                    "preset": proofPreset,
                    "profile": proofProfile,
                    "path": ["type": "string"],
                    "name": ["type": "string"],
                    "intent": intent,
                    "black_point_compensation": bpc,
                    "simulate_paper_white": ["type": "boolean"],
                    "simulate_ink_black": ["type": "boolean"],
                    "gamut_warning": ["type": "boolean"],
                    "gamut_color": ["type": "string"],
                    "preserve_numbers": preserveNumbers,
                    "plate": plate,
                    "document_id": docID,
                ]),
            tool(
                "describe_profile",
                "Everything this build can say about an ICC profile file: its name, version, "
                    + "class, device space, PCS, whether it has a matrix/TRC model or colour "
                    + "tables, which intents have their own table, its media white, whether a "
                    + "proof can run through it (proofable, and proof_refusal naming the "
                    + "missing A2B or B2A table and any floating-point D2Bx/B2Dx twin when it "
                    + "cannot), whether a 16/32-bit convert accepts it as a destination "
                    + "(high_depth_destination; for a matrix/TRC profile where it is false, "
                    + "export_refusal and convert_refusal are the sentences an export or a "
                    + "convert into it is refused with, else null), and — when it cannot be "
                    + "used — a refusal "
                    + "naming what it is (a named-colour list, a device-link or abstract "
                    + "profile, an nCLR separation space such as 2CLR-4CLR or 6CLR, a CMY, HSV "
                    + "or other space that is not RGB, Gray or CMYK, a multiProcessElements "
                    + "table, a D2Bx/B2Dx-only profile, an oversized table).",
                ["path": ["type": "string", "description": "Absolute path to an .icc/.icm file."]],
                required: ["path"]),
            tool(
                "list_installed_profiles",
                "The ICC profiles installed on this machine — the ColorSync folders, "
                    + "Adobe's, which is where the press profiles live, and the printer-driver "
                    + "paper profiles under /Library/Printers. Each entry reports "
                    + "its device space and whether this build can decode into it, encode "
                    + "out of it and proof through it, and high_depth_destination: whether a "
                    + "16/32-bit convert_profile accepts it as a destination (a matrix/TRC "
                    + "model whose curves pass the high-depth precision checks).",
                [
                    "kinds": [
                        "type": "array", "items": ["type": "string", "enum": ["rgb", "gray", "cmyk"]],
                        "description": "Device spaces to list; omit for all of them.",
                    ]
                ]),
        ]
    }

    /// Teaches the pre-11C entries what this phase added, in place — the
    /// `annotatePrecisionCatalog` shape, so the original descriptions stay
    /// where their own features own them.
    @inline(never) func annotateOutputColorCatalog(_ catalog: inout [[String: Any]]) {
        for index in catalog.indices {
            guard let name = catalog[index]["name"] as? String,
                  var schema = catalog[index]["inputSchema"] as? [String: Any],
                  var properties = schema["properties"] as? [String: Any] else { continue }
            var note: String?
            switch name {
            case "convert_profile":
                properties["intent"] = [
                    "type": "string",
                    "enum": ["perceptual", "relative", "saturation", "absolute"],
                    "description": "Rendering intent; default relative, which is the "
                        + "conversion this tool has always done.",
                ]
                properties["black_point_compensation"] = [
                    "type": "boolean",
                    "description": "ISO 18619 black point compensation; default false.",
                ]
                properties["adjustments"] = [
                    "type": "string", "enum": ["keep", "preserve_appearance"],
                    "description": "What happens to adjustment LAYERS. keep (the default) "
                        + "leaves every parameter as authored. preserve_appearance keeps "
                        + "exposure and white_balance at evaluation document_v2 — both are "
                        + "light-linear through the document's own profile — and BAKES every "
                        + "other adjustment into one disclosed raster layer \"<name> (baked)\" "
                        + "in its place, leaving the rest of the stack where it is. It refuses, "
                        + "naming the layer, when a to-be-baked adjustment is inside a group, "
                        + "is clipped, or sits over transparent pixels. Into a profile with no "
                        + "matrix/TRC model, document_v2 exposure/white_balance cannot be kept: "
                        + "keep refuses naming the layer, preserve_appearance bakes it (visible, "
                        + "top-level, unclipped, opaque beneath), and a linear_light document "
                        + "refuses the profile.",
                ]
                properties["rasterize_sources"] = [
                    "type": "boolean",
                    "description": "When the settings cannot keep a RAW layer's source bound "
                        + "(black_point_compensation, absolute, or perceptual/saturation with "
                        + "a LUT side), rasterize every such layer first, then convert — ONE "
                        + "result and ONE undo step. Default false, which refuses instead. "
                        + "Settings the sources can follow keep them bound either way.",
                ]
                properties["name"] = [
                    "type": "string",
                    "description": "With profile:\"installed\", the profile's display name.",
                ]
                note = "The result reports intent, black_point_compensation and an adjustments "
                    + "array of {index,name,op,action,reason}. profile also accepts "
                    + "\"installed\" with a name from list_installed_profiles. On an 8-bit "
                    + "document a LUT (A2B/B2A) profile is a valid source AND destination — "
                    + "get_color_profile's can_decode is the bit that says so, while "
                    + "convertible keeps its narrower matrix/TRC meaning. At 16 and 32 bits "
                    + "both profiles must be matrix/TRC; a LUT source or destination answers "
                    + "changed:false with that rule. On a document holding a RAW layer "
                    + "(a retained image source), relative, perceptual and saturation between "
                    + "matrix/TRC profiles, and relative to or from a LUT profile at 8 bits, "
                    + "convert with every source kept bound; black_point_compensation, "
                    + "absolute, and perceptual or saturation with a LUT side refuse with a "
                    + "sentence naming the count and the setting that keeps them bound "
                    + "(intent relative, black_point_compensation false), unless "
                    + "rasterize_sources is true, which rasterizes those layers and converts "
                    + "in one step and reports rasterized_sources. A destination whose bytes "
                    + "are the document's own profile answers changed:false at every intent, "
                    + "black_point_compensation and adjustments setting, before any refusal "
                    + "(at 16 bits, after the conversion's memory admission). Any other "
                    + "matrix/TRC destination whose tone curves fail the precision check "
                    + "(describe_profile: model matrix, high_depth_destination false) is "
                    + "refused by the core's conversion preflight before any work at every "
                    + "depth, with describe_profile's convert_refusal; a document already "
                    + "tagged with such a profile converts OUT of it, its curves read through "
                    + "the parser's repairs (gamma 0 as linear, a falling table as its "
                    + "running maximum)."
            case "assign_profile":
                properties["rasterize_sources"] = [
                    "type": "boolean",
                    "description": "Rasterize every layer still bound to a retained image "
                        + "source, then assign — ONE result and ONE undo step. Without it, a "
                        + "document holding such a source refuses by name rather than "
                        + "reinterpreting a master silently.",
                ]
                properties["name"] = [
                    "type": "string",
                    "description": "With profile:\"installed\", the profile's display name.",
                ]
                note = "A document holding a RAW layer (a retained image source) "
                    + "REFUSES with a sentence naming the count; rasterize_sources:true is the "
                    + "disclosed way through. On an 8-bit document a profile with no "
                    + "matrix/TRC model REFUSES, naming the layer, while a document_v2 "
                    + "Exposure/White Balance layer exists or the document uses linear_light "
                    + "evaluation. A matrix/TRC profile whose tone curves fail the precision "
                    + "check (describe_profile's export_refusal) is assigned at 8 and 16 bits "
                    + "(a 32-bit float document refuses it, as above), "
                    + "with or without rasterize_sources, unless the document uses "
                    + "linear_light evaluation, holds text/shape artwork or holds a "
                    + "document_v2 Exposure/White Balance layer: each refuses at every depth "
                    + "with one Assign sentence naming the profile (and the layer)."
            case "get_color_profile":
                note = "can_decode (Phase 11C) is the wider question: on an 8-bit document true "
                    + "when this build can turn the document's numbers into PCS colour, "
                    + "including a LUT profile with a readable A2B table, which convertible is "
                    + "false for. At 16 and 32 bits a conversion's source must be matrix/TRC: "
                    + "can_decode is true for any matrix/TRC model, while convertible also "
                    + "needs curves that pass the high-depth precision checks, so a profile "
                    + "failing them can be converted out of but not into."
            case "export_image", "export_preview":
                note = "settings also accepts profile:\"file\" with profile_path, "
                    + "profile:\"installed\" with profile_name, intent (perceptual, relative, "
                    + "saturation, absolute; default relative) and black_point_compensation "
                    + "(default false). At their defaults the bytes are unchanged. A "
                    + "matrix/TRC destination whose tone curves fail the precision check "
                    + "(list_installed_profiles: model matrix, high_depth_destination false) "
                    + "is refused by the core before encoding at every depth, with "
                    + "describe_profile's export_refusal, unless the destination's bytes are "
                    + "the document's own profile under a non-absolute intent without "
                    + "black_point_compensation, which exports it unchanged. profile "
                    + "\"document\" never converts, so it is never refused this way and "
                    + "ignores intent and black_point_compensation; a LUT destination "
                    + "converts at every depth."
            default:
                continue
            }
            schema["properties"] = properties
            catalog[index]["inputSchema"] = schema
            if let note, let description = catalog[index]["description"] as? String {
                catalog[index]["description"] = description + " " + note
            }
        }
    }
}
