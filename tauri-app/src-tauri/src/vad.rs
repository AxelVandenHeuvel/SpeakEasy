/// Simple RMS energy-based voice activity detection
pub fn has_speech(samples: &[f32], threshold: f64) -> bool {
    if samples.is_empty() {
        return false;
    }
    let sum_squares: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum_squares / samples.len() as f64).sqrt();
    rms > threshold
}
