#if ARTWORK_CAPTURE_STANDALONE
import AppKit
struct DoctoredCoreError: LocalizedError { let message:String; var errorDescription:String? {message} }
enum BackgroundWork {static let occupied=false}
enum RasterImage {static let maxResizePixels=64_000_000}
final class RasterResources {static let shared=RasterResources();func admit(scratch:UInt64)throws{if scratch>1_000_000_000{throw DoctoredCoreError(message:"probe admission refused")}}}
enum NativeArtworkPart {case path(CGPath,rgba:[Double]);case intrinsic(pixels:[UInt8],opacity:Double);case coverage(pixels:[UInt8],rgba:[Double])}
struct NativeArtworkDrawing {let width:Int;let height:Int;let offsetX:Int;let offsetY:Int;let parts:[NativeArtworkPart]}
enum ArtworkColor {static func rgba(_ color:NSColor)->[Double]? {guard let c=color.usingColorSpace(.sRGB)else{return nil};return[c.redComponent,c.greenComponent,c.blueComponent,c.alphaComponent]}}
struct TextLayerPayload {
 var string="A😀B👨‍👩‍👧‍👦C e\u{0301} fi\n漢字 مرحبا"
 var font="Helvetica Neue"
 var size:CGFloat=48
 var wrap:CGFloat=600
 var leading:CGFloat=0
 var tracking:CGFloat=0
 var baseline:CGFloat=0
 var alignment:NSTextAlignment = .left
 var underline=true
 var strike=true
 var rgba:[Double]=[0.12345,0.45678,0.78901,0.37]
 var matrix=CGAffineTransform.identity
 var runs:[(NSRange,[Double])]=[]
}
struct DescribedDrawingPlan {let width:Int;let height:Int;let offsetX:Int;let offsetY:Int;let draw:(CGContext)->Bool}
enum TextLayer {
 static func drawingPlan(_ p:TextLayerPayload,anchor:CGPoint,color:NSColor?=nil,layoutManager:NSLayoutManager?=nil)->DescribedDrawingPlan?{
  let m=layoutManager ?? NSLayoutManager();let paragraph=NSMutableParagraphStyle();paragraph.alignment=p.alignment
  if p.leading>0{paragraph.minimumLineHeight=p.leading;paragraph.maximumLineHeight=p.leading}
  let ink=color ?? NSColor(srgbRed:p.rgba[0],green:p.rgba[1],blue:p.rgba[2],alpha:p.rgba[3])
  var attrs:[NSAttributedString.Key:Any]=[.font:NSFont(name:p.font,size:p.size) ?? NSFont.systemFont(ofSize:p.size),.foregroundColor:ink,.paragraphStyle:paragraph,.kern:p.tracking,.baselineOffset:p.baseline]
  if p.underline{attrs[.underlineStyle]=NSUnderlineStyle.single.rawValue};if p.strike{attrs[.strikethroughStyle]=NSUnderlineStyle.single.rawValue}
  let s=NSTextStorage(string:p.string,attributes:attrs)
  for (range,c) in p.runs{s.addAttribute(.foregroundColor,value:NSColor(srgbRed:c[0],green:c[1],blue:c[2],alpha:c[3]),range:range)}
  let t=NSTextContainer(size:CGSize(width:p.wrap,height:10000));t.lineFragmentPadding=0;m.addTextContainer(t);s.addLayoutManager(m);m.ensureLayout(for:t)
  return DescribedDrawingPlan(width:640,height:400,offsetX:0,offsetY:0){ctx in
   ctx.translateBy(x:anchor.x,y:anchor.y);ctx.concatenate(p.matrix);ctx.setShouldAntialias(true);ctx.setAllowsFontSmoothing(false)
   return withExtendedLifetime(s){let range=m.glyphRange(for:t);m.drawBackground(forGlyphRange:range,at:.zero);m.drawGlyphs(forGlyphRange:range,at:.zero);return true}
  }
 }
}
enum Bitmap {
 static func drawFlipped(_ context:CGContext,height:Int,appKit:Bool,_ draw:(CGContext)->Bool)->Bool{
  context.translateBy(x:0,y:CGFloat(height));context.scaleBy(x:1,y:-1)
  NSGraphicsContext.saveGraphicsState();defer{NSGraphicsContext.restoreGraphicsState()};NSGraphicsContext.current=NSGraphicsContext(cgContext:context,flipped:true);return draw(context)
 }
 static func unpremultiply(_ p:inout[UInt8]){for i in stride(from:0,to:p.count,by:4){let a=Int(p[i+3]);if a==0{p[i]=0;p[i+1]=0;p[i+2]=0}else if a<255{for c in 0..<3{p[i+c]=UInt8(min(255,(Int(p[i+c])*255+a/2)/a))}}}}
}
#endif
