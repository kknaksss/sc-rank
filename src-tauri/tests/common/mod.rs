//! 테스트 공용 — 만든 xlsx 를 다시 읽는다.
#![allow(dead_code)]

use std::io::{Cursor, Read};

use calamine::{Data, Reader, Xlsx};

pub struct Sheet {
    pub range: calamine::Range<Data>,
    pub formulas: calamine::Range<String>,
}

pub fn read_sheet(bytes: &[u8]) -> Sheet {
    let mut wb: Xlsx<_> = Xlsx::new(Cursor::new(bytes.to_vec())).expect("xlsx opens");
    let name = wb.sheet_names()[0].clone();
    assert_eq!(name, "키워드 순위");
    Sheet {
        range: wb.worksheet_range(&name).expect("sheet"),
        formulas: wb.worksheet_formula(&name).expect("formulas"),
    }
}

/// "D2" 같은 주소의 셀.
pub fn cell<'a>(sheet: &'a Sheet, addr: &str) -> Option<&'a Data> {
    let (row, col) = pos(addr);
    sheet.range.get_value((row, col))
}

pub fn pos(addr: &str) -> (u32, u32) {
    let letters: String = addr
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let row: u32 = addr[letters.len()..].parse().expect("row");
    let col = letters
        .bytes()
        .fold(0u32, |n, b| n * 26 + u32::from(b - b'A' + 1))
        - 1;
    (row - 1, col)
}

/// zip 안 파일 하나를 문자열로.
pub fn zip_text(bytes: &[u8], path: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("zip");
    let mut s = String::new();
    zip.by_name(path)
        .expect("zip entry")
        .read_to_string(&mut s)
        .expect("utf8");
    s
}
