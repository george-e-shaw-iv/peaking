/// System audio capture using WASAPI loopback mode.
///
/// Loopback mode captures whatever the system is playing on the default render
/// endpoint — i.e. game audio — without requiring a virtual audio device.
///
/// On non-Windows platforms the public API compiles but `run` returns an error.
use anyhow::Result;
use tokio::sync::{mpsc, watch};

/// A chunk of raw interleaved PCM audio from the system output device.
#[derive(Debug)]
pub struct RawAudio {
    /// Interleaved float-32 samples: [L0, R0, L1, R1, …]
    pub samples_f32: Vec<f32>,
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use std::time::Duration;

    use anyhow::{Context, Result};
    use tokio::sync::{mpsc, watch};
    use windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK, IAudioCaptureClient, IAudioClient,
        IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
        WAVEFORMATEX,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL,
        COINIT_MULTITHREADED,
    };

    use super::RawAudio;

    /// Safety: with COINIT_MULTITHREADED (MTA), WASAPI COM objects are safe to
    /// use from any thread in the process. Wrapping them here lets the async
    /// future be `Send` as required by `tokio::spawn`.
    struct SendAudioState {
        audio_client: IAudioClient,
        capture_client: IAudioCaptureClient,
    }
    unsafe impl Send for SendAudioState {}

    pub async fn run(
        audio_tx: mpsc::Sender<RawAudio>,
        mut stop_rx: watch::Receiver<bool>,
    ) -> Result<()> {
        // ── Synchronous initialisation (no await) ─────────────────────────────
        //
        // Nested blocks ensure `enumerator` and `device` are dropped before
        // this section completes, so they are never captured in the async
        // state machine that spans the loop below.
        let (state, channels) = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let audio_client: IAudioClient = {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                        .context("Failed to create IMMDeviceEnumerator")?;
                let device = enumerator
                    .GetDefaultAudioEndpoint(eRender, eConsole)
                    .context("Failed to get default audio render endpoint")?;
                device
                    .Activate(CLSCTX_ALL, None)
                    .context("Failed to activate IAudioClient")?
            }; // enumerator and device dropped here

            let fmt_ptr: *mut WAVEFORMATEX = audio_client
                .GetMixFormat()
                .context("GetMixFormat failed")?;
            let fmt = &*fmt_ptr;
            let channels = fmt.nChannels;
            let sample_rate = fmt.nSamplesPerSec;

            // 200 ms buffer in 100-nanosecond units.
            let buffer_duration: i64 = 200 * 10_000;
            audio_client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    buffer_duration,
                    0,
                    fmt_ptr,
                    None,
                )
                .context("IAudioClient::Initialize failed")?;

            CoTaskMemFree(Some(fmt_ptr as *mut _));

            let capture_client: IAudioCaptureClient = audio_client
                .GetService()
                .context("Failed to get IAudioCaptureClient")?;

            audio_client.Start().context("IAudioClient::Start failed")?;

            eprintln!("[audio] WASAPI loopback started ({}ch @ {}Hz)", channels, sample_rate);

            (SendAudioState { audio_client, capture_client }, channels)
        };

        // ── Async capture loop ────────────────────────────────────────────────
        loop {
            if *stop_rx.borrow_and_update() {
                break;
            }

            let next_packet_size = unsafe { state.capture_client.GetNextPacketSize()? };
            if next_packet_size == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }

            // Scope the raw pointer to before the .await so it is never held
            // across a suspension point.
            let samples = unsafe {
                let mut data_ptr = std::ptr::null_mut();
                let mut num_frames: u32 = 0;
                let mut flags: u32 = 0;

                state.capture_client
                    .GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
                    .context("GetBuffer failed")?;

                let num_samples = num_frames as usize * channels as usize;

                let samples: Vec<f32> = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                    vec![0.0f32; num_samples]
                } else {
                    // WASAPI in shared mode with FLOAT mix format delivers IEEE 754 f32.
                    std::slice::from_raw_parts(data_ptr as *const f32, num_samples).to_vec()
                };

                state.capture_client
                    .ReleaseBuffer(num_frames)
                    .context("ReleaseBuffer failed")?;

                samples
            };

            let _ = audio_tx.send(RawAudio { samples_f32: samples }).await;
        }

        unsafe { state.audio_client.Stop()? };
        eprintln!("[audio] WASAPI loopback stopped");
        Ok(())
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Captures system audio output (loopback) using WASAPI, sending [`RawAudio`]
/// chunks to `audio_tx` until `stop_rx` is set to `true`.
pub async fn run(
    audio_tx: mpsc::Sender<RawAudio>,
    stop_rx: watch::Receiver<bool>,
) -> Result<()> {
    #[cfg(windows)]
    {
        imp::run(audio_tx, stop_rx).await
    }
    #[cfg(not(windows))]
    {
        let _ = (audio_tx, stop_rx);
        anyhow::bail!("Audio capture (WASAPI) is only supported on Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_audio_stores_data() {
        let samples = vec![0.5f32, -0.5f32];
        let audio = RawAudio { samples_f32: samples.clone() };
        assert_eq!(audio.samples_f32, samples);
    }

    /// On non-Windows the `run` stub must return an error immediately.
    #[cfg(not(windows))]
    #[tokio::test]
    async fn run_returns_error_on_non_windows() {
        let (tx, _rx) = mpsc::channel(1);
        let (_stop_tx, stop_rx) = watch::channel(false);
        let result = run(tx, stop_rx).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("Windows"));
    }
}
