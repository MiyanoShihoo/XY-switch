use serde::de::{Deserialize, Deserializer, Error, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a JSON value without duplicate object keys")
            }
            fn visit_bool<E: Error>(self, value: bool) -> Result<Self::Value, E> { Ok(UniqueValue(Value::Bool(value))) }
            fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> { Ok(UniqueValue(value.into())) }
            fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> { Ok(UniqueValue(value.into())) }
            fn visit_f64<E: Error>(self, value: f64) -> Result<Self::Value, E> {
                Number::from_f64(value).map(|value| UniqueValue(Value::Number(value))).ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> { Ok(UniqueValue(value.into())) }
            fn visit_string<E: Error>(self, value: String) -> Result<Self::Value, E> { Ok(UniqueValue(Value::String(value))) }
            fn visit_unit<E: Error>(self) -> Result<Self::Value, E> { Ok(UniqueValue(Value::Null)) }
            fn visit_none<E: Error>(self) -> Result<Self::Value, E> { Ok(UniqueValue(Value::Null)) }
            fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueValue(value)) = sequence.next_element()? { values.push(value); }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Self::Value, A::Error> {
                let mut object = Map::new();
                while let Some((key, UniqueValue(value))) = entries.next_entry::<String, UniqueValue>()? {
                    if object.insert(key, value).is_some() { return Err(A::Error::custom("duplicate configuration key")); }
                }
                Ok(UniqueValue(Value::Object(object)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

pub(crate) fn parse(text: &str) -> Result<Value, String> {
    if text.len() > 4 * 1024 * 1024 { return Err("客户端配置超过大小限制".into()); }
    let input = text.strip_prefix('\u{feff}').unwrap_or(text).as_bytes();
    let mut cleaned = input.to_vec();
    let mut index = 0;
    let mut quoted = false;
    let mut escaped = false;
    while index < input.len() {
        let byte = input[index];
        if quoted {
            if escaped { escaped = false; }
            else if byte == b'\\' { escaped = true; }
            else if byte == b'"' { quoted = false; }
            index += 1;
            continue;
        }
        if byte == b'"' { quoted = true; index += 1; continue; }
        if byte == b'/' && input.get(index + 1) == Some(&b'/') {
            while index < input.len() && !matches!(input[index], b'\r' | b'\n') { cleaned[index] = b' '; index += 1; }
        } else if byte == b'/' && input.get(index + 1) == Some(&b'*') {
            cleaned[index] = b' '; cleaned[index + 1] = b' '; index += 2;
            let mut closed = false;
            while index < input.len() {
                if input[index] == b'*' && input.get(index + 1) == Some(&b'/') {
                    cleaned[index] = b' '; cleaned[index + 1] = b' '; index += 2; closed = true; break;
                }
                if !matches!(input[index], b'\r' | b'\n') { cleaned[index] = b' '; }
                index += 1;
            }
            if !closed { return Err("客户端配置包含未结束的注释".into()); }
        } else { index += 1; }
    }
    if quoted { return Err("客户端配置包含未结束的字符串".into()); }
    index = 0;
    quoted = false;
    escaped = false;
    while index < cleaned.len() {
        let byte = cleaned[index];
        if quoted {
            if escaped { escaped = false; }
            else if byte == b'\\' { escaped = true; }
            else if byte == b'"' { quoted = false; }
        } else if byte == b'"' { quoted = true; }
        else if byte == b',' {
            let mut next = index + 1;
            while next < cleaned.len() && cleaned[next].is_ascii_whitespace() { next += 1; }
            if matches!(cleaned.get(next), Some(b'}' | b']')) { cleaned[index] = b' '; }
        }
        index += 1;
    }
    let mut deserializer = serde_json::Deserializer::from_slice(&cleaned);
    let UniqueValue(value) = UniqueValue::deserialize(&mut deserializer).map_err(|_| "客户端 JSON/JSONC 配置无效或存在重复字段".to_string())?;
    deserializer.end().map_err(|_| "客户端配置包含多余内容".to_string())?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_urls_and_comment_like_text_and_unicode() {
        let value = parse("\u{feff}{ // note\n \"url\": \"https://host/a/*b*/\", /*你好*/ \"text\": \"a,} and \\\"quote\\\"\", \"values\": [1, 2,], }").unwrap();
        assert_eq!(value["url"], "https://host/a/*b*/");
        assert_eq!(value["text"], "a,} and \"quote\"");
        assert_eq!(value["values"], serde_json::json!([1, 2]));
    }
    #[test]
    fn rejects_duplicate_keys_and_never_echoes_values() {
        for text in [r#"{"provider":{},"provider":{"apiKey":"synthetic-secret"}}"#, r#"{"nested":{"key":1,"key":2}}"#, "{ /* unclosed", "{}{}", "{\"key\": }"] {
            let error = parse(text).unwrap_err();
            assert!(!error.contains("synthetic-secret"));
        }
    }
}
