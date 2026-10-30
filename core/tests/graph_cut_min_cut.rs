//! `graph_cut::min_cut` (Auto-Blend Layers ▸ Panorama's seams) against
//! a brute-force oracle written here: random grids with at most 12 free
//! pixels, every labelling of the free pixels enumerated and costed with the
//! module's own definition (unary on the side a free pixel takes, every link
//! whose ends differ). The cut's cost must equal the least one and every
//! fixed pixel must keep its side.

use doctored_core::graph_cut::{min_cut, CutGrid};

/// A small deterministic generator (xorshift), so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn cost(grid: &CutGrid, sides: &[bool]) -> u64 {
    let (w, h) = (grid.width, grid.height);
    let mut total = 0u64;
    for i in 0..w * h {
        if grid.fixed[i].is_none() {
            total += u64::from(grid.unary[i][if sides[i] { 0 } else { 1 }]);
        }
        if i % w + 1 < w && sides[i] != sides[i + 1] {
            total += u64::from(grid.right[i]);
        }
        if i / w + 1 < h && sides[i] != sides[i + w] {
            total += u64::from(grid.down[i]);
        }
    }
    total
}

fn least(grid: &CutGrid) -> u64 {
    let free: Vec<usize> = (0..grid.fixed.len())
        .filter(|&i| grid.fixed[i].is_none())
        .collect();
    let base: Vec<bool> = grid.fixed.iter().map(|f| f.unwrap_or(false)).collect();
    let mut best = u64::MAX;
    for bits in 0u32..(1 << free.len()) {
        let mut sides = base.clone();
        for (k, &i) in free.iter().enumerate() {
            sides[i] = bits & (1 << k) != 0;
        }
        best = best.min(cost(grid, &sides));
    }
    best
}

fn random_grid(rng: &mut Rng) -> CutGrid {
    let w = 1 + rng.below(5) as usize;
    let h = 1 + rng.below(5) as usize;
    let n = w * h;
    let mut fixed = Vec::with_capacity(n);
    let mut free = 0;
    for _ in 0..n {
        let roll = rng.below(10);
        let side = if roll < 2 {
            Some(false)
        } else if roll < 4 {
            Some(true)
        } else if free < 12 {
            free += 1;
            None
        } else {
            Some(rng.below(2) == 1)
        };
        fixed.push(side);
    }
    let range = [3u64, 20, 1000][rng.below(3) as usize];
    let unary = (0..n)
        .map(|_| [rng.below(range) as u32, rng.below(range) as u32])
        .collect();
    let right = (0..n).map(|_| rng.below(range) as u32).collect();
    let down = (0..n).map(|_| rng.below(range) as u32).collect();
    CutGrid {
        width: w,
        height: h,
        fixed,
        unary,
        right,
        down,
    }
}

#[test]
fn the_cut_costs_the_brute_force_minimum_and_keeps_fixed_pixels() {
    let mut rng = Rng(0x5eed_564a);
    for case in 0..600 {
        let grid = random_grid(&mut rng);
        let sides = min_cut(&grid);
        assert_eq!(sides.len(), grid.width * grid.height, "case {case}");
        for (i, fixed) in grid.fixed.iter().enumerate() {
            if let Some(side) = fixed {
                assert_eq!(sides[i], *side, "case {case}: fixed pixel {i}");
            }
        }
        assert_eq!(cost(&grid, &sides), least(&grid), "case {case}: {grid:?}");
    }
}

#[test]
fn a_free_pixel_with_no_cost_and_no_link_is_sink() {
    let grid = CutGrid {
        width: 3,
        height: 1,
        fixed: vec![Some(false), None, Some(true)],
        unary: vec![[0, 0]; 3],
        right: vec![0; 3],
        down: vec![0; 3],
    };
    assert_eq!(min_cut(&grid), vec![false, true, true]);
}

#[test]
fn a_cheap_column_splits_a_row_between_the_terminals() {
    // Source at the left, sink at the right, links of 9 but one of 1: the
    // cut crosses the cheap link.
    let grid = CutGrid {
        width: 6,
        height: 1,
        fixed: vec![Some(false), None, None, None, None, Some(true)],
        unary: vec![[0, 0]; 6],
        right: vec![9, 9, 1, 9, 9, 0],
        down: vec![0; 6],
    };
    assert_eq!(min_cut(&grid), vec![false, false, false, true, true, true]);
}

#[test]
fn planes_of_the_wrong_length_answer_nothing() {
    let grid = CutGrid {
        width: 2,
        height: 2,
        fixed: vec![None; 3],
        unary: vec![[0, 0]; 4],
        right: vec![0; 4],
        down: vec![0; 4],
    };
    assert!(min_cut(&grid).is_empty());
}
