//! Numeric policy for the single PatchMatch implementation. Byte evaluation
//! retains its shipped rounding and integer SSD; typed evaluation carries native
//! code-value f32 RGB through every pyramid level and vote, with f64 distances.
//! A typed solve never borrows a byte search image or quantizes an intermediate.
//!
//! Byte specialization retains every legacy rounding.

pub(crate) trait Sample: Copy + Default {
    const INTEGER: bool;
    fn number(self) -> f32;
    fn mean(sum: f64, count: u64) -> Self;
    fn filtered(value: f32) -> Self;
    fn squared_distance(a: &[Self], b: &[Self]) -> f64;
}

impl Sample for u8 {
    const INTEGER: bool = true;
    fn number(self) -> f32 {
        f32::from(self)
    }
    fn mean(sum: f64, count: u64) -> Self {
        // Integer division and count/2 are the established byte box/onion rule.
        ((sum as u64 + count / 2) / count).min(255) as u8
    }
    fn filtered(value: f32) -> Self {
        (value + 0.5).floor().clamp(0.0, 255.0) as u8
    }
    #[inline]
    fn squared_distance(a: &[Self], b: &[Self]) -> f64 {
        // Preserve the vectorizable integer row kernel. Converting each byte
        // to f64 made the full legacy fill 46% slower in the parity corpus.
        let mut sum = 0u32;
        for (p, q) in a.iter().zip(b) {
            let d = i32::from(*p) - i32::from(*q);
            sum += (d * d) as u32;
        }
        f64::from(sum)
    }
}

impl Sample for f32 {
    const INTEGER: bool = false;
    fn number(self) -> f32 {
        self
    }
    fn mean(sum: f64, count: u64) -> Self {
        (sum / count as f64) as f32
    }
    fn filtered(value: f32) -> Self {
        value
    }
    #[inline]
    fn squared_distance(a: &[Self], b: &[Self]) -> f64 {
        let mut sum = 0f64;
        for (p, q) in a.iter().zip(b) {
            let d = f64::from(*p) - f64::from(*q);
            sum += d * d;
        }
        sum
    }
}
