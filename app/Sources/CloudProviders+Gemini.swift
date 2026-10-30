import Foundation

/// Gemini's image model ("Nano Banana") through the Interactions API
/// (https://ai.google.dev/gemini-api/docs/image-generation): one `POST {base}/interactions` per
/// variation, header `x-goog-api-key`, a JSON body naming the pinned model, the prompt with the
/// masking instruction, the picture and the white-on-black mask as base64 PNGs, and the native
/// size's aspect ratio at `image_size` "1K", answered as a JPEG. The API takes no mask
/// parameter, so the second image only guides the model and the core composites the answer
/// through the selection's own mask.
/// The reply's picture is `output_image.data`, else the first `steps[]` `model_output` image's
/// `data`. The two-image masking, the 1K pixel sizes and the reply path are to be confirmed by
/// the owner's smoke test (the ADR's M3b section).
/// The API documents no per-request image count
/// (https://ai.google.dev/gemini-api/docs/image-generation#limitations: the model "won't always
/// follow the exact number of image outputs"), so it stays one picture a request, through
/// `CloudProviders.oneRequestPerVariation`, and a later request's failure is reported as the
/// answer's `shortReason`, never dropped.
struct GeminiImageProvider: GenerativeFillProvider {
    static let baseEnvironment = "DR_GEMINI_BASE_URL"
    static let defaultBase = "https://generativelanguage.googleapis.com/v1beta"
    /// What follows the prompt in the text part.
    static let instruction = "The first image is the picture. The second image is a mask of the "
        + "same size: repaint only its white area and keep every other pixel unchanged. The first "
        + "image's pixels under the white area are placeholders (black where the canvas was "
        + "extended), not part of the picture: paint new content there that continues it. Answer "
        + "one image of the whole picture."
    /// The 1K sizes the API answers and the aspect ratio each is asked by.
    static let sizes: [(width: Int, height: Int, ratio: String)] = [
        (1024, 1024, "1:1"), (832, 1248, "2:3"), (1248, 832, "3:2"), (864, 1184, "3:4"),
        (1184, 864, "4:3"), (896, 1152, "4:5"), (1152, 896, "5:4"), (768, 1344, "9:16"),
        (1344, 768, "16:9"), (1536, 672, "21:9"),
    ]

    let id = CloudProviderID.gemini
    let displayName = "Gemini"
    let model = "gemini-nano-banana-2.1"
    var nativeSizes: [(width: Int, height: Int)] { Self.sizes.map { ($0.width, $0.height) } }
    let maskStyle = CloudMaskStyle.whiteOnBlack
    let terms = CloudTerms(
        training: "On Gemini's free tier Google uses your inputs and outputs to improve its "
            + "products and people may review them; on a billed project it does not. Enable "
            + "billing on your Google AI Studio project before using Gemini.",
        retention: "Google keeps paid-tier prompts and responses for a limited time to detect abuse.",
        billing: "Each variation is one Gemini request billed to your own API key.",
        url: URL(string: "https://ai.google.dev/gemini-api/terms") ?? URL(fileURLWithPath: "/"),
        version: 1)

    /// The aspect ratio a native size is asked by; "1:1" for a size the table does not hold.
    static func ratio(width: Int, height: Int) -> String {
        sizes.first { $0.width == width && $0.height == height }?.ratio ?? "1:1"
    }

    func fill(_ request: GenerativeFillRequest, count: Int, key: String,
              cancelled: @escaping () -> Bool) throws -> GenerativeFillAnswer {
        let endpoint = try CloudProviders.baseURL(environment: Self.baseEnvironment,
                                                  fallback: Self.defaultBase)
            .appendingPathComponent("interactions")
        let body: [String: Any] = [
            "model": model,
            "input": [
                ["type": "text", "text": request.prompt + " " + Self.instruction],
                ["type": "image", "mime_type": "image/png", "data": request.image.base64EncodedString()],
                ["type": "image", "mime_type": "image/png", "data": request.mask.base64EncodedString()],
            ],
            // The Interactions API answers JPEG only: it refuses `image/png` here with HTTP 400
            // (the owner's live smoke run). `CloudProviders.decode` reads either through ImageIO.
            "response_format": [
                "type": "image", "mime_type": "image/jpeg",
                "aspect_ratio": Self.ratio(width: request.width, height: request.height),
                "image_size": "1K",
            ],
        ]
        let data = try JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
        return try CloudProviders.oneRequestPerVariation(count: count, cancelled: cancelled) {
            try send(data, to: endpoint, key: key, cancelled: cancelled)
        }
    }

    private func send(_ body: Data, to endpoint: URL, key: String,
                      cancelled: @escaping () -> Bool) throws -> Data {
        var http = URLRequest(url: endpoint, timeoutInterval: 300)
        http.httpMethod = "POST"
        http.setValue(key, forHTTPHeaderField: "x-goog-api-key")
        http.setValue("application/json", forHTTPHeaderField: "Content-Type")
        http.httpBody = body
        let (reply, response) = try CloudHTTP.send(http, provider: displayName, cancelled: cancelled)
        let object = try? JSONSerialization.jsonObject(with: reply) as? [String: Any]
        guard (200..<300).contains(response.statusCode) else {
            let message = ((object?["error"] as? [String: Any])?["message"] as? String)
                ?? HTTPURLResponse.localizedString(forStatusCode: response.statusCode)
            throw CloudProviderError(message: "Gemini refused the request (HTTP \(response.statusCode)): \(message)")
        }
        guard let picture = Self.picture(object) else {
            throw CloudProviderError(message: "Gemini's reply holds no image.")
        }
        return picture
    }

    /// The reply's picture: `output_image.data`, else the first `steps[]` entry of type
    /// `model_output` whose content holds an image's `data`.
    static func picture(_ object: [String: Any]?) -> Data? {
        if let output = object?["output_image"] as? [String: Any], let encoded = output["data"] as? String {
            return Data(base64Encoded: encoded)
        }
        for step in object?["steps"] as? [[String: Any]] ?? [] where step["type"] as? String == "model_output" {
            let content = (step["content"] as? [[String: Any]])
                ?? (step["content"] as? [String: Any]).map { [$0] } ?? []
            for item in content where item["type"] as? String == "image" {
                if let encoded = item["data"] as? String { return Data(base64Encoded: encoded) }
            }
        }
        return nil
    }
}
