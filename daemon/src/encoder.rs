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
    use anyhow::{Context, Result};
    use ffmpeg_next::{self as ffmpeg, codec, encoder, format, frame, software, Packet, Rational};
    use ffmpeg_next::format::Pixel;
    use ffmpeg_next::software::scaling::Flags;

    use super::{AV_PKT_FLAG_KEY, EncoderConfig};
    use crate::audio_capture::RawAudio;
    use crate::capture::RawFrame;
    use crate::ring_buffer::{AudioCodecParams, EncodedPacket, EncodedSegment, VideoCodecParams};

    fn packet_to_encoded(pkt: &Packet) -> EncodedPacket {
        EncodedPacket {
            data: pkt.data().unwrap_or(&[]).to_vec(),
            pts: pkt.pts().unwrap_or(0),
            dts: pkt.dts().unwrap_or(0),
            duration: pkt.duration(),
            is_key: (pkt.flags() & AV_PKT_FLAG_KEY) != 0,
        }
    }

    pub struct SegmentEncoderInner {
        config: EncoderConfig,

        video_encoder: encoder::video::Video,
        scaler: software::scaling::Context,
        video_frame_count: u64,
        current_video_packets: Vec<EncodedPacket>,
        pub video_params: VideoCodecParams,

        audio_encoder: encoder::audio::Audio,
        /// Accumulates interleaved f32 PCM samples until we have a full encoder frame.
        audio_sample_buf: Vec<f32>,
        audio_frame_size: usize,
        audio_pts: i64,
        current_audio_packets: Vec<EncodedPacket>,
        pub audio_params: AudioCodecParams,

        segment_index: u64,
    }

    impl SegmentEncoderInner {
        pub fn new(config: &EncoderConfig) -> Result<Self> {
            ffmpeg::init().context("ffmpeg init failed")?;

            // ── Video encoder ────────────────────────────────────────────────
            let video_codec = encoder::find_by_name("h264_nvenc")
                .or_else(|| encoder::find_by_name("libx264"))
                .context("No H.264 encoder found (tried h264_nvenc and libx264)")?;

            let video_ctx = codec::context::Context::new();
            let mut video = video_ctx.encoder().video()?;
            video.set_width(config.width);
            video.set_height(config.height);
            video.set_format(Pixel::NV12);
            video.set_time_base(Rational::new(1, config.fps as i32));
            video.set_frame_rate(Some(Rational::new(config.fps as i32, 1)));
            video.set_bit_rate(config.video_bitrate as usize);

            // Force IDR every fps frames (= 1 second), no B-frames.
            // AV_CODEC_FLAG_GLOBAL_HEADER puts SPS+PPS in extradata (needed for MP4).
            unsafe {
                let p = video.as_mut_ptr();
                (*p).gop_size = config.fps as i32;
                (*p).max_b_frames = 0;
                (*p).flags |= ffmpeg_next::sys::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
            }

            let mut opts = ffmpeg::Dictionary::new();
            opts.set("preset", "p4");
            opts.set("tune", "ull");
            opts.set("rc", "vbr");

            let video_encoder = video.open_as_with(video_codec, opts)
                .context("Failed to open H.264 encoder")?;

            let video_extradata = unsafe {
                let p = video_encoder.as_ptr();
                if (*p).extradata.is_null() || (*p).extradata_size == 0 {
                    vec![]
                } else {
                    std::slice::from_raw_parts((*p).extradata, (*p).extradata_size as usize).to_vec()
                }
            };

            let video_params = VideoCodecParams {
                extradata: video_extradata,
                width: config.width,
                height: config.height,
                fps: config.fps,
                time_base: (1, config.fps as i32),
            };

            let scaler = software::scaling::Context::get(
                Pixel::BGRA,
                config.width,
                config.height,
                Pixel::NV12,
                config.width,
                config.height,
                Flags::BILINEAR,
            ).context("Failed to create BGRA→NV12 scaler")?;

            // ── Audio encoder ────────────────────────────────────────────────
            let audio_codec = encoder::find(codec::Id::AAC)
                .context("AAC encoder not found")?;

            let audio_ctx = codec::context::Context::new();
            let mut audio = audio_ctx.encoder().audio()?;
            audio.set_rate(config.sample_rate as i32);
            audio.set_channel_layout(ffmpeg_next::channel_layout::ChannelLayout::STEREO);
            audio.set_format(
                format::Sample::F32(format::sample::Type::Planar)
            );
            audio.set_bit_rate(config.audio_bitrate as usize);

            unsafe {
                let p = audio.as_mut_ptr();
                (*p).flags |= ffmpeg_next::sys::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
            }

            let audio_encoder = audio.open_as(audio_codec)
                .context("Failed to open AAC encoder")?;

            let audio_frame_size = audio_encoder.frame_size() as usize;

            let audio_extradata = unsafe {
                let p = audio_encoder.as_ptr();
                if (*p).extradata.is_null() || (*p).extradata_size == 0 {
                    vec![]
                } else {
                    std::slice::from_raw_parts((*p).extradata, (*p).extradata_size as usize).to_vec()
                }
            };

            let audio_params = AudioCodecParams {
                extradata: audio_extradata,
                sample_rate: config.sample_rate,
                channels: config.channels as u32,
                time_base: (1, config.sample_rate as i32),
            };

            Ok(Self {
                config: config.clone(),
                video_encoder,
                scaler,
                video_frame_count: 0,
                current_video_packets: vec![],
                video_params,
                audio_encoder,
                audio_sample_buf: Vec::new(),
                audio_frame_size,
                audio_pts: 0,
                current_audio_packets: vec![],
                audio_params,
                segment_index: 0,
            })
        }

        /// Encodes one BGRA video frame.  Returns `Some(segment)` when a complete
        /// 1-second segment boundary is crossed (i.e. a new IDR frame is emitted).
        pub fn push_video_frame(&mut self, frame: &RawFrame) -> Result<Option<EncodedSegment>> {
            // Build BGRA input frame with stride-aware row copy.
            let mut bgra_frame = frame::Video::new(
                Pixel::BGRA,
                self.config.width,
                self.config.height,
            );
            let stride = bgra_frame.stride(0);
            let row_bytes = self.config.width as usize * 4;
            for row in 0..self.config.height as usize {
                let src = &frame.bgra_data[row * row_bytes..(row + 1) * row_bytes];
                let dst_start = row * stride;
                bgra_frame.data_mut(0)[dst_start..dst_start + row_bytes].copy_from_slice(src);
            }

            // Convert BGRA → NV12.
            let mut nv12_frame = frame::Video::new(
                Pixel::NV12,
                self.config.width,
                self.config.height,
            );
            self.scaler.run(&bgra_frame, &mut nv12_frame)?;

            nv12_frame.set_pts(Some(self.video_frame_count as i64));
            self.video_frame_count += 1;

            // Send to NVENC / libx264.
            self.video_encoder.send_frame(&nv12_frame)?;

            // Drain output packets.
            let mut new_segment: Option<EncodedSegment> = None;
            let mut pkt = Packet::empty();
            while self.video_encoder.receive_packet(&mut pkt).is_ok() {
                let encoded = packet_to_encoded(&pkt);
                let is_key = encoded.is_key;

                // A new IDR packet (and we already have data) = segment boundary.
                if is_key && !self.current_video_packets.is_empty() {
                    new_segment = Some(EncodedSegment {
                        index: self.segment_index,
                        video_packets: std::mem::take(&mut self.current_video_packets),
                        audio_packets: std::mem::take(&mut self.current_audio_packets),
                        video_time_base: self.video_params.time_base,
                        audio_time_base: self.audio_params.time_base,
                    });
                    self.segment_index += 1;
                }

                self.current_video_packets.push(encoded);
                pkt = Packet::empty();
            }

            Ok(new_segment)
        }

        /// Feeds raw interleaved PCM audio into the AAC encoder.
        /// Handles arbitrary chunk sizes by buffering internally until a full
        /// encoder frame (typically 1024 samples) is available.
        pub fn push_audio(&mut self, audio: &RawAudio) -> Result<()> {
            self.audio_sample_buf.extend_from_slice(&audio.samples_f32);

            let channels = self.config.channels as usize;
            let samples_per_frame = self.audio_frame_size; // mono samples per channel
            let interleaved_per_frame = samples_per_frame * channels;

            while self.audio_sample_buf.len() >= interleaved_per_frame {
                let chunk: Vec<f32> = self.audio_sample_buf
                    .drain(..interleaved_per_frame)
                    .collect();

                let mut audio_frame = frame::Audio::new(
                    format::Sample::F32(format::sample::Type::Planar),
                    samples_per_frame,
                    ffmpeg_next::channel_layout::ChannelLayout::STEREO,
                );
                audio_frame.set_pts(Some(self.audio_pts));
                self.audio_pts += samples_per_frame as i64;

                // De-interleave: chunk = [L0, R0, L1, R1, ...] → plane 0=L, plane 1=R
                let left: Vec<f32> = chunk.iter().step_by(2).copied().collect();
                let right: Vec<f32> = chunk.iter().skip(1).step_by(2).copied().collect();
                let left_bytes = bytemuck_f32_slice(&left);
                let right_bytes = bytemuck_f32_slice(&right);
                audio_frame.data_mut(0)[..left_bytes.len()].copy_from_slice(left_bytes);
                audio_frame.data_mut(1)[..right_bytes.len()].copy_from_slice(right_bytes);

                self.audio_encoder.send_frame(&audio_frame)?;

                let mut pkt = Packet::empty();
                while self.audio_encoder.receive_packet(&mut pkt).is_ok() {
                    self.current_audio_packets.push(packet_to_encoded(&pkt));
                    pkt = Packet::empty();
                }
            }

            Ok(())
        }

        /// Flushes any remaining buffered packets as a final partial segment.
        /// Call this when the recording session ends.
        pub fn flush(&mut self) -> Result<Option<EncodedSegment>> {
            self.video_encoder.send_eof()?;
            let mut pkt = Packet::empty();
            while self.video_encoder.receive_packet(&mut pkt).is_ok() {
                self.current_video_packets.push(packet_to_encoded(&pkt));
                pkt = Packet::empty();
            }

            self.audio_encoder.send_eof()?;
            let mut pkt = Packet::empty();
            while self.audio_encoder.receive_packet(&mut pkt).is_ok() {
                self.current_audio_packets.push(packet_to_encoded(&pkt));
                pkt = Packet::empty();
            }

            if self.current_video_packets.is_empty() && self.current_audio_packets.is_empty() {
                return Ok(None);
            }

            Ok(Some(EncodedSegment {
                index: self.segment_index,
                video_packets: std::mem::take(&mut self.current_video_packets),
                audio_packets: std::mem::take(&mut self.current_audio_packets),
                video_time_base: self.video_params.time_base,
                audio_time_base: self.audio_params.time_base,
            }))
        }
    }

    /// Reinterpret a `&[f32]` as `&[u8]`.
    fn bytemuck_f32_slice(s: &[f32]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(s.as_ptr() as *const u8, s.len() * 4)
        }
    }
}

// ── Public SegmentEncoder (platform-dispatched) ───────────────────────────────

/// Encodes raw video and audio into 1-second [`EncodedSegment`]s.
///
/// On Windows, backed by NVENC H.264 + AAC via `ffmpeg-next`.
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
                    fps: config.fps,
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
        let frame = RawFrame {
            bgra_data: vec![0u8; 1920 * 1080 * 4],
            width: 1920,
            height: 1080,
            timestamp_ms: 0,
        };
        let result = enc.push_video_frame(&frame).unwrap();
        assert!(result.is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn stub_push_audio_returns_ok() {
        let cfg = EncoderConfig::default();
        let mut enc = SegmentEncoder::new(&cfg).unwrap();
        let audio = RawAudio {
            samples_f32: vec![0.0f32; 1024],
            channels: 2,
            sample_rate: 48_000,
            timestamp_ms: 0,
        };
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
        assert_eq!(enc.video_params.fps, 30);
        assert_eq!(enc.audio_params.sample_rate, 44_100);
        assert_eq!(enc.audio_params.channels, 2);
    }
}
