//! Numeric kernels used by the transformer.
//!
//! Loops are written over fixed-size chunks so LLVM auto-vectorises them with
//! whatever SIMD the target enables (SSE2 in the kernel, AVX on a host build
//! with `-C target-cpu=native`). Transcendental functions come from [`libm`]
//! because `core` provides none in `no_std` builds.

/// `e^x`.
pub fn expf(x: f32) -> f32 {
    libm::expf(x)
}
/// Square root.
pub fn sqrtf(x: f32) -> f32 {
    libm::sqrtf(x)
}
/// `base^exp`.
pub fn powf(base: f32, exp: f32) -> f32 {
    libm::powf(base, exp)
}
/// Sine.
pub fn sinf(x: f32) -> f32 {
    libm::sinf(x)
}
/// Cosine.
pub fn cosf(x: f32) -> f32 {
    libm::cosf(x)
}
/// Absolute value.
pub fn fabsf(x: f32) -> f32 {
    libm::fabsf(x)
}
/// Round half away from zero.
pub fn roundf(x: f32) -> f32 {
    libm::roundf(x)
}

/// Lanes processed per iteration by the vectorisable loops.
const LANES: usize = 8;

/// Dot product of two equal-length f32 slices.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let mut acc = [0.0f32; LANES];
    let (ac, ar) = a.split_at(a.len() / LANES * LANES);
    let (bc, br) = b.split_at(ac.len());
    for (x, y) in ac.chunks_exact(LANES).zip(bc.chunks_exact(LANES)) {
        for k in 0..LANES {
            acc[k] += x[k] * y[k];
        }
    }
    let mut sum: f32 = acc.iter().sum();
    for (x, y) in ar.iter().zip(br) {
        sum += x * y;
    }
    sum
}

/// Integer dot product of two int8 slices (one quantisation group).
pub fn dot_i8(a: &[i8], b: &[i8]) -> i32 {
    debug_assert_eq!(a.len(), b.len());
    let mut acc = [0i32; LANES * 2];
    let (ac, ar) = a.split_at(a.len() / (LANES * 2) * (LANES * 2));
    let (bc, br) = b.split_at(ac.len());
    for (x, y) in ac.chunks_exact(LANES * 2).zip(bc.chunks_exact(LANES * 2)) {
        for k in 0..LANES * 2 {
            acc[k] += x[k] as i32 * y[k] as i32;
        }
    }
    let mut sum: i32 = acc.iter().sum();
    for (x, y) in ar.iter().zip(br) {
        sum += *x as i32 * *y as i32;
    }
    sum
}

/// `out += a * x`.
pub fn axpy(out: &mut [f32], a: f32, x: &[f32]) {
    for (o, v) in out.iter_mut().zip(x) {
        *o += a * v;
    }
}

/// `out += x`.
pub fn add_assign(out: &mut [f32], x: &[f32]) {
    for (o, v) in out.iter_mut().zip(x) {
        *o += v;
    }
}

/// Root-mean-square normalisation: `out = x / rms(x) * weight`.
pub fn rmsnorm(out: &mut [f32], x: &[f32], weight: &[f32], eps: f32) {
    let mean_sq = dot(x, x) / x.len() as f32;
    let inv = 1.0 / sqrtf(mean_sq + eps);
    for ((o, v), w) in out.iter_mut().zip(x).zip(weight) {
        *o = v * inv * w;
    }
}

/// [`rmsnorm`] writing back into `x`.
pub fn rmsnorm_in_place(x: &mut [f32], weight: &[f32], eps: f32) {
    let mean_sq = dot(x, x) / x.len() as f32;
    let inv = 1.0 / sqrtf(mean_sq + eps);
    for (v, w) in x.iter_mut().zip(weight) {
        *v = *v * inv * w;
    }
}

/// Numerically stable in-place softmax.
pub fn softmax(x: &mut [f32]) {
    let max = x.iter().fold(f32::NEG_INFINITY, |m, &v| m.max(v));
    let mut sum = 0.0;
    for v in x.iter_mut() {
        *v = expf(*v - max);
        sum += *v;
    }
    for v in x.iter_mut() {
        *v /= sum;
    }
}

/// SiLU activation: `x * sigmoid(x)`.
pub fn silu(x: f32) -> f32 {
    x / (1.0 + expf(-x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_handles_remainders() {
        let a: alloc::vec::Vec<f32> = (0..19).map(|i| i as f32).collect();
        let expected: f32 = a.iter().map(|v| v * v).sum();
        assert_eq!(dot(&a, &a), expected);
        let q: alloc::vec::Vec<i8> = (0..37).map(|i| (i - 18) as i8).collect();
        let expected: i32 = q.iter().map(|&v| v as i32 * v as i32).sum();
        assert_eq!(dot_i8(&q, &q), expected);
    }

    #[test]
    fn softmax_sums_to_one() {
        let mut x = [1.0, 2.0, 3.0, 1000.0];
        softmax(&mut x);
        assert!((x.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!(x[3] > 0.99);
    }

    #[test]
    fn rmsnorm_unit_weight() {
        let x = [3.0, 4.0];
        let mut out = [0.0; 2];
        rmsnorm(&mut out, &x, &[1.0, 1.0], 0.0);
        let rms = (12.5f32).sqrt();
        assert!((out[0] - 3.0 / rms).abs() < 1e-6 && (out[1] - 4.0 / rms).abs() < 1e-6);
    }
}
