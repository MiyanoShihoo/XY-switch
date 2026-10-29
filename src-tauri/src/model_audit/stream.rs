use serde::{Deserialize, Serialize};

const MAX_SAMPLES: usize = 256;
const MIN_SAMPLES: u64 = 8;
const LENGTH_BUCKETS: usize = 1025;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StreamSummary {
    pub text_delta_count: u64,
    pub visible_output_chars: u64,
    pub max_digit_run: u64,
    pub max_han_run: u64,
    pub max_space_run: u64,
    pub max_han_run_bytes: u64,
    pub max_space_run_bytes: u64,
    pub median_delta_chars: f64,
    pub ttfb_ms: Option<u64>,
    pub digit_chars: u64,
    pub han_chars: u64,
    pub space_chars: u64,
    pub arrival_offsets_ms: Vec<u64>,
    pub batched: bool,
    pub minimum_samples_met: bool,
    pub samples_truncated: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisResult {
    Contradiction,
    Consistent,
    #[default]
    NotComparable,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AxisResults {
    pub digit: AxisResult,
    pub han: AxisResult,
    pub space: AxisResult,
}

/// Whole-output ratios are conditional on transport boundaries preserving
/// generated-token boundaries. Protocols permit splitting one token over
/// multiple text frames; a contradiction here is never an invariant flag.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StreamBilling {
    pub status: AxisResult,
    pub text_frames: u64,
    pub visible_tokens: Option<u64>,
    pub visible_bytes: Option<u64>,
    pub tokens_per_byte: Option<f64>,
    pub bytes_per_frame: Option<f64>,
    pub frames_per_token: Option<f64>,
    pub gaps: Vec<String>,
    pub limitations: Vec<String>,
}

/// Fixed histogram and bounded arrival offsets; response text is never kept.
#[derive(Debug)]
pub struct StreamAccumulator {
    summary: StreamSummary,
    lengths: [u64; LENGTH_BUCKETS],
    last_elapsed: Option<u64>,
    equal_arrivals: u64,
    clock_regressed: bool,
}

impl Default for StreamAccumulator {
    fn default() -> Self {
        Self {
            summary: StreamSummary::default(),
            lengths: [0; LENGTH_BUCKETS],
            last_elapsed: None,
            equal_arrivals: 0,
            clock_regressed: false,
        }
    }
}

fn is_han(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF |
        0x20000..=0x2FA1F | 0x30000..=0x323AF)
}

impl StreamAccumulator {
    pub fn observe(&mut self, text: &str, elapsed_ms: u64) {
        if text.is_empty() {
            return;
        }
        let mut chars = 0u64;
        let (mut digit_run, mut han_run, mut space_run) = (0u64, 0u64, 0u64);
        let (mut han_bytes, mut space_bytes) = (0u64, 0u64);
        for ch in text.chars() {
            chars = chars.saturating_add(1);
            let digit = ch.is_ascii_digit();
            let han = is_han(ch);
            let space = ch.is_whitespace();
            digit_run = if digit { digit_run + 1 } else { 0 };
            han_run = if han { han_run + 1 } else { 0 };
            space_run = if space { space_run + 1 } else { 0 };
            han_bytes = if han {
                han_bytes + ch.len_utf8() as u64
            } else {
                0
            };
            space_bytes = if space {
                space_bytes + ch.len_utf8() as u64
            } else {
                0
            };
            self.summary.max_digit_run = self.summary.max_digit_run.max(digit_run);
            self.summary.max_han_run = self.summary.max_han_run.max(han_run);
            self.summary.max_space_run = self.summary.max_space_run.max(space_run);
            self.summary.max_han_run_bytes = self.summary.max_han_run_bytes.max(han_bytes);
            self.summary.max_space_run_bytes = self.summary.max_space_run_bytes.max(space_bytes);
            self.summary.digit_chars = self.summary.digit_chars.saturating_add(u64::from(digit));
            self.summary.han_chars = self.summary.han_chars.saturating_add(u64::from(han));
            self.summary.space_chars = self.summary.space_chars.saturating_add(u64::from(space));
        }
        self.summary.text_delta_count = self.summary.text_delta_count.saturating_add(1);
        self.summary.visible_output_chars = self.summary.visible_output_chars.saturating_add(chars);
        self.summary.ttfb_ms.get_or_insert(elapsed_ms);
        if let Some(previous) = self.last_elapsed {
            self.equal_arrivals += u64::from(previous == elapsed_ms);
            self.clock_regressed |= elapsed_ms < previous;
        }
        self.last_elapsed = Some(elapsed_ms);
        if self.summary.arrival_offsets_ms.len() < MAX_SAMPLES {
            self.summary.arrival_offsets_ms.push(elapsed_ms);
        } else {
            self.summary.samples_truncated = true;
        }
        let bucket = chars.min((LENGTH_BUCKETS - 1) as u64) as usize;
        self.lengths[bucket] = self.lengths[bucket].saturating_add(1);
        if chars >= LENGTH_BUCKETS as u64 {
            self.summary.samples_truncated = true;
        }
    }

    pub fn finish(mut self) -> StreamSummary {
        let count = self.summary.text_delta_count;
        if count > 0 {
            let lower = (count - 1) / 2;
            let upper = count / 2;
            let mut seen = 0u64;
            let mut low_value = None;
            for (length, samples) in self.lengths.iter().enumerate() {
                seen = seen.saturating_add(*samples);
                if low_value.is_none() && seen > lower {
                    low_value = Some(length);
                }
                if seen > upper {
                    self.summary.median_delta_chars = (low_value.unwrap() + length) as f64 / 2.0;
                    break;
                }
            }
        }
        self.summary.minimum_samples_met = count >= MIN_SAMPLES;
        // Coarse clocks also trigger this conservative gate: losing a comparison
        // is preferable to claiming generated-token boundaries survived batching.
        self.summary.batched = self.summary.median_delta_chars > 16.0
            || self.equal_arrivals > 0
            || self.clock_regressed;
        self.summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_audit_stream_unicode_runs_and_median() {
        let mut stream = StreamAccumulator::default();
        stream.observe("", 1);
        stream.observe("汉字12  ", 10);
        stream.observe("34", 20);
        let result = stream.finish();
        assert_eq!(result.text_delta_count, 2);
        assert_eq!(result.visible_output_chars, 8);
        assert_eq!(result.max_digit_run, 2); // Never join runs across frame boundaries.
        assert_eq!(result.max_han_run, 2);
        assert_eq!(result.max_space_run, 2);
        assert_eq!(result.median_delta_chars, 4.0);
        assert_eq!(result.ttfb_ms, Some(10));
        assert!(!result.minimum_samples_met);
    }
    #[test]
    fn model_audit_batching_and_bounded_storage() {
        let mut stream = StreamAccumulator::default();
        for _ in 0..10_000 {
            stream.observe("1234", 1);
        }
        let result = stream.finish();
        assert!(result.batched && result.minimum_samples_met && result.samples_truncated);
        assert_eq!(result.arrival_offsets_ms.len(), MAX_SAMPLES);
    }
}
