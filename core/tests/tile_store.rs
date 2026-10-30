//! Phase 13B-1: the shared immutable tile store (`tile_store.rs`), black-box.
//! Oracles are hand-derived: tile counts `⌈w/256⌉·⌈h/256⌉`, clipped edge
//! tiles, contiguous planes built independently and compared sample for
//! sample (f32 always through `to_bits`, the `regional_precision.rs` rule),
//! and sharing/accounting figures computed from the rect geometry. The
//! cancellation tests are load-independent: a pre-cancelled control, a worker
//! PARKED at a chosen checkpoint (`common::probe::cancelled_parked_at`) and
//! the worker's own report log; the one wall-clock verdict is `#[ignore]`d
//! and runs under `make cancel-bench`.

use crate::common;
use crate::phase4_corpus;

use std::time::Instant;

use common::probe::{
    cancelled_parked_at, checkpoints_line, parked_line, past, probe, reports, verdict, Trigger,
    RETURN_FLOOR_S,
};
use doctored_core::ffi_jobs::*;
use doctored_core::job_control::DrJobControl;
use doctored_core::tile_store::{accounting, Sample, TilePlane, TILE_EDGE};

const TILE_RGBA8_BYTES: u64 = 256 * 256 * 4;

fn corpus(width: u32, height: u32) -> Vec<[u8; 4]> {
    phase4_corpus::image(width, height)
        .into_raw()
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn gray8(w: u32, h: u32) -> Vec<u8> {
    (0..w * h).map(|i| ((i * 7 + i / w) % 251) as u8).collect()
}

fn gray16(w: u32, h: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| (i.wrapping_mul(2654435761) >> 16) as u16)
        .collect()
}

fn rgba16(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                (x * 257) as u16,
                (y * 131) as u16,
                ((x ^ y) * 29) as u16,
                65535 - (i % 65536) as u16,
            ]
        })
        .collect()
}

fn rgba32(w: u32, h: u32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                x as f32 / 7.0 - 3.0,
                -(y as f32) * 0.125,
                (i as f32).sin() * 40.0,
                (x % 11) as f32 / 10.0,
            ]
        })
        .collect()
}

fn bits32(samples: &[[f32; 4]]) -> Vec<[u32; 4]> {
    samples.iter().map(|p| p.map(f32::to_bits)).collect()
}

/// The oracle for a rect write: the contiguous source with the rect copied in.
fn overwritten<S: Copy>(
    source: &[S],
    stride: u32,
    (x, y, w, h): (u32, u32, u32, u32),
    rect: &[S],
) -> Vec<S> {
    let mut out = source.to_vec();
    for r in 0..h {
        for c in 0..w {
            out[((y + r) * stride + x + c) as usize] = rect[(r * w + c) as usize];
        }
    }
    out
}

/// The oracle for a rect read: slicing the contiguous source.
fn sliced<S: Copy>(source: &[S], stride: u32, (x, y, w, h): (u32, u32, u32, u32)) -> Vec<S> {
    let mut out = Vec::new();
    for r in 0..h {
        let at = ((y + r) * stride + x) as usize;
        out.extend_from_slice(&source[at..at + w as usize]);
    }
    out
}

fn control(token: usize) -> &'static DrJobControl {
    unsafe { &*(token as *const DrJobControl) }
}

fn outcome<T>(result: Result<T, String>) -> (bool, String) {
    match result {
        Ok(_) => (true, String::new()),
        Err(e) => (false, e),
    }
}

// ------------------------------------------------------------ geometry --

#[test]
fn geometry_counts_ceil_and_clips_the_last_tile() {
    for (w, h) in [
        (1, 1),
        (255, 255),
        (256, 256),
        (257, 1),
        (1, 257),
        (300, 300),
        (1024, 768),
    ] {
        let plane = TilePlane::filled(w, h, 7_u8).unwrap();
        let (columns, rows) = (w.div_ceil(256), h.div_ceil(256));
        assert_eq!((plane.columns(), plane.rows()), (columns, rows), "{w}×{h}");
        assert_eq!(plane.dimensions(), (w, h));
        let mut area = 0_u64;
        for ty in 0..rows {
            for tx in 0..columns {
                let (x, y, tw, th) = plane.tile_rect(tx, ty).unwrap();
                assert_eq!((x, y), (tx * 256, ty * 256));
                let expect_w = if tx + 1 == columns { w - tx * 256 } else { 256 };
                let expect_h = if ty + 1 == rows { h - ty * 256 } else { 256 };
                assert_eq!((tw, th), (expect_w, expect_h), "{w}×{h} tile ({tx},{ty})");
                let tile = plane.tile(tx, ty).unwrap();
                assert_eq!((tile.width(), tile.height()), (tw, th));
                assert_eq!(tile.samples().len() as u32, tw * th);
                area += u64::from(tw * th);
            }
        }
        assert_eq!(area, u64::from(w) * u64::from(h), "{w}×{h}");
        assert!(plane.tile(columns, 0).is_none() && plane.tile(0, rows).is_none());
        assert!(plane.tile_rect(columns, 0).is_none() && plane.tile_rect(0, rows).is_none());
    }
    assert_eq!(TILE_EDGE, 256);
}

#[test]
fn dimensions_and_counts_are_refused_by_name() {
    const LIMIT: &str = "tile plane dimensions exceed the supported pixel limit";
    const COUNT: &str = "tile plane sample count does not match dimensions";
    assert_eq!(TilePlane::filled(0, 4, 0_u8).err().unwrap(), LIMIT);
    assert_eq!(TilePlane::filled(4, 0, 0_u8).err().unwrap(), LIMIT);
    assert_eq!(TilePlane::filled(10001, 10000, 0_u8).err().unwrap(), LIMIT);
    assert_eq!(
        TilePlane::<u8>::from_contiguous(10001, 10000, &[], None)
            .err()
            .unwrap(),
        LIMIT
    );
    assert_eq!(
        TilePlane::from_contiguous(3, 3, &[0_u8; 8], None)
            .err()
            .unwrap(),
        COUNT
    );
    let plane = TilePlane::filled(3, 3, 0_u8).unwrap();
    assert_eq!(plane.rebuilt_from(&[0_u8; 10], None).err().unwrap(), COUNT);
    assert_eq!(
        plane.with_rect(0, 0, 2, 2, &[1_u8; 3], None).err().unwrap(),
        COUNT
    );
}

// --------------------------------------------------------- round trips --

fn round_trip<S: Sample, K: PartialEq + std::fmt::Debug>(
    w: u32,
    h: u32,
    source: &[S],
    key: impl Fn(&[S]) -> Vec<K>,
) {
    let plane = TilePlane::from_contiguous(w, h, source, None).unwrap();
    let back = plane.to_contiguous(None).unwrap();
    assert_eq!(key(&back), key(source), "{w}×{h}");
    // Every tile holds exactly its window of the source.
    for ty in 0..plane.rows() {
        for tx in 0..plane.columns() {
            let rect = plane.tile_rect(tx, ty).unwrap();
            assert_eq!(
                key(plane.tile(tx, ty).unwrap().samples()),
                key(&sliced(source, w, rect))
            );
        }
    }
}

#[test]
fn round_trips_are_exact_at_all_five_sample_types() {
    for (w, h) in [(1, 1), (257, 1), (300, 300), (517, 389)] {
        round_trip(w, h, &corpus(w, h), <[_]>::to_vec);
        round_trip(w, h, &gray8(w, h), <[_]>::to_vec);
        round_trip(w, h, &gray16(w, h), <[_]>::to_vec);
        round_trip(w, h, &rgba16(w, h), <[_]>::to_vec);
        round_trip(w, h, &rgba32(w, h), bits32);
    }
    let (w, h) = phase4_corpus::DIMENSIONS[0];
    round_trip(w, h, &corpus(w, h), <[_]>::to_vec);
}

#[test]
fn read_rect_equals_slicing_the_source() {
    let (w, h) = (1024, 768);
    let source = corpus(w, h);
    let plane = TilePlane::from_contiguous(w, h, &source, None).unwrap();
    for rect in [
        (0, 0, 1024, 768),
        (1000, 700, 24, 68),
        (200, 200, 100, 100),
        (255, 255, 2, 2),
        (0, 767, 1024, 1),
        (1023, 0, 1, 768),
        (5, 9, 1, 1),
    ] {
        let (x, y, rw, rh) = rect;
        assert_eq!(
            plane.read_rect(x, y, rw, rh).unwrap(),
            sliced(&source, w, rect),
            "{rect:?}"
        );
    }
    assert_eq!(plane.read_rect(10, 10, 0, 5), Some(vec![]));
    assert_eq!(plane.read_rect(10, 10, 5, 0), Some(vec![]));
    assert!(plane.read_rect(1000, 0, 25, 1).is_none());
    assert!(plane.read_rect(0, 760, 1, 9).is_none());
    assert!(plane.read_rect(u32::MAX, 0, 2, 1).is_none());
}

// ------------------------------------------------------------- sharing --

#[test]
fn with_rect_allocates_only_the_intersecting_tiles() {
    let (w, h) = (1024, 768);
    let source = corpus(w, h);
    let a = TilePlane::from_contiguous(w, h, &source, None).unwrap();
    assert_eq!(a.columns() * a.rows(), 12);
    let rect = vec![[9_u8, 8, 7, 6]; 300 * 300];
    let b = a
        .with_rect(100, 100, 300, 300, &rect, None)
        .unwrap()
        .expect("a changed plane");
    assert_eq!(a.shared_tile_count(&b), 8);
    let both = accounting(&[&a, &b]);
    assert_eq!(both.unique_tiles, 16);
    assert_eq!(both.referenced_tiles, 24);
    assert_eq!(both.unique_bytes, 16 * TILE_RGBA8_BYTES);
    assert_eq!(both.referenced_bytes, 24 * TILE_RGBA8_BYTES);
    assert_eq!(
        b.to_contiguous(None).unwrap(),
        overwritten(&source, w, (100, 100, 300, 300), &rect)
    );
    // The input is untouched.
    assert_eq!(a.to_contiguous(None).unwrap(), source);
    // Writing back what is already there changes nothing.
    let same = b.read_rect(100, 100, 300, 300).unwrap();
    assert!(b
        .with_rect(100, 100, 300, 300, &same, None)
        .unwrap()
        .is_none());
    assert!(a
        .with_rect(0, 0, 3, 3, &source_rect(&source, w, 3), None)
        .unwrap()
        .is_none());
    // A zero-area rect is no change.
    assert!(a.with_rect(5, 5, 0, 3, &[], None).unwrap().is_none());
}

fn source_rect(source: &[[u8; 4]], stride: u32, n: u32) -> Vec<[u8; 4]> {
    sliced(source, stride, (0, 0, n, n))
}

#[test]
fn with_rect_reaches_the_right_and_bottom_edges() {
    for ((w, h), rect, touched) in [
        ((300, 300), (250, 250, 50, 50), 4),
        ((1024, 768), (900, 700, 124, 68), 1),
        ((1024, 768), (0, 767, 1024, 1), 4),
        ((1000, 600), (700, 300, 300, 300), 4),
    ] {
        let source = corpus(w, h);
        let a = TilePlane::from_contiguous(w, h, &source, None).unwrap();
        let (x, y, rw, rh) = rect;
        let data = vec![[1_u8, 2, 3, 4]; (rw * rh) as usize];
        let b = a.with_rect(x, y, rw, rh, &data, None).unwrap().unwrap();
        let tiles = (a.columns() * a.rows()) as usize;
        assert_eq!(a.shared_tile_count(&b), tiles - touched, "{rect:?}");
        assert_eq!(
            b.to_contiguous(None).unwrap(),
            overwritten(&source, w, rect, &data),
            "{rect:?}"
        );
    }
}

#[test]
fn a_rect_leaving_the_plane_is_refused() {
    let a = TilePlane::filled(300, 200, 0_u16).unwrap();
    for (x, y, w, h) in [
        (250, 0, 51, 1),
        (0, 150, 1, 51),
        (300, 0, 1, 1),
        (u32::MAX, 0, 2, 1),
    ] {
        let data = vec![1_u16; (w * h) as usize];
        assert_eq!(
            a.with_rect(x, y, w, h, &data, None).err().unwrap(),
            "tile rect leaves the plane",
            "{:?}",
            (x, y, w, h)
        );
    }
}

#[test]
fn rebuilt_from_shares_every_unchanged_tile() {
    let (w, h) = (1024, 768);
    let source = rgba16(w, h);
    let a = TilePlane::from_contiguous(w, h, &source, None).unwrap();
    assert!(a.rebuilt_from(&source, None).unwrap().is_none());
    let mut edited = source.clone();
    edited[(600 * w + 700) as usize][2] ^= 1;
    let b = a.rebuilt_from(&edited, None).unwrap().unwrap();
    assert_eq!(a.shared_tile_count(&b), 11);
    assert!(!std::sync::Arc::ptr_eq(
        a.tile(2, 2).unwrap(),
        b.tile(2, 2).unwrap()
    ));
    assert_eq!(b.to_contiguous(None).unwrap(), edited);
    assert_eq!(accounting(&[&a, &b]).unique_tiles, 13);
}

#[test]
fn the_latch_is_bitwise_for_signed_zero_and_nan() {
    // Signed-zero movement is stored-state movement (`geometry_noops.rs`).
    let (w, h) = (1024, 768);
    let a = TilePlane::filled(w, h, [0.0_f32; 4]).unwrap();
    let rect = vec![[-0.0_f32, 0.0, 0.0, 0.0]; 300 * 300];
    let b = a
        .with_rect(100, 100, 300, 300, &rect, None)
        .unwrap()
        .expect("-0.0 over 0.0 is a change");
    assert_eq!(a.shared_tile_count(&b), 8);
    let source = vec![[0.0_f32; 4]; (w * h) as usize];
    assert_eq!(
        bits32(&b.to_contiguous(None).unwrap()),
        bits32(&overwritten(&source, w, (100, 100, 300, 300), &rect))
    );
    // A NaN pattern written over itself is no movement.
    let mut samples = rgba32(300, 200);
    samples[12345][1] = f32::NAN;
    let p = TilePlane::from_contiguous(300, 200, &samples, None).unwrap();
    assert!(p
        .rebuilt_from(&p.to_contiguous(None).unwrap(), None)
        .unwrap()
        .is_none());
    let (x, y) = (12345 % 300, 12345 / 300);
    let same = p.read_rect(x, y, 1, 1).unwrap();
    assert!(same[0][1].is_nan());
    assert!(p.with_rect(x, y, 1, 1, &same, None).unwrap().is_none());
}

#[test]
fn filled_shares_at_most_four_tiles() {
    let (w, h) = phase4_corpus::DIMENSIONS[0];
    let plane = TilePlane::filled(w, h, [1_u8, 2, 3, 4]).unwrap();
    let totals = accounting(&[&plane]);
    assert!(totals.unique_tiles <= 4, "{totals:?}");
    assert!(totals.unique_bytes <= 4 * TILE_RGBA8_BYTES, "{totals:?}");
    assert_eq!(totals.referenced_tiles, 384);
    assert!(plane
        .to_contiguous(None)
        .unwrap()
        .iter()
        .all(|&p| p == [1, 2, 3, 4]));
}

/// The 8-state dab sequence: each 300×300 rect at `(100 + 512·i, 100)` covers
/// tiles `2i, 2i+1` × `0, 1` — exactly 4 — on the 24 MP corpus.
fn dab_states(w: u32, h: u32) -> Vec<TilePlane<[u8; 4]>> {
    let mut states = vec![TilePlane::from_contiguous(w, h, &corpus(w, h), None).unwrap()];
    let dab = vec![[0_u8; 4]; 300 * 300];
    for i in 0..8 {
        let next = states[i]
            .with_rect(100 + 512 * i as u32, 100, 300, 300, &dab, None)
            .unwrap()
            .unwrap();
        states.push(next);
    }
    states
}

#[test]
fn history_of_eight_dabs_retains_only_the_touched_tiles() {
    let (w, h) = phase4_corpus::DIMENSIONS[0];
    let states = dab_states(w, h);
    for pair in states.windows(2) {
        assert_eq!(pair[0].shared_tile_count(&pair[1]), 380);
    }
    let refs: Vec<_> = states.iter().collect();
    let totals = accounting(&refs);
    assert_eq!(totals.unique_tiles, 384 + 32);
    assert_eq!(totals.referenced_tiles, 9 * 384);
    let base = accounting(&[&states[0]]);
    assert_eq!(base.unique_tiles, 384);
    assert_eq!(
        states[0].retained_bytes(),
        base.unique_bytes + base.index_bytes
    );
}

#[test]
fn retained_bytes_is_tiles_plus_index() {
    let plane = TilePlane::from_contiguous(517, 389, &gray16(517, 389), None).unwrap();
    let totals = accounting(&[&plane]);
    assert_eq!(totals.unique_bytes, 517 * 389 * 2);
    assert_eq!(
        plane.retained_bytes(),
        totals.unique_bytes + totals.index_bytes
    );
    // A clone shares the index and every tile.
    let copy = plane.clone();
    assert_eq!(accounting(&[&plane, &copy]).index_bytes, totals.index_bytes);
    assert_eq!(
        accounting(&[&plane, &copy]).unique_bytes,
        totals.unique_bytes
    );
}

#[test]
fn planes_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TilePlane<[f32; 4]>>();
    assert_send_sync::<TilePlane<u8>>();
}

// -------------------------------------------------------- cancellation --

const RECT: (u32, u32, u32, u32) = (100, 100, 300, 300);

struct Fixture {
    width: u32,
    height: u32,
    big: Vec<[u8; 4]>,
    big_plane: TilePlane<[u8; 4]>,
    small: TilePlane<[u8; 4]>,
    edited: Vec<[u8; 4]>,
    dab: Vec<[u8; 4]>,
}

fn fixture() -> Fixture {
    let (width, height) = phase4_corpus::DIMENSIONS[0];
    let big = corpus(width, height);
    let big_plane = TilePlane::from_contiguous(width, height, &big, None).unwrap();
    let source = corpus(1024, 768);
    let small = TilePlane::from_contiguous(1024, 768, &source, None).unwrap();
    let mut edited = source;
    edited[500 * 1024 + 500][0] ^= 0x80;
    Fixture {
        width,
        height,
        big,
        big_plane,
        small,
        edited,
        dab: vec![[0_u8; 4]; 300 * 300],
    }
}

/// One boxed public call, run under the control behind its token.
type Render<'a> = Box<dyn FnOnce(usize) -> (bool, String) + Send + 'a>;

/// The four public calls under the control behind `token`.
fn run_from(f: &Fixture) -> impl FnOnce(usize) -> (bool, String) + Send + '_ {
    move |token| {
        outcome(TilePlane::from_contiguous(
            f.width,
            f.height,
            &f.big,
            Some(control(token)),
        ))
    }
}
fn run_to(f: &Fixture) -> impl FnOnce(usize) -> (bool, String) + Send + '_ {
    move |token| outcome(f.big_plane.to_contiguous(Some(control(token))))
}
fn run_with_rect(f: &Fixture) -> impl FnOnce(usize) -> (bool, String) + Send + '_ {
    move |token| {
        let (x, y, w, h) = RECT;
        outcome(f.small.with_rect(x, y, w, h, &f.dab, Some(control(token))))
    }
}
fn run_rebuilt(f: &Fixture) -> impl FnOnce(usize) -> (bool, String) + Send + '_ {
    move |token| outcome(f.small.rebuilt_from(&f.edited, Some(control(token))))
}

#[test]
fn a_precancelled_control_refuses_all_four_calls() {
    let f = fixture();
    let runs: [Render<'_>; 4] = [
        Box::new(run_from(&f)),
        Box::new(run_to(&f)),
        Box::new(run_with_rect(&f)),
        Box::new(run_rebuilt(&f)),
    ];
    for run in runs {
        let token = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(token) };
        let (ok, message) = run(token as usize);
        assert!(!ok);
        assert_eq!(message, "operation cancelled");
        assert_eq!(unsafe { &*token }.checkpoints(), 1);
        unsafe { dr_job_control_free(token) };
    }
}

fn parked(what: &str, visited: u64, render: impl FnOnce(usize) -> (bool, String) + Send) {
    let k = 1 + visited / 2;
    let p = cancelled_parked_at(k, &|_, stage| stage == 1, render);
    eprintln!("13B-1 {what}: {}", parked_line(&p));
}

#[test]
fn from_contiguous_cancels_parked_inside_its_tile_loop() {
    let f = fixture();
    parked("from_contiguous", 384, run_from(&f));
    let again = TilePlane::from_contiguous(f.width, f.height, &f.big, None).unwrap();
    assert_eq!(again.to_contiguous(None).unwrap(), f.big);
}

#[test]
fn to_contiguous_cancels_parked_inside_its_tile_loop() {
    let f = fixture();
    parked("to_contiguous", 384, run_to(&f));
    assert_eq!(f.big_plane.to_contiguous(None).unwrap(), f.big);
}

#[test]
fn with_rect_cancels_parked_inside_its_tile_loop() {
    let f = fixture();
    parked("with_rect", 4, run_with_rect(&f));
    let (x, y, w, h) = RECT;
    let out = f
        .small
        .with_rect(x, y, w, h, &f.dab, None)
        .unwrap()
        .unwrap();
    let source = f.small.to_contiguous(None).unwrap();
    assert_eq!(
        out.to_contiguous(None).unwrap(),
        overwritten(&source, 1024, RECT, &f.dab)
    );
}

#[test]
fn rebuilt_from_cancels_parked_inside_its_tile_loop() {
    let f = fixture();
    parked("rebuilt_from", 12, run_rebuilt(&f));
    let out = f.small.rebuilt_from(&f.edited, None).unwrap().unwrap();
    assert_eq!(out.to_contiguous(None).unwrap(), f.edited);
    assert_eq!(f.small.shared_tile_count(&out), 11);
}

fn counted(what: &str, visited: u64, render: impl FnOnce(usize) -> (bool, String) + Send) {
    let log = reports(render);
    let (first, last) = (log.first().unwrap(), log.last().unwrap());
    assert_eq!((first.checkpoints, first.stage), (1, 1), "{what}");
    assert_eq!(last.checkpoints, visited + 2, "{what}");
    assert_eq!(last.stage, 5, "{what}");
    assert_eq!(last.progress, 1.0, "{what}");
}

#[test]
fn an_uncancelled_run_polls_once_per_visited_tile() {
    let f = fixture();
    counted("from_contiguous", 384, run_from(&f));
    counted("to_contiguous", 384, run_to(&f));
    counted("with_rect", 4, run_with_rect(&f));
    counted("rebuilt_from", 12, run_rebuilt(&f));
}

// ---------------------------------------------------------------- bench --

fn uptime() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| format!("uptime unavailable: {e}"))
}

/// The fastest of three uncancelled runs, per tile.
fn per_tile(tiles: f64, run: &dyn Fn()) -> f64 {
    (0..3)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed().as_secs_f64()
        })
        .fold(f64::INFINITY, f64::min)
        / tiles
}

#[test]
#[ignore = "make cancel-bench: cancel-to-return of tile_store conversions"]
fn a_concurrently_cancelled_conversion_returns_within_one_tile() {
    let f = fixture();
    let c_to = per_tile(384.0, &|| {
        f.big_plane.to_contiguous(None).unwrap();
    });
    let c_from = per_tile(384.0, &|| {
        TilePlane::from_contiguous(f.width, f.height, &f.big, None).unwrap();
    });
    for (what, c_tile, render) in [
        ("to_contiguous", c_to, Box::new(run_to(&f)) as Render<'_>),
        ("from_contiguous", c_from, Box::new(run_from(&f))),
    ] {
        let p = probe(
            &|_, _, c| c >= 2,
            Trigger::After(50),
            &|_, stage| stage == 1,
            render,
        );
        let bound = RETURN_FLOOR_S + c_tile;
        eprintln!(
            "13B-1 {what} ({}×{} RGBA8, 384 tiles) cancel inside the tile loop: {:.3} ms \
             cancel-to-return (issued at {:.1} ms; {}); c_tile {:.3} µs; bound T_f + c_tile = \
             {:.3} ms (T_f 1 ms); ≈{:.2} tiles past the floor; {}; load: {}",
            f.width,
            f.height,
            p.latency.as_secs_f64() * 1e3,
            p.at.as_secs_f64() * 1e3,
            checkpoints_line(&p),
            c_tile * 1e6,
            bound * 1e3,
            past(p.latency, c_tile),
            verdict(p.latency, bound),
            uptime(),
        );
    }
}
