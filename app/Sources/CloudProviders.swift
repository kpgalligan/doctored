import AppKit
import ImageIO
import UniformTypeIdentifiers

// The cloud image providers (`designs/neural-models.md` architecture decision 2): one
// protocol every provider answers, so Generative Fill (`GenerativeFillRun`) never names one.
// OpenAI's image edit API (phase 1), Gemini (CloudProviders+Gemini.swift) and fal
// (CloudProviders+Fal.swift), each through the user's own key. The request runs on the job's
// worker thread (`CloudHTTP.send`), never on main.

enum CloudProviderID: String, CaseIterable, Codable {
    case openai, gemini, fal
}

/// How a provider reads the core's two pictures: OpenAI fills the TRANSPARENT cells of an RGBA
/// mask and is sent the image with the same cells transparent (GPT Image masking is
/// prompt-based, so an opaque hole reads as content to keep); Gemini and fal read an opaque
/// picture that is WHITE where they repaint and black elsewhere (`CloudImageCodec.maskPNG`) and
/// are sent the image opaque (`GenerativeFillRun.request`).
enum CloudMaskStyle {
    case transparentHole, whiteOnBlack
}

/// What a provider does with what it is sent, as the consent card quotes it.
struct CloudTerms {
    let training: String
    let retention: String
    let billing: String
    let url: URL
    /// Bumped when the quoted terms change, so the card is shown again.
    let version: Int
}

/// One Generative Fill request: the two PNG pictures at `width × height` and the prompt sent.
struct GenerativeFillRequest {
    let image: Data
    let mask: Data
    let prompt: String
    let width: Int
    let height: Int
}

/// A provider's answer: the encoded pictures that came and, when they are fewer than the count
/// asked for, the provider's own sentence saying why.
struct GenerativeFillAnswer {
    let pictures: [Data]
    /// The provider's own sentence when pictures.count < the count asked, else nil.
    let shortReason: String?
}

protocol GenerativeFillProvider {
    var id: CloudProviderID { get }
    var displayName: String { get }
    /// The pinned model id the request names.
    var model: String { get }
    /// The sizes the provider answers at; the core picks the one nearest the selection's aspect.
    var nativeSizes: [(width: Int, height: Int)] { get }
    /// How the provider reads the mask picture (the app composites whatever comes back).
    var maskStyle: CloudMaskStyle { get }
    var terms: CloudTerms { get }
    /// Sends `request` with the user's `key` for `count` variations (1–3) and answers the
    /// provider's pictures as encoded image bytes; it may answer fewer than asked, and then says
    /// why in `shortReason`. Worker thread only; `cancelled` is polled while a request is in
    /// flight.
    func fill(_ request: GenerativeFillRequest, count: Int, key: String,
              cancelled: @escaping () -> Bool) throws -> GenerativeFillAnswer
}

enum CloudProviders {
    static let all: [GenerativeFillProvider] = [OpenAIImageProvider(), GeminiImageProvider(),
                                                FalImageProvider()]

    static func provider(_ id: CloudProviderID) -> GenerativeFillProvider {
        switch id {
        case .openai: return OpenAIImageProvider()
        case .gemini: return GeminiImageProvider()
        case .fal: return FalImageProvider()
        }
    }

    /// The base URL `environment` names when set, else `fallback`. Only https, or http on
    /// 127.0.0.1 or localhost (a drive's own stub server), is accepted, for every provider.
    static func baseURL(environment: String, fallback: String) throws -> URL {
        let raw = ProcessInfo.processInfo.environment[environment].flatMap { $0.isEmpty ? nil : $0 }
            ?? fallback
        guard let url = URL(string: raw), let scheme = url.scheme?.lowercased(), let host = url.host
        else {
            throw CloudProviderError(message: "\(environment) is not a URL: \(raw)")
        }
        let local = host == "127.0.0.1" || host.lowercased() == "localhost"
        guard scheme == "https" || (scheme == "http" && local) else {
            throw CloudProviderError(
                message: "\(environment) must be an https URL, or http on 127.0.0.1 or localhost "
                    + "(got \(raw)).")
        }
        return url
    }

    /// `max(1, count)` sequential `send` calls, one picture each (Gemini and fal, which answer one
    /// picture a request). `JobCancelled` always rethrows; the first call's error throws; a later
    /// error stops the loop and its sentence becomes the answer's `shortReason`, so a short set
    /// is never silent. No retry.
    static func oneRequestPerVariation(count: Int, cancelled: () -> Bool,
                                       send: () throws -> Data) throws -> GenerativeFillAnswer {
        var pictures: [Data] = []
        for _ in 0..<max(1, count) {
            guard !cancelled() else { throw JobCancelled() }
            do {
                pictures.append(try send())
            } catch let error as JobCancelled {
                throw error
            } catch {
                if pictures.isEmpty { throw error }
                return GenerativeFillAnswer(pictures: pictures,
                                            shortReason: error.localizedDescription)
            }
        }
        return GenerativeFillAnswer(pictures: pictures, shortReason: nil)
    }

    /// "<provider> answered <received> of the <requested> pictures asked for."
    static func shortSentence(provider: String, received: Int, requested: Int) -> String {
        "\(provider) answered \(received) of the \(requested) pictures asked for."
    }
}

struct CloudProviderError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// OpenAI's image edit API (`POST /images/edits`, multipart): `model`, `prompt`, `n` (the count),
/// `size="WxH"`, `background="opaque"`, `image` and `mask` PNGs; `Authorization: Bearer <key>`;
/// the reply's `data[].b64_json`. The request shape and the model id are recorded in the ADR with
/// the doc URL they come from. The image's hole (and Expand's border) is sent TRANSPARENT, like
/// the mask's: OpenAI's guide (https://developers.openai.com/api/docs/guides/image-generation,
/// "Edit an image using a mask" and "Customize Image Output") says GPT Image masking is
/// prompt-based and that transparent pixels are the edit area, and the owner's live expand run
/// (2026-10-19) sent an opaque black border and got black back. `background="opaque"` asks for
/// an answer with no transparent pixels, since the core composites it opaque.
struct OpenAIImageProvider: GenerativeFillProvider {
    static let baseEnvironment = "DR_OPENAI_BASE_URL"
    static let defaultBase = "https://api.openai.com/v1"
    /// The prompt an empty prompt field sends (a fill from context alone).
    static let contextPrompt = "Fill the masked area so it continues the surrounding image "
        + "naturally, matching its content, lighting, perspective and grain. Add no new objects or text."

    let id = CloudProviderID.openai
    let displayName = "OpenAI"
    /// OpenAI's 2.5 image line is two models, `gpt-image-2.5-sunburst` (the full one) and
    /// `gpt-image-2.5-flare`; a bare `gpt-image-2.5` is refused with HTTP 400.
    let model = "gpt-image-2.5-sunburst"
    let nativeSizes: [(width: Int, height: Int)] = [(1024, 1024), (1536, 1024), (1024, 1536)]
    let maskStyle = CloudMaskStyle.transparentHole
    let terms = CloudTerms(
        training: "OpenAI does not train its models on API inputs or outputs.",
        retention: "OpenAI keeps API requests in its abuse-monitoring logs for up to 30 days.",
        billing: "Each Generative Fill is billed by OpenAI to your own API key.",
        url: URL(string: "https://developers.openai.com/api/docs/guides/your-data")
            ?? URL(fileURLWithPath: "/"),
        version: 1)

    /// `DR_OPENAI_BASE_URL` when set, else OpenAI's own (`CloudProviders.baseURL`).
    static func baseURL() throws -> URL {
        try CloudProviders.baseURL(environment: baseEnvironment, fallback: defaultBase)
    }

    func fill(_ request: GenerativeFillRequest, count: Int, key: String,
              cancelled: @escaping () -> Bool) throws -> GenerativeFillAnswer {
        let endpoint = try Self.baseURL().appendingPathComponent("images/edits")
        var form = MultipartForm()
        form.field("model", model)
        form.field("prompt", request.prompt)
        form.field("n", "\(count)")
        form.field("size", "\(request.width)x\(request.height)")
        form.field("background", "opaque")
        form.file("image", filename: "image.png", type: "image/png", data: request.image)
        form.file("mask", filename: "mask.png", type: "image/png", data: request.mask)
        var http = URLRequest(url: endpoint, timeoutInterval: 300)
        http.httpMethod = "POST"
        http.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        http.setValue(form.contentType, forHTTPHeaderField: "Content-Type")
        http.httpBody = form.finished()
        let (body, response) = try CloudHTTP.send(http, provider: displayName, cancelled: cancelled)
        let object = try? JSONSerialization.jsonObject(with: body) as? [String: Any]
        guard (200..<300).contains(response.statusCode) else {
            let message = ((object?["error"] as? [String: Any])?["message"] as? String)
                ?? HTTPURLResponse.localizedString(forStatusCode: response.statusCode)
            throw CloudProviderError(message: "OpenAI refused the request (HTTP \(response.statusCode)): \(message)")
        }
        let items = object?["data"] as? [[String: Any]] ?? []
        let pictures = items.compactMap { ($0["b64_json"] as? String).flatMap { Data(base64Encoded: $0) } }
        guard !pictures.isEmpty else {
            throw CloudProviderError(message: "OpenAI's reply holds no image.")
        }
        let requested = max(1, count)
        let short = pictures.count < requested
            ? CloudProviders.shortSentence(provider: displayName, received: pictures.count,
                                           requested: requested)
            : nil
        return GenerativeFillAnswer(pictures: pictures, shortReason: short)
    }
}

/// A multipart/form-data body.
private struct MultipartForm {
    private let boundary = "doctored-" + UUID().uuidString
    private var body = Data()

    var contentType: String { "multipart/form-data; boundary=\(boundary)" }

    mutating func field(_ name: String, _ value: String) {
        body.append(Data("--\(boundary)\r\nContent-Disposition: form-data; name=\"\(name)\"\r\n\r\n".utf8))
        body.append(Data(value.utf8))
        body.append(Data("\r\n".utf8))
    }

    mutating func file(_ name: String, filename: String, type: String, data: Data) {
        body.append(Data(("--\(boundary)\r\nContent-Disposition: form-data; name=\"\(name)\"; "
            + "filename=\"\(filename)\"\r\nContent-Type: \(type)\r\n\r\n").utf8))
        body.append(data)
        body.append(Data("\r\n".utf8))
    }

    mutating func finished() -> Data {
        body.append(Data("--\(boundary)--\r\n".utf8))
        return body
    }
}

/// The worker's mailbox for one request's completion, filled on the session's delegate queue.
private final class CloudReply {
    private let lock = NSLock()
    private var value: (Data?, URLResponse?, Error?)?
    func set(_ data: Data?, _ response: URLResponse?, _ error: Error?) {
        lock.lock(); value = (data, response, error); lock.unlock()
    }
    var result: (Data?, URLResponse?, Error?)? { lock.lock(); defer { lock.unlock() }; return value }
}

/// One HTTP request for a cloud provider. The completion lands on the ephemeral session's
/// delegate queue (the documented `URLSession` seam); the job's worker waits for it in 100 ms
/// slices, polling `cancelled`, and a cancel cancels the task. Nothing about the request (its
/// key above all) is logged.
enum CloudHTTP {
    static func send(_ request: URLRequest, provider: String,
                     cancelled: () -> Bool) throws -> (Data, HTTPURLResponse) {
        let session = URLSession(configuration: .ephemeral)
        defer { session.finishTasksAndInvalidate() }
        let reply = CloudReply()
        let done = DispatchSemaphore(value: 0)
        let task = session.dataTask(with: request) { data, response, error in
            reply.set(data, response, error)
            done.signal()
        }
        task.resume()
        while done.wait(timeout: .now() + .milliseconds(100)) == .timedOut {
            if cancelled() {
                task.cancel()
                throw JobCancelled()
            }
        }
        guard let (data, response, error) = reply.result else {
            throw CloudProviderError(message: "\(provider) answered nothing.")
        }
        if let error {
            if (error as? URLError)?.code == .cancelled { throw JobCancelled() }
            throw CloudProviderError(message: "Could not reach \(provider): \(error.localizedDescription)")
        }
        guard let http = response as? HTTPURLResponse else {
            throw CloudProviderError(message: "\(provider) answered something other than HTTP.")
        }
        return (data ?? Data(), http)
    }
}

/// The two PNG legs: the core's straight RGBA8 pictures to PNG bytes and the provider's
/// picture back to straight RGBA8, with no colour conversion either way (the picture is in the
/// document's encoding both ways; the PNG is tagged sRGB only so a decoder has a space).
enum CloudImageCodec {
    /// `rgba` (`width × height`, straight, row 0 = top) as PNG; `opaque` drops the alpha channel.
    static func png(rgba: [UInt8], width: Int, height: Int, opaque: Bool) throws -> Data {
        let info = opaque ? CGImageAlphaInfo.noneSkipLast : CGImageAlphaInfo.last
        guard rgba.count == width * height * 4,
              let provider = CGDataProvider(data: Data(rgba) as CFData),
              let image = CGImage(
                width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: width * 4, space: ColorProfile.sRGB,
                bitmapInfo: CGBitmapInfo(rawValue: info.rawValue), provider: provider, decode: nil,
                shouldInterpolate: false, intent: .defaultIntent) else {
            throw CloudProviderError(message: "Could not build the Generative Fill picture.")
        }
        return try encode(image)
    }

    /// `image` as PNG bytes.
    private static func encode(_ image: CGImage) throws -> Data {
        let bytes = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(bytes, UTType.png.identifier as CFString, 1, nil)
        else { throw CloudProviderError(message: "Could not encode the Generative Fill picture.") }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else {
            throw CloudProviderError(message: "Could not finish the Generative Fill picture.")
        }
        return bytes as Data
    }

    /// The core's mask picture (`rgba`: (255, 255, 255, 0) on a hole cell, (0, 0, 0, 255) on a
    /// kept one) as the PNG `style` asks for: as it is for `.transparentHole`; for
    /// `.whiteOnBlack` drawn over opaque white with CoreGraphics, so a hole reads white and every
    /// kept cell black, opaque (no Swift pixel loop).
    static func maskPNG(rgba: [UInt8], width: Int, height: Int, style: CloudMaskStyle) throws -> Data {
        switch style {
        case .transparentHole:
            return try png(rgba: rgba, width: width, height: height, opaque: false)
        case .whiteOnBlack:
            guard rgba.count == width * height * 4,
                  let provider = CGDataProvider(data: Data(rgba) as CFData),
                  let mask = CGImage(
                    width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                    bytesPerRow: width * 4, space: ColorProfile.sRGB,
                    bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                    provider: provider, decode: nil, shouldInterpolate: false,
                    intent: .defaultIntent),
                  let context = CGContext(
                    data: nil, width: width, height: height, bitsPerComponent: 8,
                    bytesPerRow: width * 4, space: ColorProfile.sRGB,
                    bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else {
                throw CloudProviderError(message: "Could not build the Generative Fill mask.")
            }
            let frame = CGRect(x: 0, y: 0, width: width, height: height)
            context.setFillColor(CGColor(srgbRed: 1, green: 1, blue: 1, alpha: 1))
            context.fill(frame)
            context.interpolationQuality = .none
            context.draw(mask, in: frame)
            guard let drawn = context.makeImage() else {
                throw CloudProviderError(message: "Could not build the Generative Fill mask.")
            }
            return try encode(drawn)
        }
    }

    /// `png` as a `data:image/png;base64,…` URI (fal's image inputs).
    static func dataURI(_ png: Data) -> String {
        "data:image/png;base64," + png.base64EncodedString()
    }

    /// The bytes of a `data:<type>;base64,<payload>` URI; nil for anything else.
    static func dataFromURI(_ uri: String) -> Data? {
        guard uri.hasPrefix("data:"), let comma = uri.firstIndex(of: ","),
              uri[..<comma].hasSuffix(";base64") else { return nil }
        return Data(base64Encoded: String(uri[uri.index(after: comma)...]))
    }

    /// The provider's encoded picture as straight RGBA8 in its own space (`Bitmap.ingestSpace`,
    /// which converts nothing), each side at most `maxSide`.
    static func decode(_ data: Data, maxSide: Int = 4096) throws -> (rgba: [UInt8], width: Int, height: Int) {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw CloudProviderError(message: "The provider's picture could not be decoded.")
        }
        guard image.width >= 1, image.height >= 1, image.width <= maxSide, image.height <= maxSide else {
            throw CloudProviderError(
                message: "The provider's picture is \(image.width)×\(image.height); at most \(maxSide) a side is read.")
        }
        guard let pixels = Bitmap.straightRGBA(
            from: image, fitting: image.width, height: image.height,
            space: Bitmap.ingestSpace(of: image).space) else {
            throw CloudProviderError(message: "The provider's picture could not be read.")
        }
        return (pixels, image.width, image.height)
    }
}
