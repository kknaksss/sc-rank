//! 엑셀 — PoC `server/workbook.mjs` (`makeWorkbook` · `labels`) 와 `index.mjs:45` 검증.

use chrono::{DateTime, FixedOffset, TimeZone};
use rust_xlsxwriter::{Color, DocProperties, Format, FormatAlign, Workbook, XlsxError};
use serde_json::Value;

use crate::js;

/// `index.mjs:45`
pub const ROWS_MESSAGE: &str = "저장할 결과는 1~50개여야 합니다.";
/// `index.mjs:51`
pub const WORKBOOK_MESSAGE: &str = "엑셀을 만들지 못했습니다. 결과를 다시 조회해 주세요.";

/// `workbook.mjs:2`
pub fn label(status: &str) -> Option<&'static str> {
    Some(match status {
        "found" => "노출",
        "not_found" => "미노출",
        "error" => "조회 실패",
        "cancelled" => "중단",
        "pending" => "대기",
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Blog,
    Place,
    /// PoC 의 세 번째 열 구성(파워링크). 화면은 부르지 않지만 `mode` 가 둘 다 아니면 PoC 도 이것을 쓴다.
    Other,
}

impl Mode {
    /// `index.mjs:47` — `mode === 'place'` / `mode === 'blog'`
    pub fn from_value(mode: Option<&Value>) -> Self {
        match mode.and_then(Value::as_str) {
            Some("blog") => Mode::Blog,
            Some("place") => Mode::Place,
            _ => Mode::Other,
        }
    }
}

/// `(머리, 키, 너비)` — `workbook.mjs:7-15`
pub fn columns(mode: Mode) -> &'static [(&'static str, &'static str, f64)] {
    match mode {
        Mode::Blog => &[
            ("키워드", "keyword", 26.0),
            ("상태", "status", 14.0),
            ("노출 순위", "rank", 14.0),
            ("확인 글 수", "total", 14.0),
            ("타겟", "target", 32.0),
            ("매칭 글 제목", "title", 60.0),
            ("매칭 글 URL", "matchedUrl", 55.0),
            ("조회 시각 (한국)", "checkedAt", 26.0),
            ("조회 기준", "scope", 70.0),
            ("검색 URL", "searchUrl", 55.0),
            ("안내", "message", 65.0),
        ],
        Mode::Place => &[
            ("키워드", "keyword", 26.0),
            ("상태", "status", 14.0),
            ("전체 순위", "rank", 14.0),
            ("광고 제외 순위", "organicRank", 18.0),
            ("페이지", "page", 12.0),
            ("확인 업체 수", "total", 16.0),
            ("광고 수", "totalAds", 12.0),
            ("타겟 병원명", "target", 26.0),
            ("매칭 상호", "title", 40.0),
            ("조회 시각 (한국)", "checkedAt", 26.0),
            ("조회 기준", "scope", 80.0),
            ("검색 URL", "searchUrl", 55.0),
            ("안내", "message", 65.0),
        ],
        Mode::Other => &[
            ("키워드", "keyword", 26.0),
            ("상태", "status", 14.0),
            ("노출 순위", "rank", 14.0),
            ("확인 광고 수", "totalAds", 15.0),
            ("타겟 도메인", "domain", 30.0),
            ("노출 광고명", "title", 30.0),
            ("조회 시각 (한국)", "checkedAt", 26.0),
            ("조회 기준", "scope", 32.0),
            ("검색 URL", "searchUrl", 55.0),
            ("안내", "message", 65.0),
        ],
    }
}

/// `export_xlsx` 입력 검증 (`index.mjs:45`) — 1~50행, 각 행은 `keyword` 가 문자열인 객체.
pub fn validate_rows(rows: Option<&Value>) -> Result<&Vec<Value>, String> {
    match rows.and_then(Value::as_array) {
        Some(list)
            if !list.is_empty()
                && list.len() <= 50
                && list
                    .iter()
                    .all(|row| row.get("keyword").is_some_and(Value::is_string)) =>
        {
            Ok(list)
        }
        _ => Err(ROWS_MESSAGE.to_string()),
    }
}

/// 저장 대화상자 기본 파일명 — `네이버_키워드순위_YYYY-MM-DD.xlsx`(한국 날짜, `main.jsx:79`).
pub fn default_file_name(now: DateTime<chrono::Utc>) -> String {
    format!(
        "네이버_키워드순위_{}.xlsx",
        now.with_timezone(&kst()).format("%Y-%m-%d")
    )
}

fn kst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("KST offset")
}

/// `new Date(checkedAt).toLocaleString('sv-SE', { timeZone: 'Asia/Seoul' })`
fn korean_time(value: &Value) -> String {
    let parsed: Option<DateTime<FixedOffset>> = match value {
        Value::String(s) => DateTime::parse_from_rfc3339(s).ok(),
        Value::Number(n) => n
            .as_f64()
            .and_then(|ms| chrono::Utc.timestamp_millis_opt(ms as i64).single())
            .map(|d| d.fixed_offset()),
        _ => None,
    };
    match parsed {
        Some(d) => d
            .with_timezone(&kst())
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        None => "Invalid Date".to_string(),
    }
}

fn first_match(row: &Value) -> Option<&Value> {
    row.get("matches")
        .and_then(Value::as_array)
        .and_then(|m| m.first())
}

/// 한 행의 열 값 (`workbook.mjs:17`). `None` 은 값 없음(빈 셀).
fn cell_value(row: &Value, key: &str, mode: Mode) -> Option<Value> {
    let or_empty = |v: Option<&Value>| {
        if js::truthy(v) {
            v.cloned()
        } else {
            Some(Value::from(""))
        }
    };
    match key {
        "matchedUrl" => or_empty(first_match(row).and_then(|m| m.get("url"))),
        "status" => {
            let status = row.get("status")?;
            match status.as_str().and_then(label) {
                Some(l) => Some(Value::from(l)),
                None => Some(status.clone()),
            }
        }
        "title" if mode == Mode::Place => {
            let joined = row.get("matches").and_then(Value::as_array).map(|ms| {
                ms.iter()
                    .map(|m| match m.get("name") {
                        None | Some(Value::Null) => String::new(),
                        other => js::to_js_string(other),
                    })
                    .collect::<Vec<_>>()
                    .join(" / ")
            });
            Some(Value::from(joined.unwrap_or_default()))
        }
        "title" => or_empty(first_match(row).and_then(|m| m.get("title"))),
        "checkedAt" => {
            let at = row.get("checkedAt");
            Some(Value::from(if js::truthy(at) {
                korean_time(at.expect("truthy is present"))
            } else {
                String::new()
            }))
        }
        "message" => {
            let message = row.get("message");
            if js::truthy(message) {
                message.cloned()
            } else if row.get("status").and_then(Value::as_str) == Some("not_found") {
                Some(Value::from("이번 응답의 첫 파워링크 광고 목록에 없음"))
            } else {
                Some(Value::from(""))
            }
        }
        _ => row.get(key).cloned(),
    }
}

/// `makeWorkbook` (`workbook.mjs:3-24`)
pub fn make_workbook(rows: &[Value], mode: Mode) -> Result<Vec<u8>, XlsxError> {
    let mut workbook = Workbook::new();
    workbook.set_properties(&DocProperties::new().set_author("SCAX"));
    let sheet = workbook.add_worksheet();
    sheet.set_name("키워드 순위")?;
    sheet.set_freeze_panes(1, 0)?;

    let cols = columns(mode);
    let header = Format::new()
        .set_bold()
        .set_font_color(Color::RGB(0xFFFFFF))
        .set_background_color(Color::RGB(0x5467F7));
    let body = Format::new()
        .set_font_size(11)
        .set_align(FormatAlign::VerticalCenter);

    for (c, (title, _, width)) in cols.iter().enumerate() {
        let c = c as u16;
        sheet.set_column_width(c, *width)?;
        sheet.write_string_with_format(0, c, *title, &header)?;
    }
    sheet.set_row_height(0, 28)?;

    for (r, row) in rows.iter().enumerate() {
        let r = (r + 1) as u32;
        sheet.set_row_height(r, 25)?;
        for (c, (_, key, _)) in cols.iter().enumerate() {
            let c = c as u16;
            match cell_value(row, key, mode) {
                None | Some(Value::Null) => {}
                Some(Value::String(s)) if s.is_empty() => {
                    sheet.write_blank(r, c, &body)?;
                }
                Some(Value::String(s)) => {
                    sheet.write_string_with_format(r, c, s, &body)?;
                }
                Some(Value::Number(n)) => {
                    sheet.write_number_with_format(r, c, n.as_f64().unwrap_or(0.0), &body)?;
                }
                Some(Value::Bool(b)) => {
                    sheet.write_boolean_with_format(r, c, b, &body)?;
                }
                Some(other) => {
                    sheet.write_string_with_format(r, c, js::to_js_string(Some(&other)), &body)?;
                }
            }
        }
    }

    let last_row = rows.len() as u32; // 머리행 포함 rowCount - 1
    sheet.autofilter(0, 0, last_row, (cols.len() - 1) as u16)?;
    workbook.save_to_buffer()
}
