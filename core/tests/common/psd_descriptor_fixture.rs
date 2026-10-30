//! The hand-encoded Photoshop descriptor builder the PSD descriptor-body
//! tests share: `psd_adjustment_descriptors.rs` (the adjustment bodies) and
//! `psd_layer_effects.rs` (the `lfx2` layer-effects body). Every expected
//! body in those files is built here, never with the crate's own writer.
#![allow(dead_code)]

/// A unicode string: `u32 count` (chars + 1) then UTF-16BE with a trailing 0.
pub fn uni(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut out = ((units.len() + 1) as u32).to_be_bytes().to_vec();
    for unit in units {
        out.extend(unit.to_be_bytes());
    }
    out.extend([0, 0]);
    out
}

/// An id: a four-byte id is `len = 0` + the bytes, any other its length.
pub fn id(bytes: &[u8]) -> Vec<u8> {
    let len = if bytes.len() == 4 {
        0
    } else {
        bytes.len() as u32
    };
    let mut out = len.to_be_bytes().to_vec();
    out.extend(bytes);
    out
}

/// A descriptor structure: name "", class, count, then the pre-encoded items.
pub fn desc(class: &[u8], items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = uni("");
    out.extend(id(class));
    out.extend((items.len() as u32).to_be_bytes());
    for item in items {
        out.extend(item);
    }
    out
}

/// A descriptor BODY: `u32 version` then the structure.
pub fn body(version: u32, class: &[u8], items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = version.to_be_bytes().to_vec();
    out.extend(desc(class, items));
    out
}

pub fn item(key: &[u8], os_type: &[u8; 4], value: &[u8]) -> Vec<u8> {
    let mut out = id(key);
    out.extend(os_type);
    out.extend(value);
    out
}

pub fn long(key: &[u8], value: i32) -> Vec<u8> {
    item(key, b"long", &value.to_be_bytes())
}

pub fn doub(key: &[u8], value: f64) -> Vec<u8> {
    item(key, b"doub", &value.to_be_bytes())
}

pub fn boolean(key: &[u8], value: u8) -> Vec<u8> {
    item(key, b"bool", &[value])
}

pub fn text(key: &[u8], value: &str) -> Vec<u8> {
    item(key, b"TEXT", &uni(value))
}

pub fn objc(key: &[u8], class: &[u8], items: &[Vec<u8>]) -> Vec<u8> {
    item(key, b"Objc", &desc(class, items))
}

pub fn untf(key: &[u8], unit: &[u8; 4], value: f64) -> Vec<u8> {
    let mut bytes = unit.to_vec();
    bytes.extend(value.to_be_bytes());
    item(key, b"UntF", &bytes)
}

pub fn enumeration(key: &[u8], type_id: &[u8], enum_id: &[u8]) -> Vec<u8> {
    let mut bytes = id(type_id);
    bytes.extend(id(enum_id));
    item(key, b"enum", &bytes)
}

pub fn tdta(key: &[u8], bytes: &[u8]) -> Vec<u8> {
    let mut value = (bytes.len() as u32).to_be_bytes().to_vec();
    value.extend(bytes);
    item(key, b"tdta", &value)
}

pub fn long_list(key: &[u8], values: &[i32]) -> Vec<u8> {
    let mut bytes = (values.len() as u32).to_be_bytes().to_vec();
    for value in values {
        bytes.extend(b"long");
        bytes.extend(value.to_be_bytes());
    }
    item(key, b"VlLs", &bytes)
}

/// An `RGBC` colour object under `key`: Photoshop's three 0…255 doubles.
pub fn rgbc(key: &[u8], red: f64, green: f64, blue: f64) -> Vec<u8> {
    objc(
        key,
        b"RGBC",
        &[
            doub(b"Rd  ", red),
            doub(b"Grn ", green),
            doub(b"Bl  ", blue),
        ],
    )
}
