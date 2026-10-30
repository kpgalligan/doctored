//! Bounded spatial/color sample index for experimental alpha matting.
//!
//! A median-split 2D tree supplies exact nearest boundary samples. A separate
//! farthest-color palette adds at most eight globally distributed colors, so
//! local sampling is not the only source of evidence. This is deliberately not
//! a second PatchMatch implementation: no correspondence propagation/random
//! search is performed. The inpainter's PatchMatch remains its sole master.

use crate::job_control::JobContext;

pub(crate) const LOCAL_SAMPLES: usize = 8;
const GLOBAL_SAMPLES: usize = 8;
pub(crate) const CANDIDATES: usize = LOCAL_SAMPLES + GLOBAL_SAMPLES;

#[derive(Clone, Copy, Default)]
pub(crate) struct Sample {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) rgb: [f32; 3],
}

impl Sample {
    pub(crate) fn distance_squared(self, x: u32, y: u32) -> f32 {
        let dx = f64::from(self.x) - f64::from(x);
        let dy = f64::from(self.y) - f64::from(y);
        (dx * dx + dy * dy) as f32
    }
}

pub(crate) struct SampleIndex {
    points: Vec<Sample>,
    palette: Vec<Sample>,
}

#[derive(Clone, Copy)]
pub(crate) struct Nearby {
    pub(crate) sample: Sample,
    pub(crate) distance: f32,
}

impl Default for Nearby {
    fn default() -> Self {
        Self {
            sample: Sample::default(),
            distance: f32::INFINITY,
        }
    }
}

impl SampleIndex {
    pub(crate) fn new(mut points: Vec<Sample>, job: JobContext<'_>) -> Result<Self, String> {
        // Eight farthest-point selections are a fixed cost, not a sample-count
        // dependent k-means convergence loop. Stable row order breaks ties.
        let mut palette = Vec::with_capacity(GLOBAL_SAMPLES);
        if let Some(first) = points.first().copied() {
            palette.push(first);
        }
        for _ in 1..GLOBAL_SAMPLES {
            let mut farthest = None;
            let mut greatest = 0.0_f32;
            for (i, point) in points.iter().enumerate() {
                job.every(i)?;
                let nearest = palette
                    .iter()
                    .map(|p| color_distance_squared(point.rgb, p.rgb))
                    .fold(f32::INFINITY, f32::min);
                if nearest > greatest {
                    greatest = nearest;
                    farthest = Some(*point);
                }
            }
            match farthest {
                Some(point) => palette.push(point),
                None => break,
            }
        }
        build_tree(&mut points, 0, job)?;
        Ok(Self { points, palette })
    }

    pub(crate) fn nearby(
        &self,
        x: u32,
        y: u32,
        output: &mut [Nearby; CANDIDATES],
        job: JobContext<'_>,
    ) -> Result<usize, String> {
        let mut remaining = u64::MAX;
        self.nearby_bounded(x, y, output, job, &mut remaining)
    }

    /// Research callers can bound cumulative spatial-tree visits separately
    /// from Gaussian objectives. The existing candidate remains unrestricted.
    pub(crate) fn nearby_bounded(
        &self,
        x: u32,
        y: u32,
        output: &mut [Nearby; CANDIDATES],
        job: JobContext<'_>,
        remaining_visits: &mut u64,
    ) -> Result<usize, String> {
        output.fill(Nearby::default());
        let mut count = 0;
        let mut visited = 0;
        query_tree(
            &self.points,
            0,
            (x, y),
            output,
            &mut count,
            job,
            &mut visited,
            *remaining_visits,
        )?;
        *remaining_visits -= visited as u64;
        for item in &mut output[..count] {
            item.distance = item.distance.sqrt();
        }
        for sample in &self.palette {
            if output[..count]
                .iter()
                .any(|p| p.sample.x == sample.x && p.sample.y == sample.y)
            {
                continue;
            }
            output[count] = Nearby {
                sample: *sample,
                distance: sample.distance_squared(x, y).sqrt(),
            };
            count += 1;
        }
        Ok(count)
    }
}

pub(crate) fn color_distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum()
}

fn build_tree(points: &mut [Sample], depth: usize, job: JobContext<'_>) -> Result<(), String> {
    if points.is_empty() {
        return Ok(());
    }
    job.check()?;
    let middle = points.len() / 2;
    points.select_nth_unstable_by_key(middle, |point| {
        if depth & 1 == 0 {
            (point.x, point.y)
        } else {
            (point.y, point.x)
        }
    });
    let (left, rest) = points.split_at_mut(middle);
    build_tree(left, depth + 1, job)?;
    build_tree(&mut rest[1..], depth + 1, job)
}

// Recursion carries a separately declared work bound alongside its traversal count.
#[allow(clippy::too_many_arguments)]
fn query_tree(
    points: &[Sample],
    depth: usize,
    location: (u32, u32),
    nearest: &mut [Nearby; CANDIDATES],
    count: &mut usize,
    job: JobContext<'_>,
    visited: &mut usize,
    visit_limit: u64,
) -> Result<(), String> {
    if points.is_empty() {
        return Ok(());
    }
    // Collinear boundaries can explore both branches repeatedly; the later
    // color-pair cap does not bound this search's node visits.
    job.every(*visited)?;
    if *visited as u64 >= visit_limit {
        return Err(
            "matting spatial sample search exceeds its declared node-visit budget; narrow the unknown band or refine a smaller area"
                .into(),
        );
    }
    *visited += 1;
    let (x, y) = location;
    let middle = points.len() / 2;
    let point = points[middle];
    let distance = point.distance_squared(x, y);
    let position = nearest[..*count].partition_point(|other| other.distance <= distance);
    if position < LOCAL_SAMPLES {
        let new_count = (*count + 1).min(LOCAL_SAMPLES);
        nearest.copy_within(position..new_count - 1, position + 1);
        nearest[position] = Nearby {
            sample: point,
            distance,
        };
        *count = new_count;
    }
    let delta = if depth & 1 == 0 {
        f64::from(x) - f64::from(point.x)
    } else {
        f64::from(y) - f64::from(point.y)
    };
    let (near, far) = if delta < 0.0 {
        (&points[..middle], &points[middle + 1..])
    } else {
        (&points[middle + 1..], &points[..middle])
    };
    query_tree(
        near,
        depth + 1,
        location,
        nearest,
        count,
        job,
        visited,
        visit_limit,
    )?;
    if *count < LOCAL_SAMPLES || delta * delta <= f64::from(nearest[*count - 1].distance) {
        query_tree(
            far,
            depth + 1,
            location,
            nearest,
            count,
            job,
            visited,
            visit_limit,
        )?;
    }
    Ok(())
}
