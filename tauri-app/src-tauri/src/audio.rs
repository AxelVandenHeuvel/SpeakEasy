use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

const TARGET_SAMPLE_RATE: u32 = 16000;

/// Shared buffer that the audio callback writes into
pub type SharedBuffer = Arc<Mutex<Vec<f32>>>;

pub fn new_buffer() -> SharedBuffer {
    Arc::new(Mutex::new(Vec::new()))
}

/// Start recording into the shared buffer. Returns a stop handle.
/// The returned Box must be kept alive — dropping it stops recording.
pub fn start_recording(buffer: &SharedBuffer) -> Result<Box<dyn std::any::Any>, String> {
    let host = cpal::default_host();
    let device = host.default_input_device()
        .ok_or("No input device found")?;
    let config = device.default_input_config()
        .map_err(|e| format!("No input config: {}", e))?;

    let hw_rate = config.sample_rate().0;
    buffer.lock().unwrap().clear();
    let buf = buffer.clone();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let resampled = resample(data, hw_rate, TARGET_SAMPLE_RATE);
                    buf.lock().unwrap().extend_from_slice(&resampled);
                },
                |err| eprintln!("[Audio] Error: {}", err),
                None,
            ).map_err(|e| format!("Build stream: {}", e))?
        }
        cpal::SampleFormat::I16 => {
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let floats: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    let resampled = resample(&floats, hw_rate, TARGET_SAMPLE_RATE);
                    buf.lock().unwrap().extend_from_slice(&resampled);
                },
                |err| eprintln!("[Audio] Error: {}", err),
                None,
            ).map_err(|e| format!("Build stream: {}", e))?
        }
        fmt => return Err(format!("Unsupported format: {:?}", fmt)),
    };

    stream.play().map_err(|e| format!("Play: {}", e))?;

    // Return the stream wrapped in a non-Send-requiring way
    // We keep it on this thread; caller holds the Box to keep it alive
    Ok(Box::new(stream))
}

/// Take the recorded samples from the buffer
pub fn take_samples(buffer: &SharedBuffer) -> Vec<f32> {
    let mut buf = buffer.lock().unwrap();
    let samples = buf.clone();
    buf.clear();
    samples
}

fn resample(data: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return data.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let out_len = (data.len() as f64 * ratio) as usize;
    if data.is_empty() { return vec![]; }
    (0..out_len).map(|i| {
        let src_idx = i as f64 / ratio;
        let lo = src_idx as usize;
        let hi = (lo + 1).min(data.len() - 1);
        let frac = (src_idx - lo as f64) as f32;
        data[lo] * (1.0 - frac) + data[hi] * frac
    }).collect()
}
