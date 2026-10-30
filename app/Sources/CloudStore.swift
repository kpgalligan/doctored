import Foundation

// The cloud providers' local state (`designs/neural-models.md` "M3b shipped"). On disk:
//
//   <directory>/<provider>_api_key   the user's own key for that provider, written 0600
//   <directory>/cloud.json           {"consents": {<provider>: {terms_version, accepted_at}},
//                                     "agent_cloud_tools": false}, written 0600 and atomically
//
// The directory is `DR_CLOUD_DIR` when set, else ~/Library/Application Support/Doctored/Cloud.
// Tests, probes and drives never use the user's. No environment variable carries a provider
// key: a key is only ever the file the user saved in Settings ▸ Cloud, and it never appears in a
// log, a job's details or a reply.

/// A user-only (0600) file of secret text: the assistant's key (`APIKeyStore`) and the cloud
/// providers' keys and `cloud.json` (`CloudStore`).
enum SecretFile {
    /// The file's text with surrounding whitespace trimmed; nil when it is missing or empty.
    static func read(_ url: URL) -> String? {
        guard let raw = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        let text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty ? nil : text
    }

    /// Writes `data` atomically, then makes the file 0600 and flushes its folder
    /// (`DurableFile.persistDirectory`). False on any failure.
    static func write(_ data: Data, to url: URL) -> Bool {
        do {
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try data.write(to: url, options: [.atomic])
            try FileManager.default.setAttributes(
                [.posixPermissions: 0o600], ofItemAtPath: url.path)
            DurableFile.persistDirectory(url.deletingLastPathComponent())
            return true
        } catch {
            return false
        }
    }

    /// Deletes the file; true when it is gone (or never was).
    static func remove(_ url: URL) -> Bool {
        do {
            try FileManager.default.removeItem(at: url)
            return true
        } catch let error as CocoaError where error.code == .fileNoSuchFile {
            return true
        } catch {
            return false
        }
    }
}

struct CloudStoreError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// The providers' keys, the consents and the agent switch (module comment). Main thread.
enum CloudStore {
    static let directoryEnvironment = "DR_CLOUD_DIR"
    /// Posted on main after a key, a consent or the agent switch changes.
    static let didChange = Notification.Name("DoctoredCloudStoreDidChange")

    /// `DR_CLOUD_DIR`, else the Application Support folder.
    static var directory: URL {
        if let value = ProcessInfo.processInfo.environment[directoryEnvironment], !value.isEmpty {
            return URL(fileURLWithPath: (value as NSString).expandingTildeInPath, isDirectory: true)
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        return support.appendingPathComponent("Doctored/Cloud", isDirectory: true)
    }

    private static func keyURL(_ id: CloudProviderID) -> URL {
        directory.appendingPathComponent("\(id.rawValue)_api_key", isDirectory: false)
    }

    private static var settingsURL: URL { directory.appendingPathComponent("cloud.json") }

    // MARK: - Keys

    /// The provider's stored key, nil when none is saved.
    static func key(_ id: CloudProviderID) -> String? { SecretFile.read(keyURL(id)) }

    /// Saves `key` (trimmed) for the provider. False for an empty key or a failed write.
    @discardableResult
    static func saveKey(_ key: String, for id: CloudProviderID) -> Bool {
        let trimmed = key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, SecretFile.write(Data((trimmed + "\n").utf8), to: keyURL(id))
        else { return false }
        NotificationCenter.default.post(name: didChange, object: nil)
        return true
    }

    /// Deletes the provider's key file.
    @discardableResult
    static func removeKey(_ id: CloudProviderID) -> Bool {
        let removed = SecretFile.remove(keyURL(id))
        NotificationCenter.default.post(name: didChange, object: nil)
        return removed
    }

    // MARK: - cloud.json

    private static func settings() -> [String: Any] {
        guard let data = try? Data(contentsOf: settingsURL),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return [:] }
        return object
    }

    private static func write(_ object: [String: Any]) throws {
        let data: Data
        do {
            data = try JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
        } catch {
            throw CloudStoreError(message: "could not encode the cloud settings: \(error.localizedDescription)")
        }
        guard SecretFile.write(data, to: settingsURL) else {
            throw CloudStoreError(message: "could not write \(settingsURL.path)")
        }
        NotificationCenter.default.post(name: didChange, object: nil)
    }

    private static func consents() -> [String: [String: Any]] {
        settings()["consents"] as? [String: [String: Any]] ?? [:]
    }

    /// Whether the user accepted the provider's terms at `termsVersion`.
    static func hasConsent(_ id: CloudProviderID, termsVersion: Int) -> Bool {
        (consents()[id.rawValue]?["terms_version"] as? NSNumber)?.intValue == termsVersion
    }

    /// Records the acceptance of the provider's terms at `termsVersion`.
    static func recordConsent(_ id: CloudProviderID, termsVersion: Int) throws {
        var all = settings()
        var entries = consents()
        entries[id.rawValue] = ["terms_version": termsVersion,
                                "accepted_at": ISO8601DateFormatter().string(from: Date())]
        all["consents"] = entries
        try write(all)
    }

    /// Withdraws the provider's consent: nothing is sent to it until the card is accepted again.
    static func revokeConsent(_ id: CloudProviderID) throws {
        var all = settings()
        var entries = consents()
        entries[id.rawValue] = nil
        all["consents"] = entries
        try write(all)
    }

    /// The agent switch, off unless the user turned it on in Settings ▸ Cloud.
    static var agentMayUseCloudTools: Bool {
        (settings()["agent_cloud_tools"] as? NSNumber)?.boolValue ?? false
    }

    static func setAgentMayUseCloudTools(_ allowed: Bool) throws {
        var all = settings()
        all["agent_cloud_tools"] = allowed
        if all["consents"] == nil { all["consents"] = [String: Any]() }
        try write(all)
    }
}
