import Foundation

// Cache keys include complete canonical camera/recipe identity; aliases do not duplicate
// accounting, and generation tickets reject late stores after clear/close.

/// Small immutable development cache. Coverage is excluded from its complete
/// camera/recipe key, so a brush gesture reuses the same genuine RAW variant.
/// Image owners remain in normal resource accounting; this is not hidden pixel
/// storage. A Develop session explicitly clears its own cache on teardown.
final class RawLocalVariantCache {
    private struct Entry {
        let source: RasterDocument
        let descriptor: RawSourceDescriptor
        let bytes: UInt64
        let aliases: Set<String>
        let payload: PrecisionEditPayload
    }
    private let lock = NSLock()
    private var entries: [String: Entry] = [:]
    private var order: [String] = []
    private var aliases: [String: String] = [:]
    private var generatedMasks: [RawLocalGeneratedKind: RasterRawLocalMask] = [:]
    private var generatedBaseKey: String?
    private var generation: UInt64 = 0
    private var hits = 0
    private var misses = 0
    private var closeObserver: NSObjectProtocol?
    static let shared = RawLocalVariantCache(clearOnClose: true)
    static let maximumBytes: UInt64 = 256 * 1024 * 1024
    static let maximumEntries = 5
    init(clearOnClose: Bool = false) {
        if clearOnClose {
            closeObserver = NotificationCenter.default.addObserver(forName: .imageDocumentDidClose,
                object: nil, queue: nil) { [weak self] _ in self?.clear() }
        }
    }
    deinit { if let closeObserver { NotificationCenter.default.removeObserver(closeObserver) } }
    func clear() {
        lock.lock(); defer { lock.unlock() }
        generation &+= 1
        entries.removeAll(); order.removeAll(); aliases.removeAll()
        generatedMasks.removeAll(); generatedBaseKey = nil
    }
    /// The generated-mask memo (`RawLocalGenerated`): at most one Vision mask per kind, all made
    /// from the base development `baseKey` names (`RawLocalEvaluation.baseKey`, which does not
    /// depend on the locals). It is what keeps a Develop session from running Vision again on
    /// every brush dab over a not-yet-applied generated local. A mask stored under another key
    /// drops the rest, and a refused generation stores nothing, so the memo of the last base that
    /// produced a mask survives a refusal. The masks are ordinary accounted owners.
    func generated(_ kind: RawLocalGeneratedKind, baseKey: String) -> RasterRawLocalMask? {
        lock.lock(); defer { lock.unlock() }
        return generatedBaseKey == baseKey ? generatedMasks[kind] : nil
    }
    func storeGenerated(_ mask: RasterRawLocalMask, kind: RawLocalGeneratedKind, baseKey: String,
                        ticket: UInt64) {
        lock.lock(); defer { lock.unlock() }
        guard ticket == generation else { return }
        if generatedBaseKey != baseKey { generatedMasks.removeAll(); generatedBaseKey = baseKey }
        generatedMasks[kind] = mask
    }
    func ticket() -> UInt64 { lock.lock(); defer { lock.unlock() }; return generation }
    func value(for key: String) -> (RasterDocument, RawSourceDescriptor)? {
        lock.lock(); defer { lock.unlock() }
        let canonical = aliases[key] ?? key
        guard let value = entries[canonical] else { misses += 1; return nil }
        hits += 1
        order.removeAll { $0 == canonical }; order.append(canonical)
        return (value.source, value.descriptor)
    }
    func store(_ source: RasterDocument, descriptor: RawSourceDescriptor, key: String, ticket: UInt64) throws {
        let canonical = try descriptor.json()
        let payload = try PrecisionEditPayload.admitted(bytes: UInt64(canonical.utf8.count + key.utf8.count) * 4 + 4096)
        let bytes = UInt64(source.width) * UInt64(source.height) * 8 + payload.bytes
        guard bytes <= Self.maximumBytes else { return }
        lock.lock(); defer { lock.unlock() }
        guard ticket == generation else { return }
        if let previous = entries[canonical] {
            for alias in previous.aliases { aliases.removeValue(forKey: alias) }
        }
        let names: Set<String> = [key, canonical]
        for name in names { aliases[name] = canonical }
        entries[canonical] = Entry(source: source, descriptor: descriptor, bytes: bytes, aliases: names, payload: payload)
        order.removeAll { $0 == canonical }; order.append(canonical)
        while entries.count > Self.maximumEntries || entries.values.reduce(0, { $0 + $1.bytes }) > Self.maximumBytes {
            guard let first = order.first else { break }
            order.removeFirst()
            if let removed = entries.removeValue(forKey: first) {
                for alias in removed.aliases { aliases.removeValue(forKey: alias) }
            }
        }
    }
    var diagnostics: [String: UInt64] {
        lock.lock(); defer { lock.unlock() }
        return ["entries": UInt64(entries.count), "bytes": entries.values.reduce(0, { $0 + $1.bytes }),
            "aliases": UInt64(aliases.count), "generation": generation, "hits": UInt64(hits), "misses": UInt64(misses)]
    }
    /// Bounded immutable input capture for source diagnostics and qualification.
    /// Aliases never duplicate entries or their accounted full-size images.
    func snapshot() -> [(RasterDocument, RawSourceDescriptor)] {
        lock.lock(); defer { lock.unlock() }
        return order.compactMap { entries[$0].map { ($0.source, $0.descriptor) } }
    }
}
