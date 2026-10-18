//! Offline byte BPE. Unsupported normalization and pretokenization fail closed.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use unicode_general_category::{GeneralCategory as Cat, get_general_category};

const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const CL100K: &str = r"(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}{1,3}| ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+";
const O200K: &str = r"[^\r\n\p{L}\p{N}]?[\p{Lu}\p{Lt}\p{Lm}\p{Lo}\p{M}]*[\p{Ll}\p{Lm}\p{Lo}\p{M}]+(?i:'s|'t|'re|'ve|'m|'ll|'d)?|[^\r\n\p{L}\p{N}]?[\p{Lu}\p{Lt}\p{Lm}\p{Lo}\p{M}]+[\p{Ll}\p{Lm}\p{Lo}\p{M}]*(?i:'s|'t|'re|'ve|'m|'ll|'d)?|\p{N}{1,3}| ?[^\s\p{L}\p{N}]+[\r\n/]*|\s*[\r\n]+|\s+(?!\S)|\s+";
const GPT2: &str = r"'s|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+(?!\S)|\s+";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pre {
    Cl100k,
    O200k,
    Gpt2,
}
impl Pre {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "cl100k" => Ok(Self::Cl100k),
            "o200k" => Ok(Self::O200k),
            "gpt2" => Ok(Self::Gpt2),
            _ => Err("Unsupported pretokenizer: use cl100k, o200k or gpt2".into()),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Cl100k => "cl100k",
            Self::O200k => "o200k",
            Self::Gpt2 => "gpt2",
        }
    }
}

enum Rules {
    Ranks(HashMap<Vec<u8>, u32>),
    Pairs(HashMap<(Vec<u8>, Vec<u8>), u32>),
}

pub(super) struct Tokenizer {
    pub sha256: String,
    pub kind: String,
    pre: Pre,
    rules: Rules,
    specials: Vec<String>,
}

/// An explicitly loaded local tokenizer. Counting must run on the shadow worker
/// or another blocking/offline task, never inside the forwarding stream.
pub struct LocalTokenizer {
    inner: Tokenizer,
}

impl LocalTokenizer {
    pub fn load(path: &str, mode: Option<&str>) -> Result<Self, String> {
        Ok(Self {
            inner: Tokenizer::load(path, mode)?,
        })
    }
    pub fn count(&self, text: &str) -> Result<u64, String> {
        self.inner.count(text)
    }
    pub fn sha256(&self) -> &str {
        &self.inner.sha256
    }

    #[cfg(test)]
    pub(super) fn byte_fixture() -> Self {
        Self {
            inner: Tokenizer {
                sha256: "a".repeat(64),
                kind: "fixture".into(),
                pre: Pre::Cl100k,
                rules: Rules::Ranks(
                    (0u8..=255)
                        .map(|byte| (vec![byte], u32::from(byte)))
                        .collect(),
                ),
                specials: Vec::new(),
            },
        }
    }
}

pub(super) fn read_bounded(path: &str, limit: u64) -> Result<Vec<u8>, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("Cannot read local file: {error}"))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(format!(
            "Expected a regular local file of at most {limit} bytes"
        ));
    }
    // Limit again while reading so replacement/growth between stat and read does
    // not turn a user import into an unbounded allocation.
    use std::io::Read;
    let file = fs::File::open(path).map_err(|error| format!("Cannot open local file: {error}"))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read local file: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err("Local file exceeded the size limit".into());
    }
    Ok(bytes)
}

impl Tokenizer {
    pub fn load(path: &str, mode: Option<&str>) -> Result<Self, String> {
        let bytes = read_bounded(path, MAX_FILE_BYTES)?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let text = std::str::from_utf8(&bytes).map_err(|_| "Tokenizer file is not valid UTF-8")?;
        let mut tokenizer = if text.trim_start().starts_with('{') {
            Self::load_hf(text, mode)?
        } else {
            let filename = Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            let inferred = match filename {
                "cl100k_base.tiktoken" => Some("cl100k"),
                "o200k_base.tiktoken" => Some("o200k"),
                "r50k_base.tiktoken" | "p50k_base.tiktoken" => Some("gpt2"),
                _ => None,
            };
            let pre = Pre::parse(
                mode.or(inferred)
                    .ok_or("An unknown .tiktoken filename requires an explicit pretokenizer")?,
            )?;
            let mut ranks = HashMap::new();
            let mut seen_ranks = HashSet::new();
            for (index, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let columns: Vec<_> = line.split_ascii_whitespace().collect();
                if columns.len() != 2 {
                    return Err(format!("Invalid tiktoken row {}", index + 1));
                }
                let token = decode_base64(columns[0])
                    .map_err(|_| format!("Invalid base64 at row {}", index + 1))?;
                let rank: u32 = columns[1]
                    .parse()
                    .map_err(|_| format!("Invalid rank at row {}", index + 1))?;
                if token.is_empty()
                    || ranks.insert(token, rank).is_some()
                    || !seen_ranks.insert(rank)
                {
                    return Err(format!(
                        "Empty/duplicate token or rank at row {}",
                        index + 1
                    ));
                }
            }
            for byte in 0u8..=255 {
                if !ranks.contains_key(&vec![byte]) {
                    return Err(
                        "Incomplete byte BPE vocabulary: all 256 byte tokens are required".into(),
                    );
                }
            }
            Self {
                sha256: String::new(),
                kind: "tiktoken".into(),
                pre,
                rules: Rules::Ranks(ranks),
                specials: Vec::new(),
            }
        };
        tokenizer.sha256 = sha256;
        Ok(tokenizer)
    }

    fn load_hf(text: &str, mode: Option<&str>) -> Result<Self, String> {
        let doc: Value = serde_json::from_str(text)
            .map_err(|error| format!("Invalid tokenizer JSON: {error}"))?;
        let model = &doc["model"];
        if model["type"].as_str() != Some("BPE") {
            return Err("Only byte-level BPE tokenizer.json is supported".into());
        }
        if !doc["normalizer"].is_null() {
            return Err("Unsupported tokenizer normalizer; reference generation skipped".into());
        }
        for key in ["continuing_subword_prefix", "end_of_word_suffix"] {
            if !model[key].is_null() && model[key].as_str() != Some("") {
                return Err(format!("Unsupported BPE option: {key}"));
            }
        }
        for key in ["byte_fallback", "fuse_unk", "ignore_merges"] {
            if model[key].as_bool() == Some(true) {
                return Err(format!("Unsupported BPE option: {key}"));
            }
        }
        if model["dropout"].as_f64().is_some_and(|value| value != 0.0) {
            return Err("Nondeterministic BPE dropout is unsupported".into());
        }
        let pre = detect_hf_pre(&doc["pre_tokenizer"])?;
        if let Some(mode) = mode {
            if Pre::parse(mode)? != pre {
                return Err("Requested pretokenizer disagrees with tokenizer.json".into());
            }
        }
        let vocab = model["vocab"]
            .as_object()
            .ok_or("Missing BPE model.vocab")?;
        let mut byte_vocab = HashSet::new();
        for token in vocab.keys() {
            if let Ok(bytes) = hf_bytes(token) {
                byte_vocab.insert(bytes);
            }
        }
        for byte in 0u8..=255 {
            if !byte_vocab.contains(&vec![byte]) {
                return Err("Incomplete byte-level BPE vocabulary".into());
            }
        }
        let merges = model["merges"]
            .as_array()
            .ok_or("Missing BPE model.merges")?;
        let mut pairs = HashMap::new();
        for (index, merge) in merges.iter().enumerate() {
            let (left, right) = if let Some(pair) = merge.as_array() {
                if pair.len() != 2 {
                    return Err("Invalid BPE merge pair".into());
                }
                (
                    pair[0].as_str().ok_or("Invalid merge token")?,
                    pair[1].as_str().ok_or("Invalid merge token")?,
                )
            } else if let Some(pair) = merge.as_str() {
                let columns: Vec<_> = pair.split(' ').collect();
                if columns.len() != 2 {
                    return Err("Invalid BPE merge pair".into());
                }
                (columns[0], columns[1])
            } else {
                return Err("Invalid BPE merge pair".into());
            };
            let left = hf_bytes(left)?;
            let right = hf_bytes(right)?;
            let mut joined = left.clone();
            joined.extend_from_slice(&right);
            if left.is_empty()
                || right.is_empty()
                || !byte_vocab.contains(&left)
                || !byte_vocab.contains(&right)
                || !byte_vocab.contains(&joined)
            {
                return Err("BPE merge references a missing/empty byte token".into());
            }
            if pairs.insert((left, right), index as u32).is_some() {
                return Err("Duplicate BPE merge pair".into());
            }
        }
        let specials = doc["added_tokens"]
            .as_array()
            .map(|tokens| {
                tokens
                    .iter()
                    .filter_map(|token| token["content"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            sha256: String::new(),
            kind: "hf_bpe".into(),
            pre,
            rules: Rules::Pairs(pairs),
            specials,
        })
    }

    pub fn pretokenizer(&self) -> &str {
        self.pre.name()
    }
    pub fn count(&self, text: &str) -> Result<u64, String> {
        if self
            .specials
            .iter()
            .any(|special| !special.is_empty() && text.contains(special))
        {
            return Err(
                "Corpus collides with an added token; cannot reproduce added-token matching safely"
                    .into(),
            );
        }
        let mut count = 0u64;
        for piece in pretokenize(text, self.pre) {
            let bytes = piece.as_bytes();
            if bytes.len() > 8192 {
                return Err("Pretokenized piece exceeds offline calculation limit".into());
            }
            count += bpe_count(bytes, &self.rules) as u64;
        }
        Ok(count)
    }
}

fn bpe_count(piece: &[u8], rules: &Rules) -> usize {
    if piece.is_empty() {
        return 0;
    }
    if let Rules::Ranks(ranks) = rules {
        if ranks.contains_key(piece) {
            return 1;
        }
    }
    let mut parts: Vec<Vec<u8>> = piece.iter().map(|byte| vec![*byte]).collect();
    while parts.len() > 1 {
        let mut best: Option<(u32, usize)> = None;
        for index in 0..parts.len() - 1 {
            let rank = match rules {
                Rules::Ranks(ranks) => {
                    let mut joined = parts[index].clone();
                    joined.extend_from_slice(&parts[index + 1]);
                    ranks.get(&joined).copied()
                }
                Rules::Pairs(pairs) => pairs
                    .get(&(parts[index].clone(), parts[index + 1].clone()))
                    .copied(),
            };
            if let Some(rank) = rank {
                if best.is_none_or(|(current, _)| rank < current) {
                    best = Some((rank, index));
                }
            }
        }
        let Some((_, index)) = best else {
            break;
        };
        let right = parts.remove(index + 1);
        parts[index].extend(right);
    }
    parts.len()
}

fn detect_hf_pre(node: &Value) -> Result<Pre, String> {
    fn byte_level(node: &Value, expected_regex: bool) -> bool {
        node["type"].as_str() == Some("ByteLevel")
            && node["add_prefix_space"].as_bool() == Some(false)
            && node["use_regex"].as_bool().unwrap_or(true) == expected_regex
    }
    if byte_level(node, true) {
        return Ok(Pre::Gpt2);
    }
    if node["type"].as_str() == Some("Sequence") {
        let nodes = node["pretokenizers"]
            .as_array()
            .ok_or("Missing pretokenizer sequence")?;
        if nodes.len() == 2
            && nodes[0]["type"].as_str() == Some("Split")
            && nodes[0]["behavior"].as_str() == Some("Isolated")
            && nodes[0]["invert"].as_bool().unwrap_or(false) == false
            && byte_level(&nodes[1], false)
        {
            let pattern = nodes[0]["pattern"]["Regex"]
                .as_str()
                .ok_or("Only recognized Regex splitters are supported")?;
            return match pattern {
                CL100K => Ok(Pre::Cl100k),
                O200K => Ok(Pre::O200k),
                GPT2 => Ok(Pre::Gpt2),
                _ => Err("Unrecognized pretokenizer regex; reference generation skipped".into()),
            };
        }
    }
    Err("Unsupported pretokenizer (requires exact GPT-2/cl100k/o200k byte-level rules, no prefix space)".into())
}

fn hf_bytes(token: &str) -> Result<Vec<u8>, String> {
    // The reversible GPT-2 bytes-to-Unicode alphabet, not UTF-8 decoding.
    static ALPHABET: std::sync::LazyLock<HashMap<char, u8>> = std::sync::LazyLock::new(|| {
        let mut direct = Vec::new();
        direct.extend(33u16..=126);
        direct.extend(161u16..=172);
        direct.extend(174u16..=255);
        let mut alphabet = HashMap::new();
        for byte in &direct {
            alphabet.insert(char::from_u32(u32::from(*byte)).unwrap(), *byte as u8);
        }
        let mut next = 256u32;
        for byte in 0u16..=255 {
            if !direct.contains(&byte) {
                alphabet.insert(char::from_u32(next).unwrap(), byte as u8);
                next += 1;
            }
        }
        alphabet
    });
    token
        .chars()
        .map(|ch| {
            ALPHABET
                .get(&ch)
                .copied()
                .ok_or_else(|| "Non-byte-level merge token is unsupported".to_string())
        })
        .collect()
}

fn decode_base64(value: &str) -> Result<Vec<u8>, ()> {
    if value.is_empty() || value.len() % 4 != 0 {
        return Err(());
    }
    let mut output = Vec::new();
    let groups = value.as_bytes().chunks_exact(4);
    let total = value.len() / 4;
    for (index, block) in groups.enumerate() {
        let mut bits = 0u32;
        let mut padding = 0;
        for (offset, ch) in block.iter().copied().enumerate() {
            let six = match ch {
                b'A'..=b'Z' => ch - b'A',
                b'a'..=b'z' => ch - b'a' + 26,
                b'0'..=b'9' => ch - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' if index + 1 == total && offset >= 2 => {
                    padding += 1;
                    0
                }
                _ => return Err(()),
            };
            if padding > 0 && ch != b'=' {
                return Err(());
            }
            bits = (bits << 6) | u32::from(six);
        }
        if padding > 2 || (padding == 2 && bits & 0xffff != 0) || (padding == 1 && bits & 0xff != 0)
        {
            return Err(());
        }
        output.push((bits >> 16) as u8);
        if padding < 2 {
            output.push((bits >> 8) as u8);
        }
        if padding == 0 {
            output.push(bits as u8);
        }
    }
    Ok(output)
}

fn letter(ch: char) -> bool {
    matches!(
        get_general_category(ch),
        Cat::UppercaseLetter
            | Cat::LowercaseLetter
            | Cat::TitlecaseLetter
            | Cat::ModifierLetter
            | Cat::OtherLetter
    )
}
fn number(ch: char) -> bool {
    matches!(
        get_general_category(ch),
        Cat::DecimalNumber | Cat::LetterNumber | Cat::OtherNumber
    )
}
fn upperish(ch: char) -> bool {
    matches!(
        get_general_category(ch),
        Cat::UppercaseLetter
            | Cat::TitlecaseLetter
            | Cat::ModifierLetter
            | Cat::OtherLetter
            | Cat::NonspacingMark
            | Cat::SpacingMark
            | Cat::EnclosingMark
    )
}
fn lowerish(ch: char) -> bool {
    matches!(
        get_general_category(ch),
        Cat::LowercaseLetter
            | Cat::ModifierLetter
            | Cat::OtherLetter
            | Cat::NonspacingMark
            | Cat::SpacingMark
            | Cat::EnclosingMark
    )
}
fn punctuation(ch: char) -> bool {
    !ch.is_whitespace() && !letter(ch) && !number(ch)
}
fn contraction(chars: &[char], at: usize, insensitive: bool) -> usize {
    for suffix in ["'re", "'ve", "'ll", "'s", "'t", "'m", "'d"] {
        let suffix: Vec<_> = suffix.chars().collect();
        if at + suffix.len() <= chars.len()
            && suffix.iter().enumerate().all(|(offset, expected)| {
                let actual = chars[at + offset];
                if insensitive {
                    actual.to_ascii_lowercase() == *expected
                        || (actual == '\u{017f}' && *expected == 's')
                } else {
                    actual == *expected
                }
            })
        {
            return suffix.len();
        }
    }
    0
}

fn pretokenize(text: &str, pre: Pre) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut at = 0;
    let mut pieces = Vec::new();
    while at < n {
        let start = at;
        let ch = chars[at];
        let mut end = at;
        if pre != Pre::O200k {
            end += contraction(&chars, at, pre != Pre::Gpt2);
        }
        if end == at && pre == Pre::O200k {
            for require_lower in [true, false] {
                let prefix = ch != '\r' && ch != '\n' && !letter(ch) && !number(ch);
                // The optional prefix and the letter classes overlap on marks.
                // Backtrack the optional prefix before trying the next regex
                // alternative, preserving leftmost-alternative precedence.
                for begin in (at..=at + usize::from(prefix)).rev() {
                    let mut upper_end = begin;
                    while upper_end < n && upperish(chars[upper_end]) {
                        upper_end += 1;
                    }
                    // Lo/Lm/M also overlap in the upper* and lower+ classes.
                    let mut split = upper_end;
                    loop {
                        let mut lower_end = split;
                        while lower_end < n && lowerish(chars[lower_end]) {
                            lower_end += 1;
                        }
                        let valid = if require_lower {
                            lower_end > split
                        } else {
                            split > begin
                        };
                        if valid {
                            end = lower_end + contraction(&chars, lower_end, true);
                            break;
                        }
                        if !require_lower || split == begin {
                            break;
                        }
                        split -= 1;
                    }
                    if end > at {
                        break;
                    }
                }
                if end > at {
                    break;
                }
            }
        }
        if end == at && pre == Pre::Cl100k {
            let mut begin = at;
            if ch != '\r'
                && ch != '\n'
                && !letter(ch)
                && !number(ch)
                && at + 1 < n
                && letter(chars[at + 1])
            {
                begin += 1;
            }
            if begin < n && letter(chars[begin]) {
                end = begin;
                while end < n && letter(chars[end]) {
                    end += 1;
                }
            }
        }
        if end == at && pre == Pre::Gpt2 {
            let begin = at + usize::from(ch == ' ' && at + 1 < n);
            if letter(chars[begin]) || number(chars[begin]) || punctuation(chars[begin]) {
                let category = if letter(chars[begin]) {
                    0
                } else if number(chars[begin]) {
                    1
                } else {
                    2
                };
                end = begin;
                while end < n
                    && match category {
                        0 => letter(chars[end]),
                        1 => number(chars[end]),
                        _ => punctuation(chars[end]),
                    }
                {
                    end += 1;
                }
            }
        }
        if end == at && pre != Pre::Gpt2 && number(ch) {
            while end < n && number(chars[end]) && end - at < 3 {
                end += 1;
            }
        }
        if end == at && pre != Pre::Gpt2 {
            let begin = at + usize::from(ch == ' ' && at + 1 < n && punctuation(chars[at + 1]));
            if begin < n && punctuation(chars[begin]) {
                end = begin;
                while end < n && punctuation(chars[end]) {
                    end += 1;
                }
                while end < n
                    && (matches!(chars[end], '\r' | '\n')
                        || (pre == Pre::O200k && chars[end] == '/'))
                {
                    end += 1;
                }
            }
        }
        if end == at && ch.is_whitespace() {
            let mut whitespace_end = at;
            while whitespace_end < n && chars[whitespace_end].is_whitespace() {
                whitespace_end += 1;
            }
            if pre != Pre::Gpt2 {
                // Greedy \s*[\r\n]+ ends at the last newline in the run.
                for index in at..whitespace_end {
                    if matches!(chars[index], '\r' | '\n') {
                        end = index + 1;
                    }
                }
            }
            if end == at {
                end = if whitespace_end == n || whitespace_end - at == 1 {
                    whitespace_end
                } else {
                    whitespace_end - 1
                };
            }
        }
        if end == at {
            end += 1;
        }
        pieces.push(chars[start..end].iter().collect());
        at = end;
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_audit_bpe_merge_priority_and_pair_identity() {
        let mut ranks: HashMap<Vec<u8>, u32> = [
            (b"ab".to_vec(), 1),
            (b"bc".to_vec(), 0),
            (b"cd".to_vec(), 2),
        ]
        .into();
        assert_eq!(bpe_count(b"abcd", &Rules::Ranks(ranks.clone())), 3);
        ranks.insert(b"bc".to_vec(), 5);
        ranks.insert(b"ab".to_vec(), 0);
        assert_eq!(bpe_count(b"abcd", &Rules::Ranks(ranks)), 2);
        // A merged token's bytes alone are not an HF pair rule.
        let pairs = [
            ((b"a".to_vec(), b"b".to_vec()), 0),
            ((b"ab".to_vec(), b"c".to_vec()), 1),
        ]
        .into();
        assert_eq!(bpe_count(b"abc", &Rules::Pairs(pairs)), 1);
    }
    #[test]
    fn model_audit_pretokenizer_unicode_and_boundaries() {
        for pre in [Pre::Cl100k, Pre::O200k, Pre::Gpt2] {
            for text in [
                "HelloWorld ABCdef",
                "人工智能 AI 2026 年",
                "👨‍👩‍👧‍👦",
                "a\n \n b",
                "  leading trailing  ",
                "ภาษาไทย",
                "it's I'M",
            ] {
                let pieces = pretokenize(text, pre);
                assert_eq!(pieces.concat(), text);
                assert!(pieces.iter().all(|piece| !piece.is_empty()));
            }
        }
        assert_eq!(pretokenize("1234567", Pre::Cl100k), vec!["123", "456", "7"]);
        assert_eq!(pretokenize("1234567", Pre::Gpt2), vec!["1234567"]);
        assert_eq!(
            pretokenize("HelloWorld", Pre::O200k),
            vec!["Hello", "World"]
        );
        assert_eq!(pretokenize("汉字", Pre::O200k), vec!["汉字"]);
        assert_eq!(pretokenize("\u{0301}A", Pre::O200k), vec!["\u{0301}", "A"]);
        assert_eq!(
            pretokenize("'\u{017f}abc", Pre::Cl100k),
            vec!["'\u{017f}", "abc"]
        );
    }
    #[test]
    fn model_audit_pretokenizer_independent_unicode_regex_oracle() {
        // Fixtures were independently evaluated with Node Unicode-property
        // regexes; \s was replaced with White_Space and (?i) contractions with
        // explicit simple-casefold alternatives, including long s (U+017F).
        // This checks actual segmentation, beyond lossless concatenation.
        let fixtures: Vec<Value> =
            serde_json::from_str(include_str!("pretokenizer-oracle.json")).unwrap();
        assert_eq!(fixtures.len(), 99);
        for fixture in fixtures {
            let mode = Pre::parse(fixture["mode"].as_str().unwrap()).unwrap();
            let text = fixture["text"].as_str().unwrap();
            let expected: Vec<_> = fixture["pieces"]
                .as_array()
                .unwrap()
                .iter()
                .map(|piece| piece.as_str().unwrap())
                .collect();
            assert_eq!(pretokenize(text, mode), expected, "{mode:?}: {text:?}");
        }
    }
    #[test]
    fn model_audit_honest_token_split_over_multiple_sse_frames_is_not_proof() {
        use crate::model_audit::{AxisResult, Observation, StreamAccumulator, audit};
        let mut ranks: HashMap<Vec<u8>, u32> = (0u8..=255)
            .map(|byte| (vec![byte], u32::from(byte)))
            .collect();
        ranks.insert(b"aa".to_vec(), 256);
        ranks.insert(b"aaaa".to_vec(), 257);
        ranks.insert(b"aaaaaaaa".to_vec(), 258);
        let tokenizer = Tokenizer {
            sha256: String::new(),
            kind: "fixture".into(),
            pre: Pre::Cl100k,
            rules: Rules::Ranks(ranks),
            specials: Vec::new(),
        };
        assert_eq!(tokenizer.count("aaaaaaaa").unwrap(), 1);
        let mut stream = StreamAccumulator::default();
        for index in 0..8 {
            stream.observe("a", 10 + index * 10);
        }
        let record = audit(Observation {
            requested_model: Some("honest-fixture".into()),
            upstream_model: Some("honest-fixture".into()),
            input_tokens: Some(10),
            output_tokens: Some(1),
            total_tokens: Some(11),
            reasoning_tokens: Some(0),
            max_tokens: Some(8),
            visible_output_bytes: Some(8),
            tool_payload_bytes: Some(0),
            complete: true,
            text_only: true,
            stream: Some(stream.finish()),
            ..Default::default()
        });
        assert!(record.flags.is_empty());
        let billing = record.stream_billing.unwrap();
        assert_eq!(billing.status, AxisResult::Contradiction);
        assert!(
            billing
                .gaps
                .contains(&"scv.token_boundaries_unverified".into())
        );
    }
    #[test]
    fn model_audit_base64_is_strict() {
        assert_eq!(decode_base64("YQ==").unwrap(), b"a");
        assert_eq!(decode_base64("YWI=").unwrap(), b"ab");
        assert_eq!(decode_base64("YWJj").unwrap(), b"abc");
        for bad in ["YQ=Z", "=Q==", "YQ", "Y===", "YR==", "YWJ="] {
            assert!(decode_base64(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn model_audit_hf_unsupported_rules_fail_closed() {
        assert!(Tokenizer::load_hf(r#"{"model":{"type":"Unigram"}}"#, None).is_err());
        assert!(
            Tokenizer::load_hf(
                r#"{"model":{"type":"BPE"},"normalizer":{"type":"NFC"}}"#,
                None
            )
            .is_err()
        );
        assert!(
            detect_hf_pre(&serde_json::json!({"type":"ByteLevel","add_prefix_space":true}))
                .is_err()
        );
        assert_eq!(
            detect_hf_pre(&serde_json::json!({"type":"ByteLevel","add_prefix_space":false}))
                .unwrap(),
            Pre::Gpt2
        );
    }
}
