/// Encoding pipeline: converts raw BGRA video frames and PCM audio samples into
/// 1-second [`EncodedSegment`]s using NVENC H.264 (with libx264 fallback) and AAC.
///
/// # Windows build requirements
/// Set the `FFMPEG_DIR` environment variable to a static FFmpeg 7.x build with NVENC support
/// before running `cargo build`. Run `scripts/Setup-Ffmpeg.ps1` to set this up via vcpkg.
use anyhow::Result;

use crate::capture::RawFrame;
use crate::audio_capture::RawAudio;
use crate::ring_buffer::{AudioCodecParams, EncodedSegment, VideoCodecParams};

const AV_PKT_FLAG_KEY: i32 = 0x0001;

/// Parameters used to configure the encoder on start-up.
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    /// Target frames-per-second. Also controls GOP size (1 IDR per second).
    pub fps: u32,
    pub sample_rate: u32,
    pub channels: u16,
    /// Video encode bitrate in bits/s (e.g. 8_000_000 for 8 Mbps).
    pub video_bitrate: i64,
    /// Audio encode bitrate in bits/s (e.g. 192_000 for 192 kbps).
    pub audio_bitrate: i64,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 60,
            sample_rate: 48_000,
            channels: 2,
            video_bitrate: 8_000_000,
            audio_bitrate: 192_000,
        }
    }
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use anyhow::{bail, Result};
    use ffmpeg_sys_next as ffsys;
    use std::ptr;

    use super::{AV_PKT_FLAG_KEY, EncoderConfig};
    use crate::audio_capture::RawAudio;
    use crate::capture::RawFrame;
    use crate::ring_buffer::{AudioCodecParams, EncodedPacket, EncodedSegment, VideoCodecParams};

    // ── RAII wrappers ─────────────────────────────────────────────────────────

    struct CodecCtxGuard(*mut ffsys::AVCodecContext);
    unsafe impl Send for CodecCtxGuard {}
    impl Drop for CodecCtxGuard {
        fn drop(&mut self) {
            unsafe { ffsys::avcodec_free_context(&mut self.0) }
        }
    }

    struct SwsCtxGuard(*mut ffsys::SwsContext);
    unsafe impl Send for SwsCtxGuard {}
    impl Drop for SwsCtxGuard {
        fn drop(&mut self) {
            unsafe { ffsys::sws_freeContext(self.0) }
        }
    }

    struct FrameGuard(*mut ffsys::AVFrame);
    unsafe impl Send for FrameGuard {}
    impl Drop for FrameGuard {
        fn drop(&mut self) {
            unsafe { ffsys::av_frame_free(&mut self.0) }
        }
    }

    struct PacketGuard(*mut ffsys::AVPacket);
    unsafe impl Send for PacketGuard {}
    impl Drop for PacketGuard {
        fn drop(&mut self) {
            unsafe { ffsys::av_packet_free(&mut self.0) }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Reads `extradata` out of a codec context after `avcodec_open2`.
    unsafe fn read_extradata(ctx: *mut ffsys::AVCodecContext) -> Vec<u8> {
        if (*ctx).extradata.is_null() || (*ctx).extradata_size == 0 {
            vec![]
        } else {
            std::slice::from_raw_parts((*ctx).extradata, (*ctx).extradata_size as usize).to_vec()
        }
    }

    /// Reinterpret a `&[f32]` as `&[u8]`.
    fn f32_as_u8(s: &[f32]) -> &[u8] {
        unsafe { std::slice::from_raw_parts(s.as_ptr() as *const u8, s.len() * 4) }
    }

    /// Drains all available encoded packets from `ctx` into `out`.
    /// Stops on EAGAIN or EOF — both are normal exits from the receive loop.
    unsafe fn drain_packets(
        ctx: *mut ffsys::AVCodecContext,
        out: &mut Vec<EncodedPacket>,
    ) {
        let pkt = PacketGuard(ffsys::av_packet_alloc());
        if pkt.0.is_null() {
            return;
        }
        loop {
            let ret = ffsys::avcodec_receive_packet(ctx, pkt.0);
            if ret < 0 {
                break; // AVERROR(EAGAIN) or AVERROR_EOF — normal
            }
            let data = if (*pkt.0).data.is_null() || (*pkt.0).size == 0 {
                vec![]
            } else {
                std::slice::from_raw_parts((*pkt.0).data, (*pkt.0).size as usize).to_vec()
            };
            out.push(EncodedPacket {
                data,
                pts: (*pkt.0).pts,
                dts: (*pkt.0).dts,
                duration: (*pkt.0).duration,
                is_key: ((*pkt.0).flags & AV_PKT_FLAG_KEY) != 0,
            });
            ffsys::av_packet_unref(pkt.0);
        }
    }

    // ── SegmentEncoderInner ───────────────────────────────────────────────────

    pub struct SegmentEncoderInner {
        config: EncoderConfig,

        video_ctx: CodecCtxGuard,
        sws_ctx: SwsCtxGuard,
        video_frame_count: u64,
        current_video_packets: Vec<EncodedPacket>,
        pub video_params: VideoCodecParams,

        audio_ctx: CodecCtxGuard,
        /// Accumulates interleaved f32 PCM samples until we have a full encoder frame.
        audio_sample_buf: Vec<f32>,
        audio_frame_size: usize,
        audio_pts: i64,
        current_audio_packets: Vec<EncodedPacket>,
        pub audio_params: AudioCodecParams,
    }

    impl SegmentEncoderInner {
        pub fn new(config: &EncoderConfig) -> Result<Self> {
            unsafe { Self::new_unsafe(config) }
        }

        unsafe fn new_unsafe(config: &EncoderConfig) -> Result<Self> {
            // ── Video encoder ─────────────────────────────────────────────────
            let video_codec = {
                let nvenc = ffsys::avcodec_find_encoder_by_name(b"h264_nvenc\0".as_ptr() as _);
                if !nvenc.is_null() {
                    nvenc
                } else {
                    ffsys::avcodec_find_encoder_by_name(b"libx264\0".as_ptr() as _)
                }
            };
            if video_codec.is_null() {
                bail!("No H.264 encoder found (tried h264_nvenc and libx264)");
            }

            let video_ctx = CodecCtxGuard(ffsys::avcodec_alloc_context3(video_codec));
            if video_ctx.0.is_null() {
                bail!("avcodec_alloc_context3 failed for video encoder");
            }

            (*video_ctx.0).width       = config.width as i32;
            (*video_ctx.0).height      = config.height as i32;
            (*video_ctx.0).pix_fmt     = ffsys::AVPixelFormat::AV_PIX_FMT_NV12;
            (*video_ctx.0).time_base   = ffsys::AVRational { num: 1, den: config.fps as i32 };
            (*video_ctx.0).framerate   = ffsys::AVRational { num: config.fps as i32, den: 1 };
            (*video_ctx.0).bit_rate    = config.video_bitrate;
            (*video_ctx.0).gop_size    = config.fps as i32; // one IDR per second
            (*video_ctx.0).max_b_frames = 0;
            // AV_CODEC_FLAG_GLOBAL_HEADER: put SPS+PPS in extradata (required for MP4).
            (*video_ctx.0).flags      |= ffsys::AV_CODEC_FLAG_GLOBAL_HEADER as i32;

            let mut opts: *mut ffsys::AVDictionary = ptr::null_mut();
            ffsys::av_dict_set(&mut opts, b"preset\0".as_ptr() as _, b"p4\0".as_ptr() as _, 0);
            ffsys::av_dict_set(&mut opts, b"tune\0".as_ptr() as _,   b"ull\0".as_ptr() as _, 0);
            ffsys::av_dict_set(&mut opts, b"rc\0".as_ptr() as _,     b"vbr\0".as_ptr() as _, 0);
            let ret = ffsys::avcodec_open2(video_ctx.0, video_codec, &mut opts);
            ffsys::av_dict_free(&mut opts);
            if ret < 0 {
                bail!("Failed to open H.264 encoder (code {ret})");
            }

            let video_params = VideoCodecParams {
                extradata: read_extradata(video_ctx.0),
                width: config.width,
                height: config.height,
                time_base: (1, config.fps as i32),
            };

            // ── Scaler: BGRA → NV12 ───────────────────────────────────────────
            let sws_ctx = SwsCtxGuard(ffsys::sws_getContext(
                config.width as i32,  config.height as i32, ffsys::AVPixelFormat::AV_PIX_FMT_BGRA,
                config.width as i32,  config.height as i32, ffsys::AVPixelFormat::AV_PIX_FMT_NV12,
                ffsys::SwsFlags::SWS_BILINEAR as i32,
                ptr::null_mut(), ptr::null_mut(), ptr::null(),
            ));
            if sws_ctx.0.is_null() {
                bail!("sws_getContext failed (BGRA→NV12)");
            }

            // ── Audio encoder ─────────────────────────────────────────────────
            let audio_codec = ffsys::avcodec_find_encoder(ffsys::AVCodecID::AV_CODEC_ID_AAC);
            if audio_codec.is_null() {
                bail!("AAC encoder not found");
            }

            let audio_ctx = CodecCtxGuard(ffsys::avcodec_alloc_context3(audio_codec));
            if audio_ctx.0.is_null() {
                bail!("avcodec_alloc_context3 failed for audio encoder");
            }

            (*audio_ctx.0).sample_rate = config.sample_rate as i32;
            (*audio_ctx.0).sample_fmt  = ffsys::AVSampleFormat::AV_SAMPLE_FMT_FLTP;
            (*audio_ctx.0).bit_rate    = config.audio_bitrate;
            // FFmpeg 7.x channel layout API (replaces deprecated uint64 mask field).
            (*audio_ctx.0).ch_layout.order       = ffsys::AVChannelOrder::AV_CHANNEL_ORDER_NATIVE;
            (*audio_ctx.0).ch_layout.nb_channels = 2;
            (*audio_ctx.0).ch_layout.u.mask      = 0x3; // AV_CH_LAYOUT_STEREO
            (*audio_ctx.0).flags |= ffsys::AV_CODEC_FLAG_GLOBAL_HEADER as i32;

            let mut opts: *mut ffsys::AVDictionary = ptr::null_mut();
            let ret = ffsys::avcodec_open2(audio_ctx.0, audio_codec, &mut opts);
            if ret < 0 {
                bail!("Failed to open AAC encoder (code {ret})");
            }

            let audio_frame_size = (*audio_ctx.0).frame_size as usize;
            let audio_params = AudioCodecParams {
                extradata: read_extradata(audio_ctx.0),
                sample_rate: config.sample_rate,
                channels: config.channels as u32,
                time_base: (1, config.sample_rate as i32),
            };

            Ok(Self {
                config: config.clone(),
                video_ctx,
                sws_ctx,
                video_frame_count: 0,
                current_video_packets: vec![],
                video_params,
                audio_ctx,
                audio_sample_buf: Vec::new(),
                audio_frame_size,
                audio_pts: 0,
                current_audio_packets: vec![],
                audio_params,
            })
        }

        /// Encodes one BGRA video frame. Returns `Some(segment)` when a complete
        /// 1-second segment boundary is crossed (a new IDR frame is emitted).
        pub fn push_video_frame(&mut self, frame: &RawFrame) -> Result<Option<EncodedSegment>> {
            unsafe { self.push_video_frame_unsafe(frame) }
        }

        unsafe fn push_video_frame_unsafe(
            &mut self,
            frame: &RawFrame,
        ) -> Result<Option<EncodedSegment>> {
            // Allocate and fill the BGRA source frame.
            let bgra = FrameGuard(ffsys::av_frame_alloc());
            if bgra.0.is_null() { bail!("av_frame_alloc failed (bgra)"); }
            (*bgra.0).format = ffsys::AVPixelFormat::AV_PIX_FMT_BGRA as i32;
            (*bgra.0).width  = self.config.width as i32;
            (*bgra.0).height = self.config.height as i32;
            let ret = ffsys::av_frame_get_buffer(bgra.0, 0);
            if ret < 0 { bail!("av_frame_get_buffer(bgra) failed: {ret}"); }

            let stride    = (*bgra.0).linesize[0] as usize;
            let row_bytes = self.config.width as usize * 4;
            let height    = self.config.height as usize;
            let dst = std::slice::from_raw_parts_mut((*bgra.0).data[0], stride * height);
            for row in 0..height {
                let src = &frame.bgra_data[row * row_bytes..(row + 1) * row_bytes];
                dst[row * stride..row * stride + row_bytes].copy_from_slice(src);
            }

            // Allocate the NV12 destination frame.
            let nv12 = FrameGuard(ffsys::av_frame_alloc());
            if nv12.0.is_null() { bail!("av_frame_alloc failed (nv12)"); }
            (*nv12.0).format = ffsys::AVPixelFormat::AV_PIX_FMT_NV12 as i32;
            (*nv12.0).width  = self.config.width as i32;
            (*nv12.0).height = self.config.height as i32;
            let ret = ffsys::av_frame_get_buffer(nv12.0, 0);
            if ret < 0 { bail!("av_frame_get_buffer(nv12) failed: {ret}"); }

            // Scale BGRA → NV12.
            ffsys::sws_scale(
                self.sws_ctx.0,
                (*bgra.0).data.as_ptr() as *const *const u8,
                (*bgra.0).linesize.as_ptr(),
                0, self.config.height as i32,
                (*nv12.0).data.as_mut_ptr(),
                (*nv12.0).linesize.as_ptr(),
            );

            (*nv12.0).pts = self.video_frame_count as i64;
            self.video_frame_count += 1;

            let ret = ffsys::avcodec_send_frame(self.video_ctx.0, nv12.0);
            if ret < 0 { bail!("avcodec_send_frame(video) failed: {ret}"); }

            // Drain encoded packets; detect IDR boundaries.
            let prev_len = self.current_video_packets.len();
            drain_packets(self.video_ctx.0, &mut self.current_video_packets);

            let mut new_segment = None;
            // If a new IDR arrived and there was already data, split a segment.
            // The IDR is the first packet appended after prev_len.
            if let Some(first_new) = self.current_video_packets.get(prev_len) {
                if first_new.is_key && prev_len > 0 {
                    let new_video = self.current_video_packets.split_off(prev_len);
                    new_segment = Some(EncodedSegment {
                        video_packets: std::mem::replace(&mut self.current_video_packets, new_video),
                        audio_packets: std::mem::take(&mut self.current_audio_packets),
                    });
                }
            }

            Ok(new_segment)
        }

        /// Feeds raw interleaved PCM audio into the AAC encoder.
        /// Buffers internally until a full encoder frame is available.
        pub fn push_audio(&mut self, audio: &RawAudio) -> Result<()> {
            unsafe { self.push_audio_unsafe(audio) }
        }

        unsafe fn push_audio_unsafe(&mut self, audio: &RawAudio) -> Result<()> {
            self.audio_sample_buf.extend_from_slice(&audio.samples_f32);

            let channels             = self.config.channels as usize;
            let samples_per_frame    = self.audio_frame_size;
            let interleaved_per_frame = samples_per_frame * channels;

            while self.audio_sample_buf.len() >= interleaved_per_frame {
                let chunk: Vec<f32> = self.audio_sample_buf
                    .drain(..interleaved_per_frame)
                    .collect();

                let af = FrameGuard(ffsys::av_frame_alloc());
                if af.0.is_null() { bail!("av_frame_alloc failed (audio)"); }
                (*af.0).format     = ffsys::AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32;
                (*af.0).nb_samples = samples_per_frame as i32;
                (*af.0).pts        = self.audio_pts;
                self.audio_pts    += samples_per_frame as i64;

                // Copy channel layout from the codec context.
                let ret = ffsys::av_channel_layout_copy(
                    &mut (*af.0).ch_layout,
                    &(*self.audio_ctx.0).ch_layout,
                );
                if ret < 0 { bail!("av_channel_layout_copy failed: {ret}"); }

                let ret = ffsys::av_frame_get_buffer(af.0, 0);
                if ret < 0 { bail!("av_frame_get_buffer(audio) failed: {ret}"); }

                // De-interleave: [L0,R0,L1,R1,…] → plane 0 = L, plane 1 = R.
                let left:  Vec<f32> = chunk.iter().step_by(2).copied().collect();
                let right: Vec<f32> = chunk.iter().skip(1).step_by(2).copied().collect();
                let lb = f32_as_u8(&left);
                let rb = f32_as_u8(&right);
                std::slice::from_raw_parts_mut((*af.0).data[0] as *mut u8, lb.len())
                    .copy_from_slice(lb);
                std::slice::from_raw_parts_mut((*af.0).data[1] as *mut u8, rb.len())
                    .copy_from_slice(rb);

                let ret = ffsys::avcodec_send_frame(self.audio_ctx.0, af.0);
                if ret < 0 { bail!("avcodec_send_frame(audio) failed: {ret}"); }

                drain_packets(self.audio_ctx.0, &mut self.current_audio_packets);
            }

            Ok(())
        }

        /// Flushes remaining buffered packets as a final partial segment.
        pub fn flush(&mut self) -> Result<Option<EncodedSegment>> {
            unsafe { self.flush_unsafe() }
        }

        unsafe fn flush_unsafe(&mut self) -> Result<Option<EncodedSegment>> {
            // Signal EOF to both encoders.
            ffsys::avcodec_send_frame(self.video_ctx.0, ptr::null());
            drain_packets(self.video_ctx.0, &mut self.current_video_packets);

            ffsys::avcodec_send_frame(self.audio_ctx.0, ptr::null());
            drain_packets(self.audio_ctx.0, &mut self.current_audio_packets);

            if self.current_video_packets.is_empty() && self.current_audio_packets.is_empty() {
                return Ok(None);
            }

            Ok(Some(EncodedSegment {
                video_packets: std::mem::take(&mut self.current_video_packets),
                audio_packets: std::mem::take(&mut self.current_audio_packets),
            }))
        }
    }
}

// ── Public SegmentEncoder (platform-dispatched) ───────────────────────────────

/// Encodes raw video and audio into 1-second [`EncodedSegment`]s.
///
/// On Windows, backed by NVENC H.264 + AAC via `ffmpeg-sys-next`.
/// On other platforms, all methods compile but return `Ok(None)` / `Ok(())`.
#[cfg(windows)]
pub struct SegmentEncoder {
    inner: imp::SegmentEncoderInner,
    pub video_params: VideoCodecParams,
    pub audio_params: AudioCodecParams,
}

#[cfg(not(windows))]
pub struct SegmentEncoder {
    pub video_params: VideoCodecParams,
    pub audio_params: AudioCodecParams,
}

impl SegmentEncoder {
    pub fn new(config: &EncoderConfig) -> Result<Self> {
        #[cfg(windows)]
        {
            let inner = imp::SegmentEncoderInner::new(config)?;
            let video_params = inner.video_params.clone();
            let audio_params = inner.audio_params.clone();
            Ok(Self { inner, video_params, audio_params })
        }
        #[cfg(not(windows))]
        {
            // Stub: return default params so the ring buffer can be initialised.
            Ok(Self {
                video_params: VideoCodecParams {
                    extradata: vec![],
                    width: config.width,
                    height: config.height,
                    time_base: (1, config.fps as i32),
                },
                audio_params: AudioCodecParams {
                    extradata: vec![],
                    sample_rate: config.sample_rate,
                    channels: config.channels as u32,
                    time_base: (1, config.sample_rate as i32),
                },
            })
        }
    }

    /// Returns `Some(segment)` when a new IDR frame signals a 1-second boundary.
    pub fn push_video_frame(&mut self, frame: &RawFrame) -> Result<Option<EncodedSegment>> {
        #[cfg(windows)]
        { self.inner.push_video_frame(frame) }
        #[cfg(not(windows))]
        { let _ = frame; Ok(None) }
    }

    /// Feeds raw interleaved PCM into the AAC encoder.
    pub fn push_audio(&mut self, audio: &RawAudio) -> Result<()> {
        #[cfg(windows)]
        { self.inner.push_audio(audio) }
        #[cfg(not(windows))]
        { let _ = audio; Ok(()) }
    }

    /// Flush remaining packets as a final partial segment.
    pub fn flush(&mut self) -> Result<Option<EncodedSegment>> {
        #[cfg(windows)]
        { self.inner.flush() }
        #[cfg(not(windows))]
        { Ok(None) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::RawFrame;
    use crate::audio_capture::RawAudio;

    // ── EncoderConfig defaults ─────────────────────────────────────────────────

    #[test]
    fn encoder_config_default_values() {
        let cfg = EncoderConfig::default();
        assert_eq!(cfg.width, 1920);
        assert_eq!(cfg.height, 1080);
        assert_eq!(cfg.fps, 60);
        assert_eq!(cfg.sample_rate, 48_000);
        assert_eq!(cfg.channels, 2);
        assert_eq!(cfg.video_bitrate, 8_000_000);
        assert_eq!(cfg.audio_bitrate, 192_000);
    }

    // ── Non-Windows stub behaviour ─────────────────────────────────────────────

    #[cfg(not(windows))]
    #[test]
    fn stub_new_succeeds() {
        let cfg = EncoderConfig::default();
        assert!(SegmentEncoder::new(&cfg).is_ok());
    }

    #[cfg(not(windows))]
    #[test]
    fn stub_push_video_frame_returns_none() {
        let cfg = EncoderConfig::default();
        let mut enc = SegmentEncoder::new(&cfg).unwrap();
        let frame = RawFrame { bgra_data: vec![0u8; 1920 * 1080 * 4] };
        let result = enc.push_video_frame(&frame).unwrap();
        assert!(result.is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn stub_push_audio_returns_ok() {
        let cfg = EncoderConfig::default();
        let mut enc = SegmentEncoder::new(&cfg).unwrap();
        let audio = RawAudio { samples_f32: vec![0.0f32; 1024] };
        assert!(enc.push_audio(&audio).is_ok());
    }

    #[cfg(not(windows))]
    #[test]
    fn stub_flush_returns_none() {
        let cfg = EncoderConfig::default();
        let mut enc = SegmentEncoder::new(&cfg).unwrap();
        let result = enc.flush().unwrap();
        assert!(result.is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn stub_codec_params_have_correct_dimensions() {
        let cfg = EncoderConfig {
            width: 2560,
            height: 1440,
            fps: 30,
            sample_rate: 44_100,
            channels: 2,
            video_bitrate: 10_000_000,
            audio_bitrate: 128_000,
        };
        let enc = SegmentEncoder::new(&cfg).unwrap();
        assert_eq!(enc.video_params.width, 2560);
        assert_eq!(enc.video_params.height, 1440);
        assert_eq!(enc.video_params.time_base, (1, 30));
        assert_eq!(enc.audio_params.sample_rate, 44_100);
        assert_eq!(enc.audio_params.channels, 2);
    }
}
