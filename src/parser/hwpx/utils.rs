//! HWPX 파서 공통 유틸리티 함수
//!
//! header.rs, section.rs 등에서 공통으로 사용하는 XML 파싱 헬퍼.

use quick_xml::events::Event;
use quick_xml::Reader;

use super::HwpxError;

/// XML 네임스페이스 접두사를 제거하고 로컬 이름만 반환
/// 예: b"hp:p" → b"p", b"tbl" → b"tbl"
pub fn local_name(name: &[u8]) -> &[u8] {
    if let Some(pos) = name.iter().position(|&b| b == b':') {
        &name[pos + 1..]
    } else {
        name
    }
}

/// 속성 값을 String으로 변환 (XML 엔티티 unescape 포함).
///
/// quick-xml 은 속성값을 **원문 그대로**(`&amp;` 등 이스케이프된 형태) 보관한다.
/// IR 은 시맨틱 값(예: `R&D`)을 저장해야 하고, 직렬화 측 writer 가 유일한 escape
/// 권위가 되어야 대칭이 성립한다. unescape 를 빠뜨리면 폼 caption 등 문자열 속성이
/// 저장할 때마다 한 겹씩 이중 이스케이프된다(`R&&D`→`R&amp;&amp;D`→…, Task #1534).
///
/// 숫자/열거형 속성은 엔티티가 없어 unescape 가 no-op 이라 무영향. 미정의 엔티티/
/// malformed 입력은 원문 lossy 로 안전 폴백한다.
pub fn attr_str(attr: &quick_xml::events::attributes::Attribute) -> String {
    let raw = String::from_utf8_lossy(&attr.value);
    match quick_xml::escape::unescape(&raw) {
        Ok(value) => value.into_owned(),
        Err(_) => raw.into_owned(),
    }
}

/// 속성 값이 특정 문자열과 일치하는지 확인 (비교용)
pub fn attr_eq(attr: &quick_xml::events::attributes::Attribute, val: &str) -> bool {
    attr.value.as_ref() == val.as_bytes()
}

pub fn parse_u8(attr: &quick_xml::events::attributes::Attribute) -> u8 {
    attr_str(attr).parse().unwrap_or(0)
}

pub fn parse_i8(attr: &quick_xml::events::attributes::Attribute) -> i8 {
    attr_str(attr).parse().unwrap_or(0)
}

pub fn parse_u16(attr: &quick_xml::events::attributes::Attribute) -> u16 {
    attr_str(attr).parse().unwrap_or(0)
}

pub fn parse_i16(attr: &quick_xml::events::attributes::Attribute) -> i16 {
    attr_str(attr).parse().unwrap_or(0)
}

pub fn parse_u32(attr: &quick_xml::events::attributes::Attribute) -> u32 {
    attr_str(attr).parse().unwrap_or(0)
}

pub fn parse_i32(attr: &quick_xml::events::attributes::Attribute) -> i32 {
    attr_str(attr).parse().unwrap_or(0)
}

/// HWPX는 음수 HWPUNIT 값을 unsigned 32-bit decimal 문자열로 저장하는 경우가 있다.
///
/// 예: `4294964867`은 HWP5 little-endian 필드에서 `0xfffff683`, 즉 signed `-2429`이다.
/// 일반 `i32::parse`는 이 값을 overflow로 실패하므로, 먼저 i32를 시도하고 실패하면
/// u32로 읽어 wrapping cast 한다.
pub fn parse_i32_wrapping(attr: &quick_xml::events::attributes::Attribute) -> i32 {
    let s = attr_str(attr);
    if let Ok(v) = s.parse::<i32>() {
        return v;
    }
    if let Ok(v) = s.parse::<u32>() {
        return v as i32;
    }
    0
}

/// HWPX 색 문자열 → HWP `ColorRef`(COLORREF `0x00BBGGRR`).
pub fn parse_color(attr: &quick_xml::events::attributes::Attribute) -> u32 {
    let s = attr_str(attr);
    parse_color_str(&s)
}

/// 색상 문자열을 HWP ColorRef로 변환.
///
/// ⚠️ 2026-08-15 정합 수리: HWPX 의 `#……` 16진 숫자는 **COLORREF 그대로**
/// (`#BBGGRR`)다 — 데스크톱 한글 2014 실측(같은 문서의 textColor 만 바꾼 3파일:
/// `#FF0000`→파랑, `#0000FF`→빨강, 대조군 `#00FF00`→초록 유지. HWP5 바이너리의
/// `COLORREF = 0x00BBGGRR` 관습이 HWPX 문자열에 그대로 넘어온 것).
/// 종전 구현은 문자열을 `#RRGGBB` 로 해석해 R↔B 를 스왑했고, 그 결과 한글이
/// 빨강으로 그리는 문서를 rhwp 는 파랑으로 그렸다(HWP5·HML 경로는 처음부터
/// COLORREF 그대로였다 — HWPX 문자열 계층만 갈라져 있던 것).
/// 8자리는 상위 바이트(알파/센티널)를 포함해 그대로 읽는다 —
/// `#FFFFFFFF` = `0xFFFFFFFF`(none 센티널)와 자연 일치.
pub fn parse_color_str(s: &str) -> u32 {
    if s == "none" || s.is_empty() {
        return 0xFFFFFFFF; // 투명/없음
    }
    let hex = s.trim_start_matches('#');
    if hex.len() == 6 || hex.len() == 8 {
        if let Ok(v) = u32::from_str_radix(hex, 16) {
            return v;
        }
    }
    0x00000000 // 검정
}

/// 속성 값을 bool로 파싱 ("true", "1" → true)
pub fn parse_bool(attr: &quick_xml::events::attributes::Attribute) -> bool {
    let s = attr_str(attr);
    s == "true" || s == "1"
}

/// OWPML `winBrush/@hatchStyle`을 HWP 무늬 번호로 변환한다.
///
/// HWP 쪽 `pattern_type`은 `-1`이 무늬없음이고, 1~6이 OWPML 스키마의
/// 6개 hatchStyle 값에 대응한다. HWPX에서 hatchStyle이 생략되면 무늬없음으로
/// 저장해야 하므로 호출자는 기본값으로 `-1`을 사용한다.
pub fn parse_hatch_style(value: &str) -> Option<i32> {
    match value {
        "HORIZONTAL" => Some(1),
        "VERTICAL" => Some(2),
        "BACK_SLASH" => Some(3),
        "SLASH" => Some(4),
        "CROSS" => Some(5),
        "CROSS_DIAGONAL" => Some(6),
        _ => None,
    }
}

/// OWPML gradient type 값을 HWP5 gradient kind 값으로 변환한다.
pub fn parse_gradient_type(value: &str) -> i16 {
    match value {
        "LINEAR" => 1,
        "RADIAL" => 2,
        "CONICAL" => 3,
        "SQUARE" => 4,
        _ => value.parse().unwrap_or(0),
    }
}

/// XML 요소를 자식 포함하여 건너뛰기 (깊이 추적)
pub fn skip_element(reader: &mut Reader<&[u8]>, _end_tag: &[u8]) -> Result<(), HwpxError> {
    let mut buf = Vec::new();
    let mut depth = 1u32;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(_)) => {
                depth += 1;
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(HwpxError::XmlError(format!("skip: {}", e))),
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_name() {
        assert_eq!(local_name(b"hp:p"), b"p");
        assert_eq!(local_name(b"tbl"), b"tbl");
        assert_eq!(local_name(b"hh:charPr"), b"charPr");
    }

    #[test]
    fn test_parse_color_str() {
        // 색 정합 수리(2026-08-15): HWPX 숫자 = COLORREF 그대로(#BBGGRR — 한글 실측).
        assert_eq!(parse_color_str("#0000FF"), 0x000000FF); // 한글이 빨강으로 그리는 값
        assert_eq!(parse_color_str("#00FF00"), 0x0000FF00); // 초록(대조군 — 스왑 불변)
        assert_eq!(parse_color_str("#FF0000"), 0x00FF0000); // 한글이 파랑으로 그리는 값
        assert_eq!(parse_color_str("#000000"), 0x00000000); // 검정
        assert_eq!(parse_color_str("none"), 0xFFFFFFFF); // 투명
    }

    #[test]
    fn test_parse_color_str_with_alpha() {
        // 8자리도 그대로(상위 바이트 = 알파/센티널).
        assert_eq!(parse_color_str("#800000FF"), 0x800000FF);
        assert_eq!(parse_color_str("#FF000000"), 0xFF000000); // 상위 바이트 비제로 → 채우기 없음
        assert_eq!(parse_color_str("#000000FF"), 0x000000FF); // alpha=00 → 동일
        assert_eq!(parse_color_str("#FFFFFFFF"), 0xFFFFFFFF); // none 센티널과 자연 일치
    }

    #[test]
    fn test_parse_hatch_style() {
        assert_eq!(parse_hatch_style("HORIZONTAL"), Some(1));
        assert_eq!(parse_hatch_style("VERTICAL"), Some(2));
        assert_eq!(parse_hatch_style("BACK_SLASH"), Some(3));
        assert_eq!(parse_hatch_style("SLASH"), Some(4));
        assert_eq!(parse_hatch_style("CROSS"), Some(5));
        assert_eq!(parse_hatch_style("CROSS_DIAGONAL"), Some(6));
        assert_eq!(parse_hatch_style(""), None);
    }
}
