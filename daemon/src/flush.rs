/// Clip flushing: mux all segments currently in the ring buffer into an MP4 file.
///
/// The output path is derived from the configured clip directory, the active
/// application's display name, and the current local timestamp:
///   `<clip_output_dir>\<display_name>\YYYY-MM-DD_HH-MM-SS.mp4`
///
/// On Windows the mux is performed by calling into the FFmpeg C API directly
/// (via `ffmpeg_next::sys`) so that we can set up pre-encoded H.264 and AAC
/// streams with their stored `extradata` blobs. `movflags=faststart` is set
/// before writing the header so the `moov` atom ends up at the front of the
/// file — no separate `qt-faststart` pass is needed.
use anyhow::Result;
use std::path::PathBuf;

use crate::ring_buffer::{AudioCodecParams, EncodedSegment, VideoCodecParams};

// ── Path helpers ───────────────────────────────────────────────────────────────

/// Expands common `%VAR%`-style environment variables embedded in Windows paths.
fn expand_env(s: &str) -> String {
    let mut result = s.to_string();
    for var in &["USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP"] {
        if let Ok(val) = std::env::var(var) {
            result = result.replace(&format!("%{var}%"), &val);
        }
    }
    result
}

/// Replaces characters that are illegal in Windows path components with `_`.
fn sanitize_dirname(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c => c,
        })
        .collect()
}

/// Returns the current local time formatted as `YYYY-MM-DD_HH-MM-SS`.
fn local_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

/// Builds the output path `<clip_output_dir>/<display_name>/<timestamp>.mp4`
/// and creates the full directory tree if it does not already exist.
pub fn build_output_path(clip_output_dir: &str, display_name: &str) -> Result<PathBuf> {
    let base = PathBuf::from(expand_env(clip_output_dir));
    let game_dir = base.join(sanitize_dirname(display_name));
    std::fs::create_dir_all(&game_dir)?;
    Ok(game_dir.join(format!("{}.mp4", local_timestamp())))
}

// ── Public flush entry point ───────────────────────────────────────────────────

/// Muxes `segments` into an MP4 file and returns the path of the saved clip.
///
/// The ffmpeg work runs on a blocking thread via [`tokio::task::spawn_blocking`]
/// so the async event loop stays responsive while the file is being written.
///
/// On non-Windows builds this always returns an error (the encoder never
/// produces segments on those platforms either, so this is never reached in
/// practice).
pub async fn flush_to_disk(
    segments: Vec<EncodedSegment>,
    video_params: VideoCodecParams,
    audio_params: AudioCodecParams,
    clip_output_dir: String,
    display_name: String,
) -> Result<PathBuf> {
    if segments.is_empty() {
        anyhow::bail!("Ring buffer is empty — nothing to save");
    }

    // On non-Windows the encoder never runs so this path is never reached,
    // but it must compile cleanly for `cargo check`.
    #[cfg(not(windows))]
    {
        let _ = (segments, video_params, audio_params, clip_output_dir, display_name);
        anyhow::bail!("Clip flushing is only supported on Windows");
    }

    #[cfg(windows)]
    {
        let output_path = build_output_path(&clip_output_dir, &display_name)?;
        let path = output_path.clone();
        tokio::task::spawn_blocking(move || {
            imp::mux_to_mp4(&segments, &video_params, &audio_params, &path)
        })
        .await
        .map_err(|e| anyhow::anyhow!("Flush task panicked: {e}"))??;
        Ok(output_path)
    }
}

// ── Windows mux implementation ─────────────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use anyhow::{bail, Result};
    use ffmpeg_sys_next as ffsys;
    use std::ffi::CString;
    use std::path::PathBuf;

    use crate::ring_buffer::{AudioCodecParams, EncodedPacket, EncodedSegment, VideoCodecParams};

    /// RAII guard that always frees the `AVFormatContext` when dropped.
    struct OctxGuard(*mut ffsys::AVFormatContext);

    impl Drop for OctxGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { ffsys::avformat_free_context(self.0) };
                self.0 = std::ptr::null_mut();
            }
        }
    }

    /// Muxes `segments` into an MP4 file at `output_path`.
    ///
    /// Sets `movflags=faststart` on the MP4 muxer so the `moov` atom is written
    /// to the front of the file (qt-faststart equivalent).
    pub fn mux_to_mp4(
        segments: &[EncodedSegment],
        video_params: &VideoCodecParams,
        audio_params: &AudioCodecParams,
        output_path: &PathBuf,
    ) -> Result<()> {
        let path_str = output_path.to_string_lossy();
        let path_c = CString::new(path_str.as_ref())
            .map_err(|e| anyhow::anyhow!("Invalid path characters: {e}"))?;

        unsafe {
            // ── Allocate output format context ────────────────────────────────
            let mut raw_octx: *mut ffsys::AVFormatContext = std::ptr::null_mut();
            let ret = ffsys::avformat_alloc_output_context2(
                &mut raw_octx,
                std::ptr::null_mut(),
                std::ptr::null(),
                path_c.as_ptr(),
            );
            if ret < 0 || raw_octx.is_null() {
                bail!("avformat_alloc_output_context2 failed ({})", ret);
            }
            // Guard ensures avformat_free_context is always called.
            let _guard = OctxGuard(raw_octx);
            let octx = raw_octx;

            // ── Video stream (H.264) ──────────────────────────────────────────
            let vstream = ffsys::avformat_new_stream(octx, std::ptr::null());
            if vstream.is_null() {
                bail!("Failed to create video stream");
            }
            (*vstream).id = 0;
            {
                let vpar = (*vstream).codecpar;
                (*vpar).codec_type = ffsys::AVMediaType::AVMEDIA_TYPE_VIDEO;
                (*vpar).codec_id = ffsys::AVCodecID::AV_CODEC_ID_H264;
                (*vpar).width = video_params.width as i32;
                (*vpar).height = video_params.height as i32;
                (*vpar).format = ffsys::AVPixelFormat::AV_PIX_FMT_YUV420P as i32;
                if !video_params.extradata.is_empty() {
                    copy_extradata(vpar, &video_params.extradata);
                }
            }
            (*vstream).time_base = ffsys::AVRational {
                num: video_params.time_base.0,
                den: video_params.time_base.1,
            };

            // ── Audio stream (AAC) ────────────────────────────────────────────
            let astream = ffsys::avformat_new_stream(octx, std::ptr::null());
            if astream.is_null() {
                bail!("Failed to create audio stream");
            }
            (*astream).id = 1;
            {
                let apar = (*astream).codecpar;
                (*apar).codec_type = ffsys::AVMediaType::AVMEDIA_TYPE_AUDIO;
                (*apar).codec_id = ffsys::AVCodecID::AV_CODEC_ID_AAC;
                (*apar).sample_rate = audio_params.sample_rate as i32;
                set_channel_layout(apar, audio_params.channels);
                if !audio_params.extradata.is_empty() {
                    copy_extradata(apar, &audio_params.extradata);
                }
            }
            (*astream).time_base = ffsys::AVRational {
                num: audio_params.time_base.0,
                den: audio_params.time_base.1,
            };

            // ── movflags=faststart (moov atom at front — no separate pass needed) ─
            {
                let key = CString::new("movflags").unwrap();
                let val = CString::new("faststart").unwrap();
                ffsys::av_opt_set((*octx).priv_data, key.as_ptr(), val.as_ptr(), 0);
            }

            // ── Open the output file ──────────────────────────────────────────
            let ret = ffsys::avio_open(
                &mut (*octx).pb,
                path_c.as_ptr(),
                ffsys::AVIO_FLAG_WRITE as i32,
            );
            if ret < 0 {
                bail!("avio_open failed ({})", ret);
            }

            // ── Write MP4 header ──────────────────────────────────────────────
            let ret = ffsys::avformat_write_header(octx, std::ptr::null_mut());
            if ret < 0 {
                ffsys::avio_closep(&mut (*octx).pb);
                bail!("avformat_write_header failed ({})", ret);
            }

            // Capture the (possibly muxer-adjusted) output time bases.
            let vtb_out = (*vstream).time_base;
            let atb_out = (*astream).time_base;
            let vtb_in = ffsys::AVRational {
                num: video_params.time_base.0,
                den: video_params.time_base.1,
            };
            let atb_in = ffsys::AVRational {
                num: audio_params.time_base.0,
                den: audio_params.time_base.1,
            };

            // Compute PTS origin so the clip always starts at presentation time 0.
            let video_pts_origin = segments
                .iter()
                .flat_map(|s| s.video_packets.iter())
                .next()
                .map(|p| p.pts)
                .unwrap_or(0);
            let audio_pts_origin = segments
                .iter()
                .flat_map(|s| s.audio_packets.iter())
                .next()
                .map(|p| p.pts)
                .unwrap_or(0);

            // ── Write all packets interleaved ─────────────────────────────────
            for segment in segments {
                for pkt in &segment.video_packets {
                    write_interleaved(octx, pkt, 0, vtb_in, vtb_out, video_pts_origin);
                }
                for pkt in &segment.audio_packets {
                    write_interleaved(octx, pkt, 1, atb_in, atb_out, audio_pts_origin);
                }
            }

            // ── Finalise ──────────────────────────────────────────────────────
            ffsys::av_write_trailer(octx);
            ffsys::avio_closep(&mut (*octx).pb);
            // _guard drops here → avformat_free_context(octx).
        }

        Ok(())
    }

    /// Allocates and copies `data` into `par->extradata` with the required
    /// `AV_INPUT_BUFFER_PADDING_SIZE` zero padding appended.
    unsafe fn copy_extradata(par: *mut ffsys::AVCodecParameters, data: &[u8]) {
        // AV_INPUT_BUFFER_PADDING_SIZE == 64 in FFmpeg 6.x.
        let padding = 64usize;
        let ptr = ffsys::av_mallocz(data.len() + padding) as *mut u8;
        if !ptr.is_null() {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            (*par).extradata = ptr;
            (*par).extradata_size = data.len() as i32;
        }
    }

    /// Sets the channel layout on `par` for the given number of channels.
    ///
    /// FFmpeg uses `AVChannelLayout` in `AVCodecParameters`.
    /// For AAC the `extradata` (AudioSpecificConfig) carries the full channel
    /// description, so setting `nb_channels` is sufficient for the muxer.
    unsafe fn set_channel_layout(par: *mut ffsys::AVCodecParameters, channels: u32) {
        (*par).ch_layout.nb_channels = channels as i32;
        if channels == 2 {
            // AV_CHANNEL_ORDER_NATIVE = 1; AV_CH_LAYOUT_STEREO mask = 0x3.
            (*par).ch_layout.order =
                std::mem::transmute::<u32, ffsys::AVChannelOrder>(1u32);
            (*par).ch_layout.u.mask = 0x3u64;
        }
    }

    /// Allocates a packet, copies the encoded data, rescales timestamps from the
    /// encoder time base to the muxer stream time base, then writes it
    /// interleaved into the output context.
    ///
    /// `pts_origin` is subtracted before rescaling so all clips start at t = 0.
    ///
    /// Errors are logged to stderr; individual packet failures are non-fatal so
    /// that a corrupt packet doesn't abort the entire flush.
    unsafe fn write_interleaved(
        octx: *mut ffsys::AVFormatContext,
        pkt: &EncodedPacket,
        stream_index: i32,
        in_tb: ffsys::AVRational,
        out_tb: ffsys::AVRational,
        pts_origin: i64,
    ) {
        let avpkt = ffsys::av_packet_alloc();
        if avpkt.is_null() {
            eprintln!("[flush] av_packet_alloc returned null — skipping packet");
            return;
        }

        let ret = ffsys::av_new_packet(avpkt, pkt.data.len() as i32);
        if ret < 0 {
            let mut p = avpkt;
            ffsys::av_packet_free(&mut p);
            eprintln!("[flush] av_new_packet failed ({ret}) — skipping packet");
            return;
        }

        std::ptr::copy_nonoverlapping(pkt.data.as_ptr(), (*avpkt).data, pkt.data.len());
        (*avpkt).pts = pkt.pts - pts_origin;
        (*avpkt).dts = pkt.dts - pts_origin;
        (*avpkt).duration = pkt.duration;
        (*avpkt).flags = if pkt.is_key {
            ffsys::AV_PKT_FLAG_KEY as i32
        } else {
            0
        };
        (*avpkt).stream_index = stream_index;

        ffsys::av_packet_rescale_ts(avpkt, in_tb, out_tb);

        let ret = ffsys::av_interleaved_write_frame(octx, avpkt);
        // av_interleaved_write_frame unrefs the packet data on both success and
        // failure; we still need to free the packet struct itself.
        let mut p = avpkt;
        ffsys::av_packet_free(&mut p);

        if ret < 0 {
            eprintln!("[flush] av_interleaved_write_frame failed ({ret})");
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_env_replaces_userprofile() {
        std::env::set_var("USERPROFILE", r"C:\Users\TestUser");
        let result = expand_env(r"%USERPROFILE%\Videos\Peaking");
        assert_eq!(result, r"C:\Users\TestUser\Videos\Peaking");
    }

    #[test]
    fn expand_env_leaves_unknown_vars_intact() {
        let result = expand_env(r"%UNKNOWN_VAR%\path");
        assert_eq!(result, r"%UNKNOWN_VAR%\path");
    }

    #[test]
    fn sanitize_dirname_replaces_illegal_chars() {
        let sanitized = sanitize_dirname(r#"Game: "Sub/Title" <v1>"#);
        assert!(!sanitized.contains(':'));
        assert!(!sanitized.contains('"'));
        assert!(!sanitized.contains('/'));
        assert!(!sanitized.contains('<'));
        assert!(!sanitized.contains('>'));
    }

    #[test]
    fn sanitize_dirname_leaves_safe_chars_intact() {
        let name = "Rocket League 2025";
        assert_eq!(sanitize_dirname(name), name);
    }

    #[test]
    fn local_timestamp_has_correct_format() {
        let ts = local_timestamp();
        // Should match YYYY-MM-DD_HH-MM-SS (19 chars).
        assert_eq!(ts.len(), 19, "Unexpected timestamp length: {ts}");
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "_");
        assert_eq!(&ts[13..14], "-");
        assert_eq!(&ts[16..17], "-");
    }

    #[test]
    fn build_output_path_creates_directory_and_has_mp4_extension() {
        let dir = tempfile::tempdir().unwrap();
        let clip_dir = dir.path().to_string_lossy().into_owned();
        let path = build_output_path(&clip_dir, "Rocket League").unwrap();
        assert!(path.parent().unwrap().exists());
        assert_eq!(path.extension().unwrap(), "mp4");
        assert!(path.parent().unwrap().ends_with("Rocket League"));
    }

    #[test]
    fn build_output_path_sanitizes_display_name() {
        let dir = tempfile::tempdir().unwrap();
        let clip_dir = dir.path().to_string_lossy().into_owned();
        let path = build_output_path(&clip_dir, r#"Game: "Test""#).unwrap();
        let parent_name = path.parent().unwrap().file_name().unwrap().to_string_lossy();
        assert!(!parent_name.contains(':'));
        assert!(!parent_name.contains('"'));
    }

    // ── expand_env: additional variables ──────────────────────────────────────

    #[test]
    fn expand_env_replaces_appdata() {
        std::env::set_var("APPDATA", r"C:\Users\TestUser\AppData\Roaming");
        let result = expand_env(r"%APPDATA%\Peaking");
        assert_eq!(result, r"C:\Users\TestUser\AppData\Roaming\Peaking");
    }

    #[test]
    fn expand_env_replaces_localappdata() {
        std::env::set_var("LOCALAPPDATA", r"C:\Users\TestUser\AppData\Local");
        let result = expand_env(r"%LOCALAPPDATA%\Peaking");
        assert_eq!(result, r"C:\Users\TestUser\AppData\Local\Peaking");
    }

    #[test]
    fn expand_env_replaces_tmp() {
        std::env::set_var("TMP", r"C:\Windows\Temp");
        let result = expand_env(r"%TMP%\cache");
        assert_eq!(result, r"C:\Windows\Temp\cache");
    }

    // ── sanitize_dirname: remaining illegal Windows characters ────────────────

    #[test]
    fn sanitize_dirname_replaces_backslash() {
        let result = sanitize_dirname(r"Game\Sub");
        assert!(!result.contains('\\'), "backslash should be replaced: {result}");
    }

    #[test]
    fn sanitize_dirname_replaces_pipe_star_and_question() {
        let result = sanitize_dirname("Game|Name?Star*");
        assert!(!result.contains('|'), "pipe should be replaced: {result}");
        assert!(!result.contains('*'), "star should be replaced: {result}");
        assert!(!result.contains('?'), "question mark should be replaced: {result}");
    }

    // ── build_output_path: filename format ────────────────────────────────────

    #[test]
    fn build_output_path_filename_has_timestamp_format() {
        let dir = tempfile::tempdir().unwrap();
        let clip_dir = dir.path().to_string_lossy().into_owned();
        let path = build_output_path(&clip_dir, "TestGame").unwrap();
        let stem = path.file_stem().unwrap().to_string_lossy();
        // Stem should be YYYY-MM-DD_HH-MM-SS (19 characters).
        assert_eq!(stem.len(), 19, "Unexpected stem: {stem}");
        assert_eq!(&stem[4..5], "-");
        assert_eq!(&stem[7..8], "-");
        assert_eq!(&stem[10..11], "_");
        assert_eq!(&stem[13..14], "-");
        assert_eq!(&stem[16..17], "-");
    }

    // ── flush_to_disk: empty-segments guard ───────────────────────────────────

    #[tokio::test]
    async fn flush_to_disk_with_empty_segments_returns_error() {
        use crate::ring_buffer::{AudioCodecParams, VideoCodecParams};
        let result = flush_to_disk(
            vec![],
            VideoCodecParams { extradata: vec![], width: 1920, height: 1080, time_base: (1, 60) },
            AudioCodecParams { extradata: vec![], sample_rate: 48_000, channels: 2, time_base: (1, 48_000) },
            std::env::temp_dir().to_string_lossy().into_owned(),
            "TestGame".to_string(),
        )
        .await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("empty"), "Expected 'empty' in error: {msg}");
    }
}
