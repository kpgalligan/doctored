//! Explicit source permission for Content-Aware Fill, independent of the hole
//! and boundary data. Ordered point-center geometry never rewrites sample RGB.
//! Every segment box is admitted before allocation, including repeated visits.
//! Capsules reuse trimap_strokes' single round-cap geometry implementation.
//!
//! The versioned bounded recipe uses ordered include/exclude rectangles and shared round capsules.
//! Permission AND-reduces through the pyramid independently of real context data, so excluded RGB
//! never enters a source vote.
use crate::job_control::JobContext;
use crate::trimap_strokes::Segment;
use serde::Deserialize;

const MAX_OPERATIONS: usize = 512;
const MAX_POINTS: usize = 4096;
const MAX_VISITS: u64 = 256_000_000;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Initial {
    All,
    None,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    IncludeRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    ExcludeRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    IncludeBrush {
        radius: f64,
        points: Vec<[f64; 2]>,
    },
    ExcludeBrush {
        radius: f64,
        points: Vec<[f64; 2]>,
    },
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMask {
    version: u32,
    initial: Initial,
    operations: Vec<Operation>,
}
struct Rect {
    bounds: [usize; 4],
    include: bool,
}
enum Shape {
    Rect(Rect),
    Brush(Segment),
}
impl SourceMask {
    fn plan(&self, width: u32, height: u32) -> Result<Vec<Shape>, String> {
        if self.version != 1 || self.operations.len() > MAX_OPERATIONS {
            return Err("source_mask requires version 1 and at most 512 operations".into());
        }
        let mut points_total = 0;
        let mut visits = 0u64;
        let mut plan = Vec::new();
        let coordinate = |v: f64| v.is_finite() && v.abs() <= 1_000_000.;
        let mut push = |shape: Shape| -> Result<(), String> {
            let n = match &shape {
                Shape::Rect(r) => {
                    ((r.bounds[2] - r.bounds[0]) * (r.bounds[3] - r.bounds[1])) as u64
                }
                Shape::Brush(s) => s.visits(),
            };
            visits = visits.saturating_add(n);
            if visits > MAX_VISITS {
                return Err("source_mask exceeds 256 million geometry pixel visits; shorten strokes or use fewer operations".into());
            }
            plan.push(shape);
            Ok(())
        };
        for op in &self.operations {
            match op {
                Operation::IncludeRect {
                    x,
                    y,
                    width: w,
                    height: h,
                }
                | Operation::ExcludeRect {
                    x,
                    y,
                    width: w,
                    height: h,
                } => {
                    if ![*x, *y, *w, *h].into_iter().all(coordinate) || *w <= 0. || *h <= 0. {
                        return Err("source_mask rectangles require finite coordinates within ±1000000 and positive dimensions".into());
                    }
                    // Pixel centers in the half-open rectangle [x,x+w) × [y,y+h).
                    let edge =
                        |v: f64, limit: u32| (v - 0.5).ceil().clamp(0., f64::from(limit)) as usize;
                    push(Shape::Rect(Rect {
                        bounds: [
                            edge(*x, width),
                            edge(*y, height),
                            edge(x + w, width),
                            edge(y + h, height),
                        ],
                        include: matches!(op, Operation::IncludeRect { .. }),
                    }))?;
                }
                Operation::IncludeBrush { radius, points }
                | Operation::ExcludeBrush { radius, points } => {
                    points_total += points.len();
                    if points.is_empty()
                        || points_total > MAX_POINTS
                        || !radius.is_finite()
                        || !(0.5..=4096.).contains(radius)
                        || !points.iter().flatten().copied().all(coordinate)
                    {
                        return Err("source_mask brushes require radius 0.5–4096, nonempty finite points within ±1000000 and at most 4096 total points".into());
                    }
                    for i in 0..points.len().saturating_sub(1).max(1) {
                        push(Shape::Brush(Segment::new(
                            points[i],
                            points[(i + 1).min(points.len() - 1)],
                            *radius,
                            u8::from(matches!(op, Operation::IncludeBrush { .. })),
                            width,
                            height,
                        )))?;
                    }
                }
            }
        }
        Ok(plan)
    }
    pub(crate) fn validate(&self, width: u32, height: u32) -> Result<(), String> {
        self.plan(width, height).map(|_| ())
    }
    /// Caller has admitted canvas storage plus bounded parser/geometry overhead.
    pub(crate) fn compile(
        &self,
        width: u32,
        height: u32,
        job: JobContext<'_>,
    ) -> Result<Vec<bool>, String> {
        job.check()?;
        let plan = self.plan(width, height)?;
        let n = crate::precision_image::pixel_count(width, height)?;
        let mut output = Vec::with_capacity(n);
        while output.len() < n {
            job.check()?;
            output.resize(
                (output.len() + 65536).min(n),
                matches!(self.initial, Initial::All),
            );
        }
        for shape in plan {
            let (bounds, include) = match &shape {
                Shape::Rect(r) => (r.bounds, r.include),
                Shape::Brush(s) => (s.bounds, s.label != 0),
            };
            for y in bounds[1]..bounds[3] {
                job.check()?;
                for x in bounds[0]..bounds[2] {
                    job.every(x - bounds[0])?;
                    if match &shape {
                        Shape::Rect(_) => true,
                        Shape::Brush(s) => s.contains(x as f64 + 0.5, y as f64 + 0.5),
                    } {
                        output[y * width as usize + x] = include;
                    }
                }
            }
        }
        Ok(output)
    }
}
