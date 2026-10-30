#if ARTWORK_CAPTURE_STANDALONE
import AppKit
import Foundation
var rows=[[String:Any]]()
let figures=URL(fileURLWithPath:"/tmp/rz-phase8e-text-capture-standalone")
do {
 try FileManager.default.createDirectory(at:figures,withIntermediateDirectories:true)
 for name in ["natural","wrap","center","tight_leading","tracking_baseline","transform","rtl","color_runs","intrinsic_opaque","intrinsic_opacity","decorations","decorations_transform","decorations_tight"] {
  var p=TextLayerPayload();p.rgba[3]=1
  if name=="wrap"{p.wrap=170};if name=="center"{p.alignment = .center}
  if name=="tight_leading"{p.leading=20};if name=="tracking_baseline"{p.tracking=2.25;p.baseline=3.75}
  if name=="rtl"{p.string="مرحبا بالعالم\nשלום";p.alignment = .right}
  if name=="color_runs"{p.runs=[(NSRange(location:0,length:1),[0.76,0.12,0.31,0.613]) ]}
  if name.contains("transform"){p.matrix=CGAffineTransform(a:0.91,b:0.23,c:-0.17,d:0.86,tx:0,ty:0)}
  if name.hasPrefix("intrinsic"){p.string="😀";p.underline=false;p.strike=false;p.rgba[3]=name=="intrinsic_opaque" ? 1:0.37}
  if name.hasPrefix("decorations"){p.string="😀👨‍👩‍👧‍👦\n😀";p.rgba[3]=0.37;if name=="decorations_tight"{p.leading=20;p.baseline=3.75}}
  let anchor=CGPoint(x:8.25,y:60.125)
  let d=try ArtworkTextCapture.capture(payload:p,anchor:anchor)
  let onlyDecor=name.hasPrefix("decorations")
  let reference=try textCaptureReference(payload:p,anchor:anchor,decorationsOnly:onlyDecor)
  let pixels=try textCaptureRendered(d,pathsOnly:onlyDecor)
  var report=textCaptureDifference(reference,pixels,width:d.width)
  report["name"]=name;report["parts"]=d.parts.count
  if ["color_runs","tight_leading","decorations_transform","decorations_tight"].contains(name) {
   try textCaptureSavePNG(reference,width:d.width,height:d.height,url:figures.appendingPathComponent(name+"-reference.png"))
   try textCaptureSavePNG(pixels,width:d.width,height:d.height,url:figures.appendingPathComponent(name+"-captured.png"))
  }
  if name.hasPrefix("intrinsic") {
   var opaque=p;opaque.rgba[3]=1
   let original=try textCaptureReference(payload:opaque,anchor:anchor)
   for part in d.parts {if case let .intrinsic(bytes,opacity)=part {
    report["intrinsic_exact_opaque_pixels"]=bytes==original;report["authored_opacity"]=opacity
   }}
  }
  if name.hasPrefix("intrinsic") {
   guard report["intrinsic_exact_opaque_pixels"] as? Bool == true else {throw DoctoredCoreError(message:"Intrinsic pixel parity failed")}
  } else {
   let a=report["reference_alpha_area"] as? Int ?? 0,b=report["captured_alpha_area"] as? Int ?? 0
   guard a>0,abs(Double(a-b))/Double(a)<=0.02,report["ink_pixels_outside_two_pixel_reference_neighborhood"] as? Int == 0 else {throw DoctoredCoreError(message:"Layout gate failed: \(name)")}
   if onlyDecor {guard report["max_alpha"] as? Int ?? 256 <= 2 else {throw DoctoredCoreError(message:"Decoration gate failed")}}
  }
  rows.append(report)
 }
 FileHandle.standardOutput.write(try JSONSerialization.data(withJSONObject:rows,options:[.prettyPrinted,.sortedKeys]))
} catch {FileHandle.standardError.write(Data((error.localizedDescription+"\n").utf8));exit(1)}
#else
import AppKit
import Foundation
_ = NSApplication.shared
AutomationAlerts.installIfEnabled()
NSApp.setActivationPolicy(.prohibited)
do {
    guard let path = ProcessInfo.processInfo.environment["DR_ARTWORK_TEXT_PROBE_DIR"] else {
        throw DoctoredCoreError(message: "Missing private text probe directory")
    }
    let directory = URL(fileURLWithPath: path)
    var report = try runArtworkTextCaptureProbe(directory: directory)
    #if ARTWORK_WORKFLOW_UI
    report["workflow_ui"] = try runArtworkUIWorkflowProbe(directory: directory)
    #endif
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: directory.appendingPathComponent("host-report.json"))
    FileHandle.standardOutput.write(try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]))
} catch {
    FileHandle.standardError.write(Data((error.localizedDescription + "\n").utf8))
    exit(1)
}
#endif
