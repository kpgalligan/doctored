import AppKit
import Security

/// Storage for the Anthropic API key. Resolution order: the
/// ANTHROPIC_API_KEY environment variable, then a user-only (0600) file
/// under Application Support. Earlier builds kept the key in the login
/// keychain, but a keychain item's ACL is bound to the app's code
/// signature and every rebuild is ad-hoc signed afresh, so each launch
/// re-prompted for keychain access. The keychain is now read at most
/// once — to migrate an existing key into the file — and the legacy item
/// is deleted once the file holds it.
enum APIKeyStore {
    private static let service = "com.kgalligan.Rasterize"
    private static let account = "AnthropicAPIKey"

    static func resolve() -> String? {
        if let env = ProcessInfo.processInfo.environment["ANTHROPIC_API_KEY"],
            !env.isEmpty
        {
            return env
        }
        if let stored = loadFile() { return stored }
        // Migration: this keychain read is the last prompt an existing
        // install ever sees. Denying it just falls back to the panel's
        // key-entry field.
        guard let legacy = loadKeychain() else { return nil }
        if saveFile(legacy) { deleteKeychainItem() }
        return legacy
    }

    @discardableResult
    static func save(_ key: String) -> Bool {
        guard saveFile(key) else { return false }
        deleteKeychainItem()  // never leave a second, stale copy behind
        return true
    }

    /// Whether ANTHROPIC_API_KEY is set for this launch; it wins over the file.
    static var environmentOverrides: Bool {
        !(ProcessInfo.processInfo.environment["ANTHROPIC_API_KEY"] ?? "").isEmpty
    }

    /// Whether a key is stored in the file (ignoring the environment).
    static var hasStoredKey: Bool { loadFile() != nil }

    /// Deletes the stored key. The legacy keychain item goes too, or
    /// `resolve()` would migrate it straight back into the file.
    @discardableResult
    static func remove() -> Bool {
        deleteKeychainItem()
        guard let url = keyFileURL else { return false }
        do {
            try FileManager.default.removeItem(at: url)
            return true
        } catch let error as CocoaError where error.code == .fileNoSuchFile {
            return true
        } catch {
            return false
        }
    }

    // MARK: Key file

    /// ~/Library/Application Support/Doctored/anthropic_api_key, written
    /// 0600. Hand-placing a key there works too — loads trim whitespace.
    static var keyFileURL: URL? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first?
            .appendingPathComponent("Doctored", isDirectory: true)
            .appendingPathComponent("anthropic_api_key", isDirectory: false)
    }

    private static func loadFile() -> String? {
        guard let url = keyFileURL else { return nil }
        return SecretFile.read(url)
    }

    private static func saveFile(_ key: String) -> Bool {
        guard let url = keyFileURL else { return false }
        return SecretFile.write(Data((key + "\n").utf8), to: url)
    }

    // MARK: Legacy keychain (migration only)

    private static func loadKeychain() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
            let data = item as? Data
        else { return nil }
        return String(data: data, encoding: .utf8)
    }

    private static func deleteKeychainItem() {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
    }
}

/// Why an assistant session could not start, in a sentence for the panel.
struct OpenError: Error {
    let message: String
}
