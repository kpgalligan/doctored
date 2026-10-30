import Foundation

/// fal's FLUX.1 Fill [pro] inpainting endpoint
/// (https://fal.ai/docs/documentation/model-apis/common-parameters): one synchronous
/// `POST {base}/fal-ai/flux-pro/v1/fill` per variation with `Authorization: Key <key>`, always
/// `X-Fal-Store-IO: 0` and an output expiry of an hour, and a JSON body of the prompt, the picture
/// and the white-on-black mask as PNG data URIs, `num_images: 1`, PNG output and `sync_mode`, so
/// the picture comes back as a data URI in `images[].url` (the first is taken). An https URL
/// there is refused by name and never fetched. That the inputs take data URIs, that white is the
/// area filled and that the replies are data URIs are to be confirmed by the owner's smoke test
/// (the ADR's M3b section).
/// One request a variation: https://fal.ai/models/fal-ai/flux-pro/v1/fill/api lists
/// `num_images` 1–4, but the owner's live run on 2026-10-19 asked for 3 and got 1, so each
/// variation is its own request (`CloudProviders.oneRequestPerVariation`), and a later request's
/// failure is reported as the answer's `shortReason`. Billing is per image either way.
struct FalImageProvider: GenerativeFillProvider {
    static let baseEnvironment = "DR_FAL_BASE_URL"
    static let defaultBase = "https://fal.run"
    static let route = "fal-ai/flux-pro/v1/fill"

    let id = CloudProviderID.fal
    let displayName = "fal"
    let model = FalImageProvider.route
    let nativeSizes: [(width: Int, height: Int)] = [
        (1024, 1024), (1248, 832), (832, 1248), (1344, 768), (768, 1344),
    ]
    let maskStyle = CloudMaskStyle.whiteOnBlack
    let terms = CloudTerms(
        training: "fal's data terms (linked) govern any use of your inputs.",
        retention: "fal keeps request data for 30 days unless told not to; Doctored always sends "
            + "X-Fal-Store-IO: 0 and asks for outputs to expire after an hour.",
        billing: "Each run is billed by fal to your own API key.",
        url: URL(string: "https://fal.ai/docs/model-apis/data-retention") ?? URL(fileURLWithPath: "/"),
        version: 1)

    func fill(_ request: GenerativeFillRequest, count: Int, key: String,
              cancelled: @escaping () -> Bool) throws -> GenerativeFillAnswer {
        let endpoint = try CloudProviders.baseURL(environment: Self.baseEnvironment,
                                                  fallback: Self.defaultBase)
            .appendingPathComponent(Self.route)
        let body: [String: Any] = [
            "prompt": request.prompt,
            "image_url": CloudImageCodec.dataURI(request.image),
            "mask_url": CloudImageCodec.dataURI(request.mask),
            "num_images": 1,
            "output_format": "png",
            "sync_mode": true,
        ]
        var http = URLRequest(url: endpoint, timeoutInterval: 300)
        http.httpMethod = "POST"
        http.setValue("Key \(key)", forHTTPHeaderField: "Authorization")
        http.setValue("0", forHTTPHeaderField: "X-Fal-Store-IO")
        http.setValue("{\"expiration_duration_seconds\": 3600}",
                      forHTTPHeaderField: "X-Fal-Object-Lifecycle-Preference")
        http.setValue("application/json", forHTTPHeaderField: "Content-Type")
        http.httpBody = try JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
        return try CloudProviders.oneRequestPerVariation(count: count, cancelled: cancelled) {
            try send(http, cancelled: cancelled)
        }
    }

    /// One POST and its first data-URI picture.
    private func send(_ http: URLRequest, cancelled: @escaping () -> Bool) throws -> Data {
        let (reply, response) = try CloudHTTP.send(http, provider: displayName, cancelled: cancelled)
        let object = try? JSONSerialization.jsonObject(with: reply) as? [String: Any]
        guard (200..<300).contains(response.statusCode) else {
            let detail = object?["detail"]
            let message = (detail as? String)
                ?? ((detail as? [[String: Any]])?.first?["msg"] as? String)
                ?? HTTPURLResponse.localizedString(forStatusCode: response.statusCode)
            throw CloudProviderError(message: "fal refused the request (HTTP \(response.statusCode)): \(message)")
        }
        let urls = (object?["images"] as? [[String: Any]] ?? []).compactMap { $0["url"] as? String }
        if urls.contains(where: { !$0.hasPrefix("data:") }) {
            throw CloudProviderError(
                message: "fal answered a link instead of the picture; Doctored does not fetch it.")
        }
        guard let picture = urls.lazy.compactMap(CloudImageCodec.dataFromURI).first else {
            throw CloudProviderError(message: "fal's reply holds no image.")
        }
        return picture
    }
}
