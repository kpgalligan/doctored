//! The little-endian input of the Photoshop take
//! (`scripts/photoshop-probe/tiff32_679_take.py`): one of Doctored's own
//! big-endian 32-bit layered TIFFs (File ▸ Export as PSD… at 32 bits)
//! written in byte order IBM PC, as Photoshop writes its own
//! `layered32.tif`, through the core's seam
//! (`doctored_core::layered_tiff_little_endian`):
//!
//! ```text
//! tiff32_little <in.tiff> <out.tiff>
//! ```
//!
//! Prints one JSON line `{"in": <in>, "out": <out>, "bytes": <out's size>}`,
//! or exits non-zero with the seam's error on stderr.
use serde_json::json;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = &arguments[..] else {
        eprintln!("usage: tiff32_little <in.tiff> <out.tiff>");
        std::process::exit(2);
    };
    let bytes = match std::fs::read(input) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("failed to read {input}: {e}");
            std::process::exit(1);
        }
    };
    let little = match doctored_core::layered_tiff_little_endian(&bytes) {
        Ok(little) => little,
        Err(e) => {
            eprintln!("{input}: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = std::fs::write(output, &little) {
        eprintln!("failed to write {output}: {e}");
        std::process::exit(1);
    }
    println!(
        "{}",
        json!({"in": input, "out": output, "bytes": little.len()})
    );
}
