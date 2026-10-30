import Foundation

/// The catalog entries for `get_file_info` and `set_file_info`, in their
/// own file behind an explicit non-inlining boundary for the reason
/// `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func fileInfoCatalog() -> [[String: Any]] {
        let docID: [String: Any] = [
            "type": "string",
            "description": "Which open document; defaults to the frontmost.",
        ]
        let list: (String) -> [String: Any] = { what in
            [
                "type": "array", "items": ["type": "string"],
                "description": what + " One string is accepted as a one-entry list. Each "
                    + "entry at most 256 characters, at most 256 entries; entries are "
                    + "trimmed and blank ones dropped. An empty array clears the field.",
            ]
        }
        return [
            tool(
                "get_file_info",
                "The document's file info — File ▸ File Info…: creator (an array of "
                    + "names), copyright, description and keywords (an array). Always all "
                    + "four keys; an absent field is an empty array or string. Each is read "
                    + "from the document's XMP (dc:creator, dc:rights, dc:description, "
                    + "dc:subject), else from the file's IPTC datasets, else from its EXIF "
                    + "Artist, Copyright and ImageDescription. Reads only.",
                ["document_id": docID]),
            tool(
                "set_file_info",
                "Sets the document's file info — File ▸ File Info… ▸ OK. Pass only the "
                    + "fields to change: an omitted key keeps the document's own value, and "
                    + "an empty string or array clears that field. One undo step. The "
                    + "fields are saved in the .doctored and written into JPEG, PNG and TIFF "
                    + "exports as XMP (and as EXIF Artist, Copyright and ImageDescription "
                    + "when the document carries an EXIF block with room for them); a "
                    + "file's older IPTC copies of the four fields are removed so they "
                    + "cannot contradict it. The reply is {ok, changed, file_info}; a call "
                    + "that changes nothing reports changed:false and adds no undo step. "
                    + "No pixel changes.",
                [
                    "document_id": docID,
                    "creator": list("Who made the picture, one entry per person."),
                    "copyright": [
                        "type": "string",
                        "description": "The copyright notice, at most 2000 characters, one "
                            + "line.",
                    ],
                    "description": [
                        "type": "string",
                        "description": "A description or caption, at most 8000 characters. "
                            + "Line feeds are kept; other control characters are refused.",
                    ],
                    "keywords": list("Keywords, one entry each."),
                ]),
        ]
    }
}
