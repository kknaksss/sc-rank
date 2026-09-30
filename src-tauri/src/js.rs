//! PoC(JavaScript) 문자열 의미를 그대로 옮기기 위한 도우미.
//! 판정이 JS 의 `\s`·`trim()`·`String()`·`length`·`encodeURIComponent` 에 기대므로 Rust 기본값을 쓰지 않는다.

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde_json::Value;

/// JS 정규식 `\s` · `String.prototype.trim` 이 공백으로 보는 문자.
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `value.trim()`
pub fn trim(value: &str) -> &str {
    value.trim_matches(is_space)
}

/// `value.replace(/\s/g, '')`
pub fn strip_spaces(value: &str) -> String {
    value.chars().filter(|c| !is_space(*c)).collect()
}

/// `value.replace(/\s+/g, ' ')`
pub fn collapse_spaces(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_space = false;
    for c in value.chars() {
        if is_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// JS `string.length` (UTF-16 코드 단위 수).
pub fn length(value: &str) -> usize {
    value.encode_utf16().count()
}

/// JS 참/거짓 판정.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// JS `String(value)` — 값이 없으면(`undefined`) `"undefined"`.
pub fn to_js_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => number_to_js(n),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::Null => String::new(),
                other => to_js_string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

fn number_to_js(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    let f = n.as_f64().unwrap_or(f64::NAN);
    if f.fract() == 0.0 && f.abs() < 1e21 {
        format!("{f:.0}")
    } else {
        f.to_string()
    }
}

/// `encodeURIComponent` 가 그대로 두는 문자: `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// `encodeURIComponent(value)`
pub fn encode_uri_component(value: &str) -> String {
    utf8_percent_encode(value, URI_COMPONENT).to_string()
}

/// `new Date().toISOString()` — 밀리초 3자리, `Z`.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn js_whitespace_and_trim() {
        assert_eq!(trim("\u{3000} a b \u{FEFF}"), "a b");
        assert_eq!(strip_spaces("무 이\t성형\u{A0}외과"), "무이성형외과");
        assert_eq!(collapse_spaces("a \n\t b  c"), "a b c");
    }

    #[test]
    fn js_string_and_uri() {
        assert_eq!(to_js_string(None), "undefined");
        assert_eq!(to_js_string(Some(&json!(1.0))), "1");
        assert_eq!(
            encode_uri_component("강남 역!(a)"),
            "%EA%B0%95%EB%82%A8%20%EC%97%AD!(a)"
        );
        assert_eq!(length("😀가"), 3);
    }
}
