extern crate doctored_core;
use doctored_core::doc::DrDocument;
// Test fixture only: create unsupported memory state without allocating large pixels.
#[no_mangle]
pub unsafe extern "C" fn probe_oversized_layers(doc: *mut DrDocument) {
    let doc = unsafe { &mut *doc };
    doc.layers = vec![doc.layers[0].clone(); 1025];
}
