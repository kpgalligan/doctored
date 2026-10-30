import Foundation

extension AgentServer {
    /// `list_lens_profiles` (v1-lensfun; handler in AgentServer+LensProfiles.swift, the model in
    /// LensProfile.swift).
    func lensProfilesCatalog() -> [[String: Any]] {
        // An array of literals joined, never a `+` chain (see `adjustmentOps`).
        let description = [
            "Lists the bundled Lensfun lens profiles (Lensfun database v0.3.4, CC BY-SA 3.0, ",
            "the Lensfun project and its contributors) for the document's EXIF camera and lens: ",
            "camera {maker, model, mount, crop_factor} or null, crop_factor and its source (the ",
            "camera, or exif_35mm from the 35 mm focal length), candidates (lenses whose names ",
            "match the EXIF lens, best first: {id, maker, model, crop_factor, focal_min, ",
            "focal_max, distortion, tca, vignetting}), with all:true mount_lenses (every lens on ",
            "the camera's mount family), selected (the automatic match), profile (its resolved ",
            "distortion, TCA and vignetting at the EXIF focal length, f-number and distance) or ",
            "reason (why there is none: no camera crop factor, no lens name, no match, an unknown ",
            "focal length, or a target layer that is not the camera frame), the identity read from ",
            "EXIF, the database credit and the one-line summary. Pass a lens id or \"auto\" as ",
            "apply_filter lens_correction's profile. Changes nothing. Mirrors Filter > Lens ",
            "Correction's Lens Profile popup.",
        ].joined()
        return [
            tool("list_lens_profiles", description,
                 ["document_id": Self.docIDProperty,
                  "layer": ["type": "integer", "description": "The target layer (default: the active one); its pixel size is the frame the automatic match checks."],
                  "all": ["type": "boolean", "description": "Also list every lens on the camera's mount family (default false)."]]),
        ]
    }
}
