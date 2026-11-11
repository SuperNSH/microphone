//! The platform backends. Each target gets one `NativeBackend`; a target with none, the web
//! included, reports that nothing is available.

#[cfg(target_os = "android")]
mod android;
#[cfg(target_vendor = "apple")]
mod apple;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "linux",
    target_os = "windows",
    target_arch = "wasm32"
)))]
mod unsupported;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Arc;

use crate::backend::Backend;

#[cfg(target_os = "android")]
use android::NativeBackend;
#[cfg(target_vendor = "apple")]
use apple::NativeBackend;
#[cfg(target_os = "linux")]
use linux::NativeBackend;
#[cfg(not(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "linux",
    target_os = "windows",
    target_arch = "wasm32"
)))]
use unsupported::NativeBackend;
#[cfg(target_arch = "wasm32")]
use web::NativeBackend;
#[cfg(target_os = "windows")]
use windows::NativeBackend;

/// The backend of the target this build runs on.
pub(crate) fn default_backend() -> Arc<dyn Backend> {
    Arc::new(NativeBackend)
}

// Only the targets that write PCM files themselves pull the WAV writer in.
#[cfg(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "windows",
    target_os = "linux"
))]
pub(crate) fn write_wav(
    path: &str,
    sample_rate: u32,
    channels: u16,
    samples: &[i16],
) -> Result<StoppedRecording, RecorderError> {
    use std::{
        fs::File,
        io::{BufWriter, Write},
    };

    use crate::{
        error::{ErrorKind, RecorderError},
        options::StoppedRecording,
    };

    // RIFF sizes are 32-bit; an absurd input saturates instead of wrapping into a corrupt header.
    let data_len = u32::try_from(samples.len().saturating_mul(2)).unwrap_or(u32::MAX);
    let riff_len = u32::try_from(u64::from(data_len) + 36).unwrap_or(u32::MAX);
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    // 36 counts every byte after the RIFF size field: WAVE, the fmt chunk and the data header.
    header.extend_from_slice(&riff_len.to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM, no compression.
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = u64::from(sample_rate) * u64::from(channels) * 2;
    header.extend_from_slice(&u32::try_from(byte_rate).unwrap_or(u32::MAX).to_le_bytes());
    header.extend_from_slice(&channels.saturating_mul(2).to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes()); // bits per sample.
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_len.to_le_bytes());
    let mut data = Vec::with_capacity(usize::from(data_len));
    for sample in samples {
        data.extend_from_slice(&sample.to_le_bytes());
    }

    fn backend_error(error: std::io::Error) -> RecorderError {
        // The OS status travels when there is one, as the reference mapping keeps the host's code.
        match error.raw_os_error() {
            Some(status) => RecorderError::new(ErrorKind::Backend).with_status(i64::from(status)),
            None => RecorderError::new(ErrorKind::Backend),
        }
    }

    let writer = File::create(path).map_err(backend_error)?;
    let mut writer = BufWriter::new(writer);
    writer.write_all(&header).map_err(backend_error)?;
    writer.write_all(&data).map_err(backend_error)?;
    writer.flush().map_err(backend_error)?;

    // Recorder refuses a zero rate before the backend runs; the guards keep this helper
    // panic-free for any caller.
    let frames = if channels == 0 {
        0
    } else {
        (samples.len() / channels as usize) as u64
    };
    let duration_ms = if sample_rate == 0 {
        0
    } else {
        (frames * 1000 + u64::from(sample_rate) / 2) / u64::from(sample_rate)
    };
    Ok(StoppedRecording {
        path: Some(path.to_owned()),
        duration_ms: Some(duration_ms),
    })
}
