import Foundation

/// The zipped Action bundle's container (`zip_store`): pack a folder into one ZIP, and unpack a
/// ZIP into a folder. `ActionAssets` is the one caller.
enum ZipArchive {
    /// Writes `folder`'s regular files into a new archive at `archive`, each named
    /// `<folder name>/<relative path>`, deterministically (`dr_zip_pack_dir`).
    static func pack(folder: URL, into archive: URL) throws {
        var error: UnsafeMutablePointer<CChar>?
        guard dr_zip_pack_dir(folder.path, archive.path, &error) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not write \(archive.path)."))
        }
    }

    /// Extracts `archive` into the existing folder `destination` (`dr_zip_unpack`), refusing any
    /// entry that could land outside it. On a throw `destination` may hold some entries; the
    /// caller removes it.
    static func unpack(_ archive: URL, into destination: URL) throws {
        var error: UnsafeMutablePointer<CChar>?
        guard dr_zip_unpack(archive.path, destination.path, &error) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not read \(archive.path)."))
        }
    }
}
