import Foundation

/// The two files an Apple LIVE PHOTO is made of, as they sit on disk: a still
/// image and the short motion clip shot around it. Photos exports them as a
/// pair sharing one basename in one folder (`IMG_0001.HEIC` + `IMG_0001.MOV`,
/// or `.JPG` + `.MOV` in "most compatible" mode), which is exactly how
/// `LivePhoto.locate` finds the partner of whichever half the user opened. A
/// bare movie is a legitimate source too — it simply has no still.
struct LivePhotoSource {
    let video: URL
    /// nil for a bare movie: the frames are then all there is.
    let still: URL?
}

/// Pairing a Live Photo's files. A Live Photo places as an ordinary raster
/// layer of its still (`HostSourceWork.place`); the described `live_photo`
/// layer kind, which re-rendered other frames from the files' paths, was
/// removed on 2026-10-10 and an old file's layer of that kind opens as its
/// pixels (`isLegacyLayerDescription`).
enum LivePhoto {
    /// File extensions each half of a pair may carry, lowercased. The still
    /// list is deliberately only what Photos writes (HEIC, or JPEG in "most
    /// compatible" mode): pairing is by name, so admitting every image format
    /// would turn an unrelated `shot.png`/`shot.mov` pair into a Live Photo.
    static let videoExtensions = ["mov", "m4v", "mp4"]
    static let stillExtensions = ["heic", "heif", "jpg", "jpeg"]

    /// The Live Photo `url` belongs to, given either half of it: a movie
    /// (with its still if one sits beside it) or a still that HAS a movie
    /// beside it. nil for an ordinary image — a still with no clip is just a
    /// photo, and that is what makes this safe to try on every file the app
    /// opens.
    static func locate(_ url: URL) -> LivePhotoSource? {
        let ext = url.pathExtension.lowercased()
        if videoExtensions.contains(ext) {
            guard FileManager.default.fileExists(atPath: url.path) else { return nil }
            return LivePhotoSource(video: url, still: sibling(of: url, extensions: stillExtensions))
        }
        guard stillExtensions.contains(ext),
              let video = sibling(of: url, extensions: videoExtensions)
        else { return nil }
        return LivePhotoSource(video: video, still: url)
    }

    /// Same folder, same basename, one of `extensions` — the pairing Photos'
    /// own exports use. Both spellings are probed because the camera writes
    /// upper-case extensions and exporters often lower-case them; the pairing
    /// is by NAME only, so two unrelated files that share a basename do pair.
    private static func sibling(of url: URL, extensions: [String]) -> URL? {
        let base = url.deletingPathExtension()
        for ext in extensions {
            for candidate in [
                base.appendingPathExtension(ext), base.appendingPathExtension(ext.uppercased()),
            ] where FileManager.default.fileExists(atPath: candidate.path) {
                return candidate
            }
        }
        return nil
    }

    /// Whether `meta` is the description a Live Photo LAYER carried before
    /// 2026-10-10 (`{"type":"live_photo",…}`, any version). The native open
    /// clears it: the layer keeps its pixels and is a plain raster.
    static func isLegacyLayerDescription(_ meta: String) -> Bool {
        guard let data = meta.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return false }
        return object["type"] as? String == "live_photo"
    }
}
