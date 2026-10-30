// Independent Phase 8G camera/mask/composition oracle. Foundation only; no app/core linkage.
import Foundation
struct OracleFailure: Error { let message: String }
func check(_ value: Bool, _ message: String) throws { if !value { throw OracleFailure(message: message) } }
func object(_ path: String) throws -> [String: Any] {
    guard let value = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: path))) as? [String: Any] else { throw OracleFailure(message: "Expected JSON object") }; return value
}
func samples(_ path: String, skip: Int = 0) throws -> [UInt16] {
    let data = try Data(contentsOf: URL(fileURLWithPath: path)); try check(data.count >= skip && (data.count-skip)%2 == 0, "Malformed independent sample file")
    return data.withUnsafeBytes { raw in Array(raw[skip...].bindMemory(to: UInt16.self)) }
}
func u16(_ data: [UInt8], _ n: Int) -> Int { Int(data[n]) * 256 + Int(data[n+1]) }
func u32(_ data: [UInt8], _ n: Int) -> Int { u16(data,n) * 65536 + u16(data,n+2) }
struct Transfer {
    var curve: [Double] = []; var kind = 0; var p: [Double] = []
    init(_ profile: [UInt8], channel: Int) throws {
        let signature = Array(["rTRC", "gTRC", "bTRC"][channel].utf8)
        var tag: [UInt8] = []
        for i in 0..<u32(profile,128) { let n=132+i*12
            if Array(profile[n..<n+4]) == signature { let s=u32(profile,n+4), c=u32(profile,n+8); tag=Array(profile[s..<s+c]); break }
        }
        try check(tag.count >= 12, "ICC missing native transfer function")
        if Array(tag[0..<4]) == Array("curv".utf8) {
            let count=u32(tag,8)
            if count == 0 { kind=0; p=[1] }
            else if count == 1 { kind=0; p=[Double(u16(tag,12))/256] }
            else { curve=(0..<count).map { Double(u16(tag,12+$0*2))/65535 } }
        } else {
            try check(Array(tag[0..<4]) == Array("para".utf8), "Unsupported independent ICC TRC")
            kind=u16(tag,8); try check(kind <= 4, "Unsupported ICC parametric function")
            let count=[1,3,4,5,7][kind]
            p=(0..<count).map { Double(Int32(bitPattern: UInt32(u32(tag,12+$0*4))))/65536 }
        }
    }
    func decode(_ encoded: Double) -> Double {
        let x=min(1,max(0,encoded))
        if !curve.isEmpty { let position=x*Double(curve.count-1), lo=Int(position), hi=min(lo+1,curve.count-1); return curve[lo]+(curve[hi]-curve[lo])*(position-Double(lo)) }
        if kind == 0 { return pow(x,p[0]) }
        switch kind {
        case 1: return x >= -p[2]/p[1] ? pow(p[1]*x+p[2],p[0]) : 0
        case 2: return x >= -p[2]/p[1] ? pow(p[1]*x+p[2],p[0])+p[3] : p[3]
        case 3: return x >= p[4] ? pow(p[1]*x+p[2],p[0]) : p[3]*x
        default: return x >= p[4] ? pow(p[1]*x+p[2],p[0])+p[5] : p[3]*x+p[6]
        }
    }
    func encode(_ linear: Double) -> Double {
        let y=min(1,max(0,linear)); if y <= decode(0) { return 0 }; if y >= decode(1) { return 1 }
        if !curve.isEmpty {
            var lo=0, hi=curve.count-1
            while hi-lo>1 { let m=(lo+hi)/2; if curve[m]<y { lo=m } else { hi=m } }
            return (Double(lo)+(y-curve[lo])/(curve[hi]-curve[lo]))/Double(curve.count-1)
        }
        if kind == 0 { return pow(y,1/p[0]) }
        let x: Double
        switch kind {
        case 1: x=(pow(y,1/p[0])-p[2])/p[1]
        case 2: x=(pow(max(0,y-p[3]),1/p[0])-p[2])/p[1]
        case 3: x=y >= pow(p[1]*p[4]+p[2],p[0]) ? (pow(y,1/p[0])-p[2])/p[1] : y/p[3]
        default: x=y >= pow(p[1]*p[4]+p[2],p[0])+p[5] ? (pow(max(0,y-p[5]),1/p[0])-p[2])/p[1] : (y-p[6])/p[3]
        }
        return min(1,max(0,x))
    }
}
func radial(_ x: Double, _ y: Double, _ cx: Double, _ cy: Double, _ rx: Double, _ ry: Double, _ h: Double) -> Double {
    let d=hypot((x-cx)/rx,(y-cy)/ry); if d <= h { return 1 }; if d >= 1 { return 0 }; let t=(d-h)/(1-h); return 1-t*t*(3-2*t)
}
func maskReference(_ index: Int, _ x: Double, _ y: Double) -> Double {
    switch index {
    case 0: return radial(x,y,1504,1000,1400,950,0.4)
    case 1: return min(1,max(0,((x-200)*2600+(y-150)*1700)/(2600*2600+1700*1700)))
    case 2: return radial(x,y,1700,700,650,500,0)
    default:
        let a=radial(x,y,800,1300,250,250,0.5)*0.72
        let b=radial(x,y,1000,1350,300,300,0.3)*0.51
        return a+b*(1-a)
    }
}
func compareSDK(_ actual: [UInt16], _ expected: [UInt16], _ name: String) throws -> [String: Any] {
    try check(actual.count == expected.count, "SDK geometry mismatch: \(name)")
    var maximum=0, different=0
    for i in actual.indices { let delta=abs(Int(actual[i])-Int(expected[i])); maximum=max(maximum,delta); if delta != 0 { different += 1 }; if i%4 == 3 { try check(actual[i] == 65535 && expected[i] == 65535, "Camera alpha must be exactly opaque") } }
    try check(maximum <= 3, "Independent SDK camera gate failed \(name): \(maximum) > 3")
    return ["name":name,"components":actual.count,"maximum_u16_error":maximum,"bound_u16":3,"different_components":different]
}

func compareExplicitOptics(_ input: [UInt16], _ output: [UInt16],
                           width: Int, height: Int, profile: [String: Any]) throws -> [String: Any] {
    try check(input.count == width * height * 4 && output.count == input.count,
        "Pinned camera optical samples have different dimensions")
    guard let center = profile["center"] as? [Double], center.count == 2,
          let radial = profile["radial"] as? [Double], radial.count == 3,
          let red = profile["red_scale"] as? Double,
          let blue = profile["blue_scale"] as? Double else {
        throw OracleFailure(message: "Pinned camera optical coefficients missing")
    }
    let c = (center[0] * Double(width), center[1] * Double(height))
    let radius = Double(max(width, height)) / 2
    var maximum = 0, displaced = [Int](repeating: 0, count: 3)
    for y in 0..<height {
        for x in 0..<width {
            let destination = (y * width + x) * 4
            let q = ((Double(x) + 0.5 - c.0) / radius,
                     (Double(y) + 0.5 - c.1) / radius)
            let r2 = q.0 * q.0 + q.1 * q.1
            let factor = 1 + r2 * (radial[0] + r2 * (radial[1] + r2 * radial[2]))
            var alpha = [Double](repeating: 0, count: 3)
            var values = [Double](repeating: 0, count: 3)
            for channel in 0..<3 {
                let scale = channel == 0 ? red : channel == 2 ? blue : 1
                let px = c.0 + radius * q.0 * factor * scale
                let py = c.1 + radius * q.1 * factor * scale
                let bx = Int(floor(px - 0.5)), by = Int(floor(py - 0.5))
                for ty in 0..<2 {
                    for tx in 0..<2 {
                        let sx = bx + tx, sy = by + ty
                        guard sx >= 0, sy >= 0, sx < width, sy < height else { continue }
                        let weight = max(0, 1 - abs(px - 0.5 - Double(sx)))
                            * max(0, 1 - abs(py - 0.5 - Double(sy)))
                        let source = (sy * width + sx) * 4
                        let a = Double(input[source + 3]) / 65535
                        alpha[channel] += a * weight
                        values[channel] += Double(input[source + channel]) / 65535 * a * weight
                    }
                }
            }
            let expectedAlpha = Int((min(1, alpha[1]) * 65535).rounded())
            maximum = max(maximum, abs(Int(output[destination + 3]) - expectedAlpha))
            for channel in 0..<3 {
                let expected = expectedAlpha == 0 || alpha[channel] == 0 ? 0
                    : Int((min(1, values[channel] / alpha[channel]) * 65535).rounded())
                maximum = max(maximum, abs(Int(output[destination + channel]) - expected))
                if abs(Int(output[destination + channel]) - Int(input[destination + channel])) >= 128 {
                    displaced[channel] += 1
                }
            }
        }
    }
    try check(maximum <= 1 && displaced.allSatisfy { $0 > 1024 },
        "Pinned Nikon optical exact-input reference error \(maximum), channel displacement \(displaced)")
    return ["compared_components": input.count, "max_native_error": maximum,
        "bound_u16": 1, "displaced_per_channel": displaced,
        "profile": profile]
}
do {
    let args=CommandLine.arguments; try check(args.count == 4,"oracle host-report.json reference-directory output.json")
    let report=try object(args[1]), reference=URL(fileURLWithPath:args[2]); var rows:[[String:Any]]=[]
    guard let cases=report["cameras"] as? [[String:Any]] else { throw OracleFailure(message:"Missing actual camera cases") }
    for item in cases {
        try autoreleasepool {
            let width=item["width"] as! Int, height=item["height"] as! Int, count=width*height
            let space=item["space"] as! String, depth=item["depth"] as! Int
            var sdk:[[String:Any]]=[]; var pixels=[[UInt16]](repeating:[],count:5); var profile:Data?
            let names=["clipped","recovered","warm","cool","neutral"]
            for input in item["inputs"] as! [[String:Any]] {
                let descriptor=input["descriptor"] as! [String:Any], recipe=descriptor["recipe"] as! [String:Any], values=recipe["values"] as! [String:Any]
                let exposure=(values["exposure"] as! NSNumber).intValue
                let index=[4:0,0:1,1:2,2:3,3:4][exposure]!
                pixels[index]=try samples(input["values"] as! String)
                let icc=try Data(contentsOf:URL(fileURLWithPath:input["profile"] as! String))
                if let previous=profile { try check(icc == previous,"Variants must share exact source ICC") } else { profile=icc }
                let name=names[index]+"-"+space
                sdk.append(try compareSDK(pixels[index], samples(reference.appendingPathComponent(name+".rgba16le").path), name))
            }
            for calibration in item["calibration"] as? [[String:Any]] ?? [] {
                let name=calibration["name"] as! String
                sdk.append(try compareSDK(samples(calibration["values"] as! String), samples(reference.appendingPathComponent(name+".rgba16le").path), name))
            }
            try check(pixels.allSatisfy{$0.count == count*4},"Missing complete actual product variant")
            let masks=try (item["masks"] as! [[String:Any]]).map { try samples($0["path"] as! String,skip:16) }
            try check(masks.count == 4 && masks.allSatisfy{$0.count == count},"Missing native mask plane")
            let mixed=try samples(item["mixed_values"] as! String), vignette=try samples(item["vignette_values"] as! String)
            let transfers=try (0..<3).map { try Transfer(Array(profile!),channel:$0) }
            let densities=[1.0,0.35,0.6,1.0], cx=Double(width)*0.45, cy=Double(height)*0.55
            let radius=max(hypot(0.5-cx,0.5-cy),hypot(Double(width)-0.5-cx,0.5-cy),hypot(0.5-cx,Double(height)-0.5-cy),hypot(Double(width)-0.5-cx,Double(height)-0.5-cy))
            var maskMaximum=[Int](repeating:0,count:4), mixMaximum=0.0, vignetteMaximum=0
            var outside=0, clippedRecovered=0, recovered=Set<UInt64>()
            for index in 0..<count {
                let x=Double(index%width)+0.5, y=Double(index/width)+0.5, offset=index*4
                for node in 0..<4 { let expected=Int((maskReference(node,x,y)*65535).rounded()); maskMaximum[node]=max(maskMaximum[node],abs(Int(masks[node][index])-expected)) }
                let isOutside=(0..<4).allSatisfy{masks[$0][index] == 0}
                if isOutside { outside += 1 }
                if masks[0][index] == 65535 && pixels[0][offset] == 65535 && pixels[0][offset+1] == 65535 && pixels[0][offset+2] == 65535 {
                    clippedRecovered += 1
                    recovered.insert(UInt64(pixels[1][offset])<<32 | UInt64(pixels[1][offset+1])<<16 | UInt64(pixels[1][offset+2]))
                }
                let gain=pow(2,0.5*pow(hypot(x-cx,y-cy)/radius,2))
                for channel in 0..<3 {
                    var value=Double(pixels[0][offset+channel])/65535
                    for node in 0..<4 { let coverage=Double(masks[node][index])/65535*densities[node]; value=value*(1-coverage)+Double(pixels[node+1][offset+channel])/65535*coverage }
                    mixMaximum=max(mixMaximum,abs(Double(mixed[offset+channel])/65535-value))
                    if isOutside { try check(mixed[offset+channel] == pixels[0][offset+channel],"Outside-mask camera base must remain bit exact") }
                    let expected=Int((transfers[channel].encode(transfers[channel].decode(value)*gain)*65535).rounded())
                    vignetteMaximum=max(vignetteMaximum,abs(Int(vignette[offset+channel])-expected))
                }
                try check(mixed[offset+3] == 65535 && vignette[offset+3] == 65535,"Local camera alpha changed")
            }
            let mixBound=2e-6+0.5/65535
            try check(maskMaximum.allSatisfy{$0 <= 1},"Independent native mask bound failed: \(maskMaximum)")
            try check(mixMaximum <= mixBound,"Exact-input ordered blend gate failed: \(mixMaximum) > \(mixBound)")
            try check(vignetteMaximum <= 3,"Independent profile-aware vignette gate failed: \(vignetteMaximum) > 3")
            try check(outside > 1000,"Fixture lacks exact outside-mask witnesses")
            try check(clippedRecovered > 1024 && recovered.count > 1024,"No original-derived detail beneath clipped developed base")
            let optical = try compareExplicitOptics(vignette,
                try samples(item["optical_values"] as! String), width: width, height: height,
                profile: item["optical_profile"] as! [String: Any])
            let row:[String:Any]=["space":space,"depth":depth,"sdk":sdk,"mask_maximum_u16_errors":maskMaximum,"mask_bound_u16":1,"exact_input_mix_maximum_error":mixMaximum,"mix_bound":mixBound,"vignette_maximum_u16_error":vignetteMaximum,"vignette_bound_u16":3,"exact_outside_pixels":outside,"clipped_base_recovered_pixels":clippedRecovered,"distinct_original_derived_recovered_rgb":recovered.count,"components_per_master":count*4,"pinned_nikon_explicit_optics":optical]
            rows.append(row)
            print("PASS independent camera pipeline",depth,space,"mix",mixMaximum,"vignette",vignetteMaximum,"masks",maskMaximum)
        }
    }
    try JSONSerialization.data(withJSONObject:["passed":true,"cases":rows,"uses_exact_product_variants_for_kernel_oracle":true,"quantization":"ordered float blend then ICC vignette then one u16 quantization","camera_decoder_claim":"original-derived detail; no claim about sensor-saturated values"],options:[.prettyPrinted,.sortedKeys]).write(to:URL(fileURLWithPath:args[3]))
} catch { FileHandle.standardError.write(Data("Independent camera pipeline failed: \(error)\n".utf8)); exit(1) }
