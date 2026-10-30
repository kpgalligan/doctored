//! `dr_heif_primary_color_facts` and `dr_heif_premultiplied_reference`
//! over hand-built ISO-BMFF files. Every fixture is assembled box by
//! box here, so the expected facts and offsets are the ones each file was
//! written to state, never a reading of the parser.
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_heif_container::{
    dr_heif_premultiplied_reference, dr_heif_primary_color_facts,
};
use std::ffi::{c_char, CStr};
use std::ptr;

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(body);
    out
}
fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut inner = vec![version];
    inner.extend(&flags.to_be_bytes()[1..]);
    inner.extend(body);
    bx(kind, &inner)
}
fn id(version: u8, value: u32) -> Vec<u8> {
    if version == 0 {
        (value as u16).to_be_bytes().to_vec()
    } else {
        value.to_be_bytes().to_vec()
    }
}
fn ftyp() -> Vec<u8> {
    bx(b"ftyp", b"avif\0\0\0\0avifmif1miaf")
}
fn pitm(version: u8, item: u32) -> Vec<u8> {
    full(b"pitm", version, 0, &id(version, item))
}
fn nclx(matrix: u16) -> Vec<u8> {
    let mut body = b"nclx".to_vec();
    body.extend(1u16.to_be_bytes());
    body.extend(13u16.to_be_bytes());
    body.extend(matrix.to_be_bytes());
    body.push(0x80);
    bx(b"colr", &body)
}
fn prof() -> Vec<u8> {
    bx(b"colr", b"prof\0\0\0\x10fake-icc")
}
fn ispe() -> Vec<u8> {
    full(b"ispe", 0, 0, &[0, 0, 0, 64, 0, 0, 0, 48])
}
/// `entries` are (item, [(essential, 1-based index)]).
fn ipma(version: u8, flags: u32, entries: &[(u32, &[(bool, u16)])]) -> Vec<u8> {
    let mut body = (entries.len() as u32).to_be_bytes().to_vec();
    for (item, associations) in entries {
        body.extend(id(version, *item));
        body.push(associations.len() as u8);
        for (essential, index) in associations.iter() {
            if flags & 1 != 0 {
                body.extend((index | if *essential { 0x8000 } else { 0 }).to_be_bytes());
            } else {
                body.push(*index as u8 | if *essential { 0x80 } else { 0 });
            }
        }
    }
    full(b"ipma", version, flags, &body)
}
fn iprp(properties: &[Vec<u8>], ipma: &[u8]) -> Vec<u8> {
    let mut body = bx(b"ipco", &properties.concat());
    body.extend(ipma);
    bx(b"iprp", &body)
}
/// `references` are (type, from, [to]).
fn iref(version: u8, references: &[(&[u8; 4], u32, &[u32])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (kind, from, to) in references {
        let mut child = id(version, *from);
        child.extend((to.len() as u16).to_be_bytes());
        for item in to.iter() {
            child.extend(id(version, *item));
        }
        body.extend(bx(kind, &child));
    }
    full(b"iref", version, 0, &body)
}
fn meta(children: &[Vec<u8>]) -> Vec<u8> {
    let mut body = full(b"hdlr", 0, 0, b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0\0");
    body.extend(children.concat());
    full(b"meta", 0, 0, &body)
}
fn file(meta: &[u8]) -> Vec<u8> {
    let mut out = ftyp();
    out.extend(meta);
    out.extend(bx(b"mdat", &[7u8; 32]));
    out
}
/// pitm 1 with properties [ispe, nclx(matrix)] associated (ispe, colr).
fn single(matrix: u16) -> Vec<u8> {
    file(&meta(&[
        pitm(0, 1),
        iprp(
            &[ispe(), nclx(matrix)],
            &ipma(0, 0, &[(1, &[(false, 1), (true, 2)])]),
        ),
    ]))
}

unsafe fn error_string(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    value
}
fn facts(bytes: &[u8]) -> Result<(i32, bool), String> {
    let (mut matrix, mut premultiplied) = (-77i32, true);
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_heif_primary_color_facts(
            bytes.as_ptr(),
            bytes.len(),
            &mut matrix,
            &mut premultiplied,
            &mut error,
        )
    };
    let message = unsafe { error_string(error) };
    if ok {
        assert!(message.is_empty(), "{message}");
        Ok((matrix, premultiplied))
    } else {
        assert!(!message.is_empty(), "refusal lacked a diagnostic");
        // Outputs are untouched on refusal.
        assert_eq!((matrix, premultiplied), (-77, true));
        assert_eq!(message, message.to_lowercase(), "{message}");
        Err(message)
    }
}

#[test]
fn the_primary_items_nclx_matrix_is_reported() {
    assert_eq!(facts(&single(0)), Ok((0, false)));
    assert_eq!(facts(&single(6)), Ok((6, false)));
    assert_eq!(facts(&single(65535)), Ok((65535, false)));
}

#[test]
fn the_first_nclx_among_the_primarys_associations_wins_over_a_prof() {
    // An ICC file carries both `colr prof` and `colr nclx`.
    let both = file(&meta(&[
        pitm(0, 1),
        iprp(
            &[prof(), ispe(), nclx(0), nclx(9)],
            &ipma(0, 0, &[(1, &[(false, 2), (true, 1), (true, 3), (true, 4)])]),
        ),
    ]));
    assert_eq!(facts(&both), Ok((0, false)));
    // Association order decides, not property order.
    let reordered = file(&meta(&[
        pitm(0, 1),
        iprp(
            &[nclx(0), nclx(9)],
            &ipma(0, 0, &[(1, &[(true, 2), (true, 1)])]),
        ),
    ]));
    assert_eq!(facts(&reordered), Ok((9, false)));
    // A prof alone is no nclx.
    let icc_only = file(&meta(&[
        pitm(0, 1),
        iprp(&[prof()], &ipma(0, 0, &[(1, &[(true, 1)])])),
    ]));
    assert_eq!(facts(&icc_only), Ok((-1, false)));
}

#[test]
fn an_nclx_on_another_item_is_not_the_primarys() {
    let other = file(&meta(&[
        pitm(0, 2),
        iprp(
            &[ispe(), nclx(0), nclx(6)],
            &ipma(
                0,
                0,
                &[(1, &[(false, 1), (true, 2)]), (2, &[(false, 1), (true, 3)])],
            ),
        ),
    ]));
    assert_eq!(facts(&other), Ok((6, false)));
    let none = file(&meta(&[
        pitm(0, 3),
        iprp(&[ispe(), nclx(0)], &ipma(0, 0, &[(1, &[(true, 2)])])),
    ]));
    assert_eq!(facts(&none), Ok((-1, false)));
}

#[test]
fn wide_ids_and_fifteen_bit_indices_are_read() {
    let mut properties = vec![ispe(); 200];
    properties.push(nclx(0));
    let wide = file(&meta(&[
        pitm(1, 70000),
        iprp(
            &properties,
            &ipma(1, 1, &[(70000, &[(false, 1), (true, 201)])]),
        ),
    ]));
    assert_eq!(facts(&wide), Ok((0, false)));
    // Several ipma boxes are all read.
    let mut split = ipma(0, 0, &[(2, &[(true, 1)])]);
    split.extend(ipma(0, 0, &[(1, &[(true, 2)])]));
    let two = file(&meta(&[pitm(0, 1), iprp(&[ispe(), nclx(0)], &split)]));
    assert_eq!(facts(&two), Ok((0, false)));
}

#[test]
fn avifencs_premultiplied_layout_is_premultiplied() {
    // `avifenc -l --premultiply`: pitm 1, `prem` from 1 to (2), `auxl` from
    // 2 to (1).
    let real = file(&meta(&[
        pitm(0, 1),
        iprp(
            &[ispe(), nclx(0)],
            &ipma(0, 0, &[(1, &[(false, 1), (true, 2)]), (2, &[(false, 1)])]),
        ),
        iref(0, &[(b"prem", 1, &[2]), (b"auxl", 2, &[1])]),
    ]));
    assert_eq!(facts(&real), Ok((0, true)));
    let wide = file(&meta(&[
        pitm(1, 1),
        iprp(&[nclx(0)], &ipma(1, 0, &[(1, &[(true, 1)])])),
        iref(1, &[(b"auxl", 2, &[1]), (b"prem", 1, &[2])]),
    ]));
    assert_eq!(facts(&wide), Ok((0, true)));
}

#[test]
fn a_prem_that_only_points_at_the_primary_is_not_premultiplied() {
    let reversed = file(&meta(&[
        pitm(0, 1),
        iprp(&[nclx(0)], &ipma(0, 0, &[(1, &[(true, 1)])])),
        iref(0, &[(b"prem", 2, &[1]), (b"auxl", 2, &[1])]),
    ]));
    assert_eq!(facts(&reversed), Ok((0, false)));
    // Straight alpha: only the auxl reference.
    let straight = file(&meta(&[
        pitm(0, 1),
        iprp(&[nclx(0)], &ipma(0, 0, &[(1, &[(true, 1)])])),
        iref(0, &[(b"auxl", 2, &[1])]),
    ]));
    assert_eq!(facts(&straight), Ok((0, false)));
}

#[test]
fn well_formed_files_missing_a_part_answer_none() {
    let mut no_meta = ftyp();
    no_meta.extend(bx(b"mdat", &[1, 2, 3]));
    assert_eq!(facts(&no_meta), Ok((-1, false)));
    assert_eq!(facts(&ftyp()), Ok((-1, false)));
    let no_pitm = file(&meta(&[iprp(
        &[nclx(0)],
        &ipma(0, 0, &[(1, &[(true, 1)])]),
    )]));
    assert_eq!(facts(&no_pitm), Ok((-1, false)));
    let no_iprp = file(&meta(&[pitm(0, 1), iref(0, &[(b"prem", 1, &[2])])]));
    assert_eq!(facts(&no_iprp), Ok((-1, true)));
    let no_ipma = file(&meta(&[pitm(0, 1), bx(b"iprp", &bx(b"ipco", &nclx(0)))]));
    assert_eq!(facts(&no_ipma), Ok((-1, false)));
}

#[test]
fn largesize_and_size_zero_boxes_are_framed() {
    // A largesize `meta`, then a size-0 `mdat` running to the end.
    let body = meta(&[
        pitm(0, 1),
        iprp(&[nclx(0)], &ipma(0, 0, &[(1, &[(true, 1)])])),
    ]);
    let mut large = ftyp();
    large.extend(1u32.to_be_bytes());
    large.extend(b"meta");
    large.extend(((body.len() - 8 + 16) as u64).to_be_bytes());
    large.extend(&body[8..]);
    large.extend(0u32.to_be_bytes());
    large.extend(b"mdat");
    large.extend([9u8; 40]);
    assert_eq!(facts(&large), Ok((0, false)));
    // A size-0 `meta` is the last box.
    let mut zero = ftyp();
    zero.extend(0u32.to_be_bytes());
    zero.extend(&body[4..]);
    assert_eq!(facts(&zero), Ok((0, false)));
}

#[test]
fn malformed_files_refuse() {
    // Empty, and a first box that is not ftyp.
    assert!(facts(&[]).is_err());
    let mut late = bx(b"mdat", &[0; 4]);
    late.extend(single(0));
    assert!(facts(&late).is_err());
    assert!(facts(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR").is_err());
    // A size smaller than its header, and one past its parent.
    let mut small = ftyp();
    small.extend(4u32.to_be_bytes());
    small.extend(b"meta");
    assert!(facts(&small).is_err());
    let mut over = single(0);
    let at = ftyp().len();
    over[at..at + 4].copy_from_slice(&100_000u32.to_be_bytes());
    assert!(facts(&over).is_err());
    let mut tiny_large = ftyp();
    tiny_large.extend(1u32.to_be_bytes());
    tiny_large.extend(b"meta");
    tiny_large.extend(8u64.to_be_bytes());
    assert!(facts(&tiny_large).is_err());
    // A truncated nclx.
    let short_nclx = file(&meta(&[
        pitm(0, 1),
        iprp(
            &[bx(b"colr", b"nclx\0\x01\0")],
            &ipma(0, 0, &[(1, &[(true, 1)])]),
        ),
    ]));
    assert!(facts(&short_nclx).is_err());
    // An association past ipco, for the primary and for another item.
    for item in [1, 5] {
        let past = file(&meta(&[
            pitm(0, 1),
            iprp(&[ispe(), nclx(0)], &ipma(0, 0, &[(item, &[(true, 3)])])),
        ]));
        assert!(facts(&past).is_err(), "item {item}");
    }
    // An ipma whose declared entries run past its body.
    let mut short_ipma = ipma(0, 0, &[(1, &[(true, 1)])]);
    short_ipma[15] = 9;
    let declared = file(&meta(&[pitm(0, 1), iprp(&[nclx(0)], &short_ipma)]));
    assert!(facts(&declared).is_err());
    // An iref child whose to_item_IDs run past it.
    let mut child = 1u16.to_be_bytes().to_vec();
    child.extend(5u16.to_be_bytes());
    child.extend(2u16.to_be_bytes());
    let short_iref = file(&meta(&[
        pitm(0, 1),
        full(b"iref", 0, 0, &bx(b"prem", &child)),
    ]));
    assert!(facts(&short_iref).is_err());
    // A truncated pitm.
    let short_pitm = file(&meta(&[full(b"pitm", 0, 0, &[0])]));
    assert!(facts(&short_pitm).is_err());
}

#[test]
fn every_truncation_and_byte_flip_answers_without_panicking() {
    let real = file(&meta(&[
        pitm(0, 1),
        iprp(
            &[ispe(), prof(), nclx(0)],
            &ipma(0, 1, &[(1, &[(false, 1), (true, 3)]), (2, &[(false, 1)])]),
        ),
        iref(0, &[(b"prem", 1, &[2]), (b"auxl", 2, &[1])]),
    ]));
    assert_eq!(facts(&real), Ok((0, true)));
    for length in 0..real.len() {
        let _ = facts(&real[..length]);
    }
    for at in 0..real.len() {
        for value in [0u8, 1, 0x7f, 0x80, 0xff] {
            let mut changed = real.clone();
            changed[at] = value;
            let _ = facts(&changed);
        }
    }
}

#[test]
fn null_arguments_return_false_with_a_diagnostic() {
    let bytes = single(0);
    let (mut matrix, mut premultiplied) = (-77i32, true);
    for missing in 0..3 {
        let mut error = ptr::null_mut();
        let ok = unsafe {
            dr_heif_primary_color_facts(
                if missing == 0 {
                    ptr::null()
                } else {
                    bytes.as_ptr()
                },
                bytes.len(),
                if missing == 1 {
                    ptr::null_mut()
                } else {
                    &mut matrix
                },
                if missing == 2 {
                    ptr::null_mut()
                } else {
                    &mut premultiplied
                },
                &mut error,
            )
        };
        assert!(!ok);
        assert!(!unsafe { error_string(error) }.is_empty());
        assert_eq!((matrix, premultiplied), (-77, true));
    }
    unsafe {
        assert!(!dr_heif_primary_color_facts(
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut()
        ));
        // A malformed file with a NULL err_out still answers false.
        assert!(!dr_heif_primary_color_facts(
            [0u8; 3].as_ptr(),
            3,
            &mut matrix,
            &mut premultiplied,
            ptr::null_mut()
        ));
        // A good file with a NULL err_out answers.
        assert!(dr_heif_primary_color_facts(
            bytes.as_ptr(),
            bytes.len(),
            &mut matrix,
            &mut premultiplied,
            ptr::null_mut()
        ));
    }
    assert_eq!((matrix, premultiplied), (0, false));
}

// ------------------------------------------ the primary's prem reference --

const UNTOUCHED: (usize, usize) = (usize::MAX - 3, 77);

/// `Ok(None)` when the file has no `prem` child from the primary item.
fn reference(bytes: &[u8]) -> Result<Option<(usize, usize)>, String> {
    let (mut offset, mut width) = UNTOUCHED;
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_heif_premultiplied_reference(
            bytes.as_ptr(),
            bytes.len(),
            &mut offset,
            &mut width,
            &mut error,
        )
    };
    let message = unsafe { error_string(error) };
    if ok {
        assert!(message.is_empty(), "{message}");
        Ok((width != 0).then_some((offset, width)))
    } else {
        assert!(!message.is_empty(), "refusal lacked a diagnostic");
        assert_eq!((offset, width), UNTOUCHED, "outputs untouched on refusal");
        assert_eq!(message, message.to_lowercase(), "{message}");
        Err(message)
    }
}

/// The file with the field's bytes set to 0xFF, which is what the host does.
fn detached(bytes: &[u8], at: (usize, usize)) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[at.0..at.0 + at.1].fill(0xff);
    out
}

#[test]
fn the_prem_reference_of_the_primary_is_located_by_offset_and_width() {
    // The offset is counted from the pieces this file was assembled from:
    // everything before `iref`, its 12-byte FullBox header, the children
    // before the `prem` one, and that child's 8-byte box header.
    let hdlr = full(b"hdlr", 0, 0, b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0\0").len();
    for version in [0u8, 1] {
        let width = if version == 0 { 2 } else { 4 };
        let head = [
            pitm(version, 1),
            iprp(
                &[ispe(), nclx(0)],
                &ipma(
                    version,
                    0,
                    &[(1, &[(false, 1), (true, 2)]), (2, &[(false, 1)])],
                ),
            ),
        ];
        let before = ftyp().len() + 12 + hdlr + head.concat().len() + 12;
        // avifenc's order: `prem` first.
        let first = file(&meta(&[
            head[0].clone(),
            head[1].clone(),
            iref(version, &[(b"prem", 1, &[2]), (b"auxl", 2, &[1])]),
        ]));
        let found = reference(&first).unwrap().expect("a prem from the primary");
        assert_eq!(found, (before + 8, width), "version {version}");
        assert_eq!(&first[found.0 - 4..found.0], b"prem");
        // `prem` second: one whole `auxl` child (header, from, count, one id)
        // comes first.
        let second = file(&meta(&[
            head[0].clone(),
            head[1].clone(),
            iref(version, &[(b"auxl", 2, &[1]), (b"prem", 1, &[2])]),
        ]));
        let found = reference(&second)
            .unwrap()
            .expect("a prem from the primary");
        assert_eq!(found, (before + 8 + width + 2 + width + 8, width));
        assert_eq!(&second[found.0 - 4..found.0], b"prem");
        // Setting the field's bytes to 0xFF detaches the reference: the file
        // still parses, the facts no longer say premultiplied, and there is
        // nothing left to locate.
        for bytes in [first, second] {
            assert_eq!(facts(&bytes), Ok((0, true)));
            let at = reference(&bytes).unwrap().unwrap();
            let patched = detached(&bytes, at);
            assert_eq!(patched.len(), bytes.len());
            assert_eq!(facts(&patched), Ok((0, false)), "version {version}");
            assert_eq!(reference(&patched), Ok(None));
        }
    }
}

#[test]
fn only_the_first_prem_from_the_primary_is_located() {
    let twice = file(&meta(&[
        pitm(0, 1),
        iref(
            0,
            &[
                (b"prem", 3, &[1]),
                (b"prem", 1, &[2]),
                (b"prem", 1, &[4, 5]),
            ],
        ),
    ]));
    let found = reference(&twice).unwrap().unwrap();
    assert_eq!(found.1, 2);
    assert_eq!(&twice[found.0..found.0 + 2], &[0, 1]);
    // The child before it is the `prem` from item 3: header 8, from 2, count
    // 2, one id 2.
    assert_eq!(&twice[found.0 - 18..found.0 - 14], b"prem");
    assert_eq!(&twice[found.0 - 4..found.0], b"prem");
    assert_eq!(&twice[found.0 + 2..found.0 + 6], &[0, 1, 0, 2]);
    // One patch leaves the second reference in place, so the file still says
    // premultiplied — the host refuses that file rather than guess.
    assert_eq!(facts(&detached(&twice, found)), Ok((-1, true)));
}

#[test]
fn files_without_a_prem_from_the_primary_answer_none() {
    assert_eq!(reference(&single(0)), Ok(None));
    assert_eq!(reference(&ftyp()), Ok(None));
    let reversed = file(&meta(&[
        pitm(0, 1),
        iref(0, &[(b"prem", 2, &[1]), (b"auxl", 2, &[1])]),
    ]));
    assert_eq!(reference(&reversed), Ok(None));
    let no_pitm = file(&meta(&[iref(0, &[(b"prem", 1, &[2])])]));
    assert_eq!(reference(&no_pitm), Ok(None));
}

#[test]
fn the_prem_locator_refuses_what_the_facts_reader_refuses() {
    assert!(reference(&[]).is_err());
    assert!(reference(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR").is_err());
    let mut child = 1u16.to_be_bytes().to_vec();
    child.extend(5u16.to_be_bytes());
    child.extend(2u16.to_be_bytes());
    let short_iref = file(&meta(&[
        pitm(0, 1),
        full(b"iref", 0, 0, &bx(b"prem", &child)),
    ]));
    assert!(reference(&short_iref).is_err());
    // A later child that is malformed refuses even after a match.
    let mut body = iref(0, &[(b"prem", 1, &[2])]);
    body.extend(bx(b"auxl", &[0]));
    let size = (body.len() as u32).to_be_bytes();
    body[..4].copy_from_slice(&size);
    assert!(reference(&file(&meta(&[pitm(0, 1), body]))).is_err());

    let real = file(&meta(&[
        pitm(0, 1),
        iprp(
            &[ispe(), prof(), nclx(0)],
            &ipma(0, 1, &[(1, &[(false, 1), (true, 3)]), (2, &[(false, 1)])]),
        ),
        iref(0, &[(b"auxl", 2, &[1]), (b"prem", 1, &[2])]),
    ]));
    assert!(reference(&real).unwrap().is_some());
    for length in 0..real.len() {
        if let Ok(Some((offset, width))) = reference(&real[..length]) {
            assert!(offset + width <= length);
        }
    }
    for at in 0..real.len() {
        for value in [0u8, 1, 0x7f, 0x80, 0xff] {
            let mut changed = real.clone();
            changed[at] = value;
            if let Ok(Some((offset, width))) = reference(&changed) {
                assert!(offset + width <= changed.len());
                assert!(matches!(width, 2 | 4));
            }
        }
    }
}

#[test]
fn the_prem_locator_tolerates_null_arguments() {
    let bytes = single(0);
    let (mut offset, mut width) = UNTOUCHED;
    for missing in 0..3 {
        let mut error = ptr::null_mut();
        let ok = unsafe {
            dr_heif_premultiplied_reference(
                if missing == 0 {
                    ptr::null()
                } else {
                    bytes.as_ptr()
                },
                bytes.len(),
                if missing == 1 {
                    ptr::null_mut()
                } else {
                    &mut offset
                },
                if missing == 2 {
                    ptr::null_mut()
                } else {
                    &mut width
                },
                &mut error,
            )
        };
        assert!(!ok);
        assert!(!unsafe { error_string(error) }.is_empty());
        assert_eq!((offset, width), UNTOUCHED);
    }
    unsafe {
        assert!(!dr_heif_premultiplied_reference(
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut()
        ));
        assert!(!dr_heif_premultiplied_reference(
            [0u8; 3].as_ptr(),
            3,
            &mut offset,
            &mut width,
            ptr::null_mut()
        ));
        assert!(dr_heif_premultiplied_reference(
            bytes.as_ptr(),
            bytes.len(),
            &mut offset,
            &mut width,
            ptr::null_mut()
        ));
    }
    assert_eq!(width, 0, "no prem from the primary");
}
