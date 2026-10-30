//! Independent classic-PSD construction and expected scalar/composite samples.
#![allow(dead_code)]
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Record {
    pub bounds: [i32; 4],
    pub channels: Vec<(i16, Vec<u8>)>,
    pub name: &'static str,
    pub divider: u32,
    pub opacity: u8,
    pub clipping: u8,
    pub flags: u8,
    pub blend: [u8; 4],
    pub mask_body: Vec<u8>,
    pub blocks: Vec<u8>,
    /// The layer blending ranges section's bytes, verbatim (empty = none).
    pub ranges: Vec<u8>,
}
pub fn raw(samples: &[u8]) -> Vec<u8> {
    [vec![0, 0], samples.to_vec()].concat()
}
pub fn rows(rows: &[&[u8]]) -> Vec<u8> {
    let mut bytes = vec![0, 1];
    for row in rows {
        bytes.extend((row.len() as u16).to_be_bytes());
    }
    for row in rows {
        bytes.extend(*row);
    }
    bytes
}
pub fn rle(samples: &[u8], width: usize) -> Vec<u8> {
    let encoded: Vec<Vec<u8>> = samples
        .chunks(width)
        .map(|row| {
            let mut out = vec![128, (row.len() - 1) as u8];
            out.extend(row);
            out
        })
        .collect();
    rows(&encoded.iter().map(Vec::as_slice).collect::<Vec<_>>())
}
pub fn block(key: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [
        b"8BIM".as_slice(),
        key,
        &(body.len() as u32).to_be_bytes(),
        body,
    ]
    .concat()
}
pub fn mask_body(bounds: [i32; 4], outside: u8, flags: u8) -> Vec<u8> {
    let mut body = Vec::new();
    for v in bounds {
        body.extend(v.to_be_bytes());
    }
    body.extend([outside, flags, 0, 0]);
    body
}
impl Record {
    pub fn raster(bounds: [i32; 4], rgba: &[u8], grayscale: bool) -> Self {
        let ids: &[i16] = if grayscale { &[0, -1] } else { &[0, 1, 2, -1] };
        Self {
            bounds,
            channels: ids
                .iter()
                .map(|&id| {
                    let component = if id == -1 { 3 } else { id as usize };
                    (
                        id,
                        raw(&rgba
                            .chunks_exact(4)
                            .map(|p| p[component])
                            .collect::<Vec<_>>()),
                    )
                })
                .collect(),
            name: "Duplicate",
            divider: 0,
            opacity: 255,
            clipping: 0,
            flags: 8,
            blend: *b"norm",
            mask_body: vec![],
            blocks: vec![],
            ranges: vec![],
        }
    }
    pub fn folder(divider: u32, blend: [u8; 4]) -> Self {
        Self {
            bounds: [0; 4],
            channels: vec![],
            name: "Duplicate",
            divider,
            opacity: 255,
            clipping: 0,
            flags: 8,
            blend,
            mask_body: vec![],
            blocks: vec![],
            ranges: vec![],
        }
    }
    pub fn mask(mut self, mask: &ExpectedMask, compressed: bool) -> Self {
        self.mask_body = mask_body(mask.bounds, mask.outside, if mask.enabled { 0 } else { 2 });
        self.channels.insert(
            0,
            (
                -2,
                if compressed {
                    rle(&mask.coverage, (mask.bounds[3] - mask.bounds[1]) as usize)
                } else {
                    raw(&mask.coverage)
                },
            ),
        );
        self
    }
    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for v in self.bounds {
            bytes.extend(v.to_be_bytes());
        }
        bytes.extend((self.channels.len() as u16).to_be_bytes());
        for (id, channel) in &self.channels {
            bytes.extend(id.to_be_bytes());
            bytes.extend((channel.len() as u32).to_be_bytes());
        }
        bytes.extend(b"8BIM");
        bytes.extend(self.blend);
        bytes.extend([self.opacity, self.clipping, self.flags, 0]);
        let mut extra = (self.mask_body.len() as u32).to_be_bytes().to_vec();
        extra.extend(&self.mask_body);
        extra.extend((self.ranges.len() as u32).to_be_bytes());
        extra.extend(&self.ranges);
        let mut name = vec![self.name.len() as u8];
        name.extend(self.name.as_bytes());
        while !name.len().is_multiple_of(4) {
            name.push(0);
        }
        extra.extend(name);
        if self.divider != 0 {
            extra.extend(block(b"lsct", &self.divider.to_be_bytes()));
        }
        extra.extend(&self.blocks);
        bytes.extend((extra.len() as u32).to_be_bytes());
        bytes.extend(extra);
        bytes
    }
}
pub fn fixture(records: &[Record], canvas: [u32; 2], grayscale: bool) -> Vec<u8> {
    let mut info = (records.len() as i16).to_be_bytes().to_vec();
    for record in records {
        info.extend(record.encode());
    }
    for record in records {
        for (_, channel) in &record.channels {
            info.extend(channel);
        }
    }
    if !info.len().is_multiple_of(2) {
        info.push(0);
    }
    let mut bytes = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    bytes.extend((if grayscale { 1u16 } else { 3 }).to_be_bytes());
    bytes.extend(canvas[1].to_be_bytes());
    bytes.extend(canvas[0].to_be_bytes());
    bytes.extend(8u16.to_be_bytes());
    bytes.extend((if grayscale { 1u16 } else { 3 }).to_be_bytes());
    bytes.extend([0; 8]);
    bytes.extend(((info.len() + 8) as u32).to_be_bytes());
    bytes.extend((info.len() as u32).to_be_bytes());
    bytes.extend(info);
    bytes.extend([0; 6]);
    let pixels = canvas[0] as usize * canvas[1] as usize;
    bytes.extend(vec![13; pixels]);
    if !grayscale {
        bytes.extend(vec![27; pixels]);
        bytes.extend(vec![41; pixels]);
    }
    bytes
}
#[derive(Clone)]
pub struct ExpectedMask {
    pub bounds: [i32; 4],
    pub coverage: Vec<u8>,
    pub outside: u8,
    pub enabled: bool,
}
impl ExpectedMask {
    pub fn at(&self, x: i32, y: i32) -> u8 {
        if !self.enabled {
            return 255;
        }
        let [top, left, bottom, right] = self.bounds;
        if x < left || x >= right || y < top || y >= bottom {
            return self.outside;
        }
        self.coverage[((i64::from(y) - i64::from(top)) * (i64::from(right) - i64::from(left))
            + i64::from(x)
            - i64::from(left)) as usize]
    }
    pub fn json(&self) -> Value {
        let [top, left, bottom, right] = self.bounds;
        json!({"offset":[left,top],"size":[right-left,bottom-top],"coverage":self.coverage,
            "enabled":self.enabled,"outside_coverage":self.outside,"linked":false,"density":1.0,"feather":0.0})
    }
}
pub fn normal_projection(
    canvas: [u32; 2],
    bounds: [i32; 4],
    rgba: &[u8],
    opacity: u8,
    mask: Option<&ExpectedMask>,
) -> Vec<u8> {
    let [top, left, bottom, right] = bounds;
    let mut out = Vec::new();
    for y in 0..canvas[1] as i32 {
        for x in 0..canvas[0] as i32 {
            if x < left || x >= right || y < top || y >= bottom {
                out.extend([0; 4]);
                continue;
            }
            let i = ((i64::from(y) - i64::from(top)) * (i64::from(right) - i64::from(left))
                + i64::from(x)
                - i64::from(left)) as usize
                * 4;
            let alpha = f64::from(rgba[i + 3]) * f64::from(opacity) / 255.
                * f64::from(mask.map_or(255, |m| m.at(x, y)))
                / 255.;
            if alpha == 0. {
                out.extend([0; 4]);
            } else {
                out.extend(&rgba[i..i + 3]);
                out.push(alpha.round() as u8);
            }
        }
    }
    out
}
pub fn layer_json(record: &Record, depth: u16, rgba: &[u8], mask: Option<&ExpectedMask>) -> Value {
    let [top, left, bottom, right] = record.bounds;
    let group = record.divider != 0;
    json!({"name":record.name,"kind":if group {"group"} else {"raster"},"depth":depth,
        "offset":if group {mask.map_or([0,0],|m|[m.bounds[1],m.bounds[0]])} else {[left,top]},
        "size":if group {[1,1]} else {[right-left,bottom-top]},"rgba":rgba,
        "visible":record.flags&2==0,"opacity":f64::from(record.opacity)/255.,
        "blend":match &record.blend {b"pass"=>"Pass Through",b"mul "=>"Multiply",_=>"Normal"},
        "open":record.divider!=2,"mask":mask.map(ExpectedMask::json)})
}
pub fn stored(canvas: [u32; 2], gray: bool) -> Vec<u8> {
    (if gray {
        [13, 13, 13, 255]
    } else {
        [13, 27, 41, 255]
    })
    .repeat(canvas[0] as usize * canvas[1] as usize)
}
pub fn export(name: &str, bytes: &[u8], expected: &Value) {
    if let Ok(directory) = std::env::var("DR_PSD_MASK_FIXTURES") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(format!("{name}.psd")), bytes).unwrap();
        std::fs::write(
            directory.join(format!("{name}.psd.json")),
            serde_json::to_vec_pretty(expected).unwrap(),
        )
        .unwrap();
    }
}
