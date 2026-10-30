//! A hand-assembled four-component baseline JPEG of ONE uniform sample per
//! component (`open_profile_tests`' CMYK/YCCK fixtures): SOF0, a DQT
//! of all ones, one DC table for categories 0…11 and an AC table holding
//! only EOB, every 8 × 8 block DC-only. A uniform block's DC coefficient is
//! `8·(s − 128)`, which a DC-only IDCT turns back into `s` exactly, and every
//! block after a component's first codes a DC difference of 0. Optional
//! segments: Adobe APP14 (with its colour transform), APP2 ICC (chunked as
//! ICC.1 Annex B) and APP1 EXIF. Written from ITU-T T.81, not from any
//! decoder in this crate.
#![allow(dead_code)]

/// What to put around the scan.
#[derive(Default)]
pub struct CmykJpeg<'a> {
    /// The Adobe APP14 colour transform (0 CMYK, 2 YCCK), or no APP14.
    pub adobe_transform: Option<u8>,
    pub icc: Option<&'a [u8]>,
    /// An EXIF TIFF stream (without the `Exif\0\0` prefix).
    pub exif: Option<&'a [u8]>,
}

/// A `w × h` four-component JPEG whose every pixel stores `samples`, in
/// component order (C, M, Y, K — or Y, Cb, Cr, K under transform 2).
pub fn cmyk_jpeg(w: u16, h: u16, samples: [u8; 4], extra: &CmykJpeg) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    if let Some(exif) = extra.exif {
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(exif);
        segment(&mut out, 0xE1, &payload);
    }
    if let Some(icc) = extra.icc {
        let chunks: Vec<&[u8]> = icc.chunks(65519 - 14).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let mut payload = b"ICC_PROFILE\0".to_vec();
            payload.push((i + 1) as u8);
            payload.push(chunks.len() as u8);
            payload.extend_from_slice(chunk);
            segment(&mut out, 0xE2, &payload);
        }
    }
    if let Some(transform) = extra.adobe_transform {
        let mut payload = b"Adobe".to_vec();
        payload.extend_from_slice(&100u16.to_be_bytes()); // version
        payload.extend_from_slice(&0u16.to_be_bytes()); // flags0
        payload.extend_from_slice(&0u16.to_be_bytes()); // flags1
        payload.push(transform);
        segment(&mut out, 0xEE, &payload);
    }
    // DQT: table 0, 8-bit, every entry 1.
    let mut dqt = vec![0x00];
    dqt.extend_from_slice(&[1u8; 64]);
    segment(&mut out, 0xDB, &dqt);
    // SOF0: 8-bit, h × w, four components, 1×1 sampling, table 0.
    let mut sof = vec![8];
    sof.extend_from_slice(&h.to_be_bytes());
    sof.extend_from_slice(&w.to_be_bytes());
    sof.push(4);
    for id in 1..=4u8 {
        sof.extend_from_slice(&[id, 0x11, 0]);
    }
    segment(&mut out, 0xC0, &sof);
    // DHT: DC table 0 = twelve 4-bit codes for categories 0…11 (code i is
    // category i); AC table 0 = one 1-bit code, EOB.
    let mut dht = vec![0x00];
    let mut bits = [0u8; 16];
    bits[3] = 12;
    dht.extend_from_slice(&bits);
    dht.extend(0..12u8);
    dht.push(0x10);
    let mut bits = [0u8; 16];
    bits[0] = 1;
    dht.extend_from_slice(&bits);
    dht.push(0x00);
    segment(&mut out, 0xC4, &dht);
    // SOS: all four components, tables 0/0, full spectral range.
    let mut sos = vec![4];
    for id in 1..=4u8 {
        sos.extend_from_slice(&[id, 0x00]);
    }
    sos.extend_from_slice(&[0, 63, 0]);
    segment(&mut out, 0xDA, &sos);

    let blocks = usize::from(w).div_ceil(8) * usize::from(h).div_ceil(8);
    let mut writer = BitWriter::default();
    for block in 0..blocks {
        for s in samples {
            let diff = if block == 0 {
                8 * (i32::from(s) - 128)
            } else {
                0
            };
            let category = 32 - diff.unsigned_abs().leading_zeros();
            writer.put(category, 4);
            if category > 0 {
                let value = if diff < 0 {
                    diff + (1 << category) - 1
                } else {
                    diff
                };
                writer.put(value as u32, category);
            }
            writer.put(0, 1); // EOB
        }
    }
    out.extend_from_slice(&writer.finish());
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

fn segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
}

/// MSB-first entropy-coded bits with 0xFF byte stuffing; the last byte is
/// padded with 1-bits.
#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    current: u32,
    filled: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.current = (self.current << 1) | ((value >> i) & 1);
            self.filled += 1;
            if self.filled == 8 {
                self.emit();
            }
        }
    }

    fn emit(&mut self) {
        let byte = self.current as u8;
        self.bytes.push(byte);
        if byte == 0xFF {
            self.bytes.push(0x00);
        }
        self.current = 0;
        self.filled = 0;
    }

    fn finish(mut self) -> Vec<u8> {
        while self.filled != 0 {
            self.current = (self.current << 1) | 1;
            self.filled += 1;
            if self.filled == 8 {
                self.emit();
            }
        }
        self.bytes
    }
}
