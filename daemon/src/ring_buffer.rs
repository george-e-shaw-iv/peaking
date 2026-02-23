use std::collections::VecDeque;

use crate::config::{MAX_BUFFER_LENGTH_SECS, MIN_BUFFER_LENGTH_SECS};

/// A single encoded packet extracted from the encoder output.
/// Carries enough metadata for the MP4 muxer (Phase 9) to reconstruct timing.
#[derive(Debug, Clone)]
pub struct EncodedPacket {
    /// Raw compressed bytes (H.264 NAL units or AAC ADTS frames).
    pub data: Vec<u8>,
    /// Presentation timestamp in codec time-base units.
    pub pts: i64,
    /// Decoding timestamp in codec time-base units.
    pub dts: i64,
    /// Duration in codec time-base units.
    pub duration: i64,
    /// True when this packet starts a new decodable group (IDR frame for H.264).
    pub is_key: bool,
}

/// Codec-level parameters needed to initialise the MP4 muxer during flush.
#[derive(Debug, Clone)]
pub struct VideoCodecParams {
    /// H.264 global header (SPS + PPS in avcC format), written by the encoder
    /// when `AV_CODEC_FLAG_GLOBAL_HEADER` is set.
    pub extradata: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// ffmpeg AVRational time base stored as (num, den).
    pub time_base: (i32, i32),
}

/// Codec-level parameters needed to initialise the MP4 muxer during flush.
#[derive(Debug, Clone)]
pub struct AudioCodecParams {
    /// AAC codec config (AudioSpecificConfig binary blob).
    pub extradata: Vec<u8>,
    pub sample_rate: u32,
    pub channels: u32,
    /// ffmpeg AVRational time base stored as (num, den).
    pub time_base: (i32, i32),
}

/// One complete 1-second window of encoded video and audio.
/// Each segment starts with an IDR (keyframe) so it is independently decodable.
#[derive(Debug, Clone)]
pub struct EncodedSegment {
    /// Sequential segment number, 0-based from the start of the current recording session.
    pub index: u64,
    pub video_packets: Vec<EncodedPacket>,
    pub audio_packets: Vec<EncodedPacket>,
    pub video_time_base: (i32, i32),
    pub audio_time_base: (i32, i32),
}

/// Circular buffer of 1-second [`EncodedSegment`]s.
///
/// Holds at most `capacity` segments (= buffer length in seconds, clamped to
/// [`MIN_BUFFER_LENGTH_SECS`]–[`MAX_BUFFER_LENGTH_SECS`]).  When full, the
/// oldest segment is evicted to make room for the newest.
pub struct RingBuffer {
    segments: VecDeque<EncodedSegment>,
    /// Maximum number of segments to retain (= buffer length in seconds).
    capacity: usize,
    /// Codec parameters set once when the encoder is first opened.
    pub video_params: Option<VideoCodecParams>,
    pub audio_params: Option<AudioCodecParams>,
}

impl RingBuffer {
    /// Creates an empty ring buffer with the given capacity in seconds.
    /// The capacity is clamped to the configured min/max.
    pub fn new(capacity_secs: u32) -> Self {
        Self {
            segments: VecDeque::new(),
            capacity: Self::clamp_capacity(capacity_secs),
            video_params: None,
            audio_params: None,
        }
    }

    /// Pushes a new segment, evicting the oldest if the buffer is at capacity.
    pub fn push(&mut self, segment: EncodedSegment) {
        if self.segments.len() == self.capacity {
            self.segments.pop_front();
        }
        self.segments.push_back(segment);
    }

    /// Drains all segments out of the buffer (consuming them) and returns them
    /// in chronological order. Used by the flush operation (Phase 9).
    pub fn take_all(&mut self) -> Vec<EncodedSegment> {
        self.segments.drain(..).collect()
    }

    /// Returns a slice view of all segments without removing them.
    pub fn segments(&self) -> &VecDeque<EncodedSegment> {
        &self.segments
    }

    pub fn len(&self) -> usize {
        self.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Clears all segments (e.g. when a new recording session starts).
    pub fn clear(&mut self) {
        self.segments.clear();
    }

    /// Resizes the buffer to `capacity_secs` seconds, clamped to min/max.
    /// If the new capacity is smaller than the current fill level, the oldest
    /// segments are dropped.
    pub fn resize(&mut self, capacity_secs: u32) {
        let new_cap = Self::clamp_capacity(capacity_secs);
        self.capacity = new_cap;
        while self.segments.len() > self.capacity {
            self.segments.pop_front();
        }
    }

    fn clamp_capacity(secs: u32) -> usize {
        secs.clamp(MIN_BUFFER_LENGTH_SECS, MAX_BUFFER_LENGTH_SECS) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_segment(index: u64) -> EncodedSegment {
        EncodedSegment {
            index,
            video_packets: vec![],
            audio_packets: vec![],
            video_time_base: (1, 60),
            audio_time_base: (1, 48_000),
        }
    }

    // ── capacity clamping ─────────────────────────────────────────────────────

    #[test]
    fn new_clamps_below_min() {
        // The simplest observable check: push more than MIN segments and the
        // oldest should be evicted (capacity == MIN, not 1).
        let mut rb = RingBuffer::new(0);
        for i in 0..MIN_BUFFER_LENGTH_SECS + 1 {
            rb.push(make_segment(i as u64));
        }
        assert_eq!(rb.len(), MIN_BUFFER_LENGTH_SECS as usize);
        assert_eq!(rb.segments().front().unwrap().index, 1);
    }

    #[test]
    fn new_clamps_above_max() {
        let mut rb = RingBuffer::new(u32::MAX);
        for i in 0..MAX_BUFFER_LENGTH_SECS + 1 {
            rb.push(make_segment(i as u64));
        }
        assert_eq!(rb.len(), MAX_BUFFER_LENGTH_SECS as usize);
        assert_eq!(rb.segments().front().unwrap().index, 1);
    }

    #[test]
    fn new_valid_capacity() {
        let cap = 30u32;
        let mut rb = RingBuffer::new(cap);
        for i in 0..cap + 1 {
            rb.push(make_segment(i as u64));
        }
        assert_eq!(rb.len(), cap as usize);
        assert_eq!(rb.segments().front().unwrap().index, 1);
    }

    // ── push / eviction ───────────────────────────────────────────────────────

    #[test]
    fn push_does_not_exceed_capacity() {
        let cap = 10u32;
        let mut rb = RingBuffer::new(cap);
        for i in 0..cap * 2 {
            rb.push(make_segment(i as u64));
        }
        assert_eq!(rb.len(), cap as usize);
    }

    #[test]
    fn push_evicts_oldest_segment() {
        let mut rb = RingBuffer::new(MIN_BUFFER_LENGTH_SECS);
        for i in 0..MIN_BUFFER_LENGTH_SECS + 3 {
            rb.push(make_segment(i as u64));
        }
        // The first 3 segments (indices 0, 1, 2) should have been evicted.
        assert_eq!(rb.segments().front().unwrap().index, 3);
        assert_eq!(
            rb.segments().back().unwrap().index,
            MIN_BUFFER_LENGTH_SECS as u64 + 2
        );
    }

    #[test]
    fn push_into_empty_buffer() {
        let mut rb = RingBuffer::new(10);
        assert!(rb.is_empty());
        rb.push(make_segment(0));
        assert_eq!(rb.len(), 1);
        assert!(!rb.is_empty());
    }

    // ── take_all ──────────────────────────────────────────────────────────────

    #[test]
    fn take_all_returns_chronological_order() {
        let mut rb = RingBuffer::new(10);
        for i in 0..5u64 {
            rb.push(make_segment(i));
        }
        let drained = rb.take_all();
        assert_eq!(drained.len(), 5);
        for (expected, seg) in drained.iter().enumerate() {
            assert_eq!(seg.index, expected as u64);
        }
    }

    #[test]
    fn take_all_empties_buffer() {
        let mut rb = RingBuffer::new(10);
        rb.push(make_segment(0));
        rb.push(make_segment(1));
        let _ = rb.take_all();
        assert!(rb.is_empty());
    }

    #[test]
    fn take_all_on_empty_returns_empty_vec() {
        let mut rb = RingBuffer::new(10);
        let drained = rb.take_all();
        assert!(drained.is_empty());
    }

    // ── segments view ─────────────────────────────────────────────────────────

    #[test]
    fn segments_does_not_remove_items() {
        let mut rb = RingBuffer::new(10);
        rb.push(make_segment(0));
        rb.push(make_segment(1));
        let _ = rb.segments();
        assert_eq!(rb.len(), 2);
    }

    // ── clear ─────────────────────────────────────────────────────────────────

    #[test]
    fn clear_empties_buffer() {
        let mut rb = RingBuffer::new(10);
        for i in 0..5 {
            rb.push(make_segment(i));
        }
        rb.clear();
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
    }

    #[test]
    fn clear_then_push_works() {
        let mut rb = RingBuffer::new(10);
        rb.push(make_segment(0));
        rb.clear();
        rb.push(make_segment(1));
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.segments().front().unwrap().index, 1);
    }

    // ── resize ────────────────────────────────────────────────────────────────

    #[test]
    fn resize_smaller_evicts_oldest() {
        let mut rb = RingBuffer::new(10);
        for i in 0..10u64 {
            rb.push(make_segment(i));
        }
        rb.resize(7);
        assert_eq!(rb.len(), 7);
        // Segments 0-2 should be gone; segment 3 is now the oldest.
        assert_eq!(rb.segments().front().unwrap().index, 3);
    }

    #[test]
    fn resize_larger_does_not_add_segments() {
        let mut rb = RingBuffer::new(10);
        for i in 0..5u64 {
            rb.push(make_segment(i));
        }
        rb.resize(20);
        assert_eq!(rb.len(), 5);
    }

    #[test]
    fn resize_to_same_capacity_is_noop() {
        let mut rb = RingBuffer::new(10);
        for i in 0..10u64 {
            rb.push(make_segment(i));
        }
        rb.resize(10);
        assert_eq!(rb.len(), 10);
    }

    #[test]
    fn resize_clamps_below_min() {
        let mut rb = RingBuffer::new(10);
        for i in 0..10u64 {
            rb.push(make_segment(i));
        }
        rb.resize(0);
        // Should be clamped to MIN, not 0.
        assert_eq!(rb.len(), MIN_BUFFER_LENGTH_SECS as usize);
    }

    #[test]
    fn resize_clamps_above_max() {
        let mut rb = RingBuffer::new(10);
        for i in 0..10u64 {
            rb.push(make_segment(i));
        }
        rb.resize(u32::MAX);
        // Capacity grows, but existing segments are kept.
        assert_eq!(rb.len(), 10);
    }

    // ── codec params ──────────────────────────────────────────────────────────

    #[test]
    fn codec_params_start_as_none() {
        let rb = RingBuffer::new(10);
        assert!(rb.video_params.is_none());
        assert!(rb.audio_params.is_none());
    }

    #[test]
    fn codec_params_can_be_set() {
        let mut rb = RingBuffer::new(10);
        rb.video_params = Some(VideoCodecParams {
            extradata: vec![0x01, 0x02],
            width: 1920,
            height: 1080,
            fps: 60,
            time_base: (1, 60),
        });
        rb.audio_params = Some(AudioCodecParams {
            extradata: vec![0x03],
            sample_rate: 48_000,
            channels: 2,
            time_base: (1, 48_000),
        });
        assert!(rb.video_params.is_some());
        assert!(rb.audio_params.is_some());
    }
}
