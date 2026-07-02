//! HWPX 직렬화 공용 헬퍼 — XML escape / 공통 이벤트 쓰기

use std::io::Write;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;

use super::SerializeError;

/// `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` 선언을 쓴다.
pub fn write_xml_decl<W: Write>(w: &mut Writer<W>) -> Result<(), SerializeError> {
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), Some("yes"))))
        .map_err(|e| SerializeError::XmlError(e.to_string()))?;
    Ok(())
}

/// 속성 없는 시작 태그
pub fn start_tag<W: Write>(w: &mut Writer<W>, name: &str) -> Result<(), SerializeError> {
    w.write_event(Event::Start(BytesStart::new(name)))
        .map_err(|e| SerializeError::XmlError(e.to_string()))?;
    Ok(())
}

/// 속성 있는 시작 태그
pub fn start_tag_attrs<W: Write>(
    w: &mut Writer<W>,
    name: &str,
    attrs: &[(&str, &str)],
) -> Result<(), SerializeError> {
    let mut el = BytesStart::new(name);
    for (k, v) in attrs {
        el.push_attribute((*k, *v));
    }
    w.write_event(Event::Start(el))
        .map_err(|e| SerializeError::XmlError(e.to_string()))?;
    Ok(())
}

/// 종료 태그
pub fn end_tag<W: Write>(w: &mut Writer<W>, name: &str) -> Result<(), SerializeError> {
    w.write_event(Event::End(BytesEnd::new(name)))
        .map_err(|e| SerializeError::XmlError(e.to_string()))?;
    Ok(())
}

/// 자기 닫힘 태그 (`<name a="..."/>`)
pub fn empty_tag<W: Write>(
    w: &mut Writer<W>,
    name: &str,
    attrs: &[(&str, &str)],
) -> Result<(), SerializeError> {
    let mut el = BytesStart::new(name);
    for (k, v) in attrs {
        el.push_attribute((*k, *v));
    }
    w.write_event(Event::Empty(el))
        .map_err(|e| SerializeError::XmlError(e.to_string()))?;
    Ok(())
}

/// 텍스트 노드 (자동 이스케이프)
pub fn text<W: Write>(w: &mut Writer<W>, content: &str) -> Result<(), SerializeError> {
    w.write_event(Event::Text(BytesText::new(content)))
        .map_err(|e| SerializeError::XmlError(e.to_string()))?;
    Ok(())
}

/// 이미 escape 완료된 XML 조각(String 기반 writer 출력)을 Writer 에 그대로 주입한다.
/// section.rs `render_run_content` 처럼 String 을 생산하는 경로와 quick_xml Writer
/// 기반 경로(table.rs/shape.rs)를 잇는 브리지.
pub fn write_raw<W: Write>(w: &mut Writer<W>, xml: &str) -> Result<(), SerializeError> {
    w.get_mut()
        .write_all(xml.as_bytes())
        .map_err(|e| SerializeError::XmlError(format!("raw write: {e}")))
}

/// XML 속성·텍스트 이스케이프 (&, <, >, ", ')
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

// =====================================================================
// 공용 색상 / 채우기(fillBrush) 직렬화
//
// borderFill(header.xml)과 도형(section.xml)의 <hc:fillBrush> 는 동일한
// winBrush/gradation/imgBrush 계약을 공유한다 — header.rs 와 shape/section
// writer 가 이 헬퍼를 함께 사용한다 (중복 구현 금지).
// =====================================================================

use crate::model::style::{Fill, FillType, ImageFillMode};
use crate::model::ColorRef;

use super::context::SerializeContext;

pub(crate) fn color_hex(c: ColorRef) -> String {
    // ColorRef = u32. HWP 내부 저장: 상위 바이트가 비투명 플래그(0이면 유효 색상).
    // 0xFFFFFFFF = 투명/없음 센티넬 → "none"
    if c == 0xFFFFFFFF {
        return "none".to_string();
    }
    // HWPX는 "#RRGGBB" 또는 "#AARRGGBB".
    let a = ((c >> 24) & 0xFF) as u8;
    let r = (c & 0xFF) as u8;
    let g = ((c >> 8) & 0xFF) as u8;
    let b = ((c >> 16) & 0xFF) as u8;
    if a == 0 {
        format!("#{:02X}{:02X}{:02X}", r, g, b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", a, r, g, b)
    }
}

/// `<hc:fillBrush>` 자식 직렬화 — HWPX parser 계약
/// (src/parser/hwpx/header.rs `parse_border_fill` / section.rs `parse_shape_fill_brush`
/// 의 winBrush/gradation/imgBrush 분기)의 역방향 미러.
pub(crate) fn write_fill_brush<W: Write>(
    w: &mut Writer<W>,
    fill: &Fill,
    ctx: &SerializeContext,
) -> Result<(), SerializeError> {
    start_tag(w, "hc:fillBrush")?;
    match fill.fill_type {
        FillType::Solid => {
            if let Some(solid) = &fill.solid {
                let face = color_hex(solid.background_color);
                let hatch = color_hex(solid.pattern_color);
                let alpha = fill_alpha_str(fill.alpha);
                // 속성 순서: faceColor, hatchColor, (hatchStyle), alpha (한컴 관찰: aift.hwpx 등)
                let mut attrs: Vec<(&str, &str)> =
                    vec![("faceColor", &face), ("hatchColor", &hatch)];
                if let Some(hs) = hatch_style_str(solid.pattern_type) {
                    attrs.push(("hatchStyle", hs));
                }
                attrs.push(("alpha", &alpha));
                empty_tag(w, "hc:winBrush", &attrs)?;
            }
        }
        FillType::Gradient => {
            if let Some(grad) = &fill.gradient {
                let ty = grad.gradient_type.to_string();
                let angle = grad.angle.to_string();
                let cx = grad.center_x.to_string();
                let cy = grad.center_y.to_string();
                let blur = grad.blur.to_string();
                start_tag_attrs(
                    w,
                    "hc:gradation",
                    &[
                        ("type", &ty),
                        ("angle", &angle),
                        ("centerX", &cx),
                        ("centerY", &cy),
                        ("blur", &blur),
                    ],
                )?;
                for c in &grad.colors {
                    let v = color_hex(*c);
                    empty_tag(w, "hc:color", &[("value", &v)])?;
                }
                end_tag(w, "hc:gradation")?;
            }
        }
        FillType::Image => {
            if let Some(img) = &fill.image {
                let bright = img.brightness.to_string();
                let contrast = img.contrast.to_string();
                start_tag_attrs(
                    w,
                    "hc:imgBrush",
                    &[
                        ("mode", image_fill_mode_str(img.fill_mode)),
                        ("bright", &bright),
                        ("contrast", &contrast),
                    ],
                )?;
                // binaryItemIDRef 는 그림(<hc:img>)과 동일하게 ctx.bin_data_map 을 통해
                // manifest id 로 변환. 미등록 bin_data_id 면 img 자식만 생략 (panic 금지).
                if let Some(manifest_id) = ctx.resolve_bin_id(img.bin_data_id) {
                    empty_tag(w, "hc:img", &[("binaryItemIDRef", manifest_id)])?;
                }
                end_tag(w, "hc:imgBrush")?;
            }
        }
        FillType::None => {}
    }
    end_tag(w, "hc:fillBrush")?;
    Ok(())
}

/// alpha u8(0~255) → HWPX float 문자열(0.0~1.0).
///
/// parser 는 `(f * 255) as u8` (버림) 로 되읽으므로, 중간값은 `(a + 0.5) / 255` 로 내보내
/// 부동소수점 반올림 오차와 무관하게 정확히 `a` 로 복원되게 한다.
/// 한컴 관찰값(aift.hwpx 등)은 대부분 정수 표기 `alpha="0"`.
fn fill_alpha_str(alpha: u8) -> String {
    match alpha {
        0 => "0".to_string(),
        255 => "1".to_string(),
        a => ((a as f64 + 0.5) / 255.0).to_string(),
    }
}

/// `parse_hatch_style` (src/parser/hwpx/utils.rs) 의 역함수.
/// pattern_type 1~6 외에는 hatchStyle 속성 자체를 생략한다 (무늬 없음).
fn hatch_style_str(pattern_type: i32) -> Option<&'static str> {
    match pattern_type {
        1 => Some("HORIZONTAL"),
        2 => Some("VERTICAL"),
        3 => Some("BACK_SLASH"),
        4 => Some("SLASH"),
        5 => Some("CROSS"),
        6 => Some("CROSS_DIAGONAL"),
        _ => None,
    }
}

/// ImageFillMode → OWPML `imgBrush/@mode` 문자열.
/// parser 가 같은 variant 로 역매핑하는 문자열을 우선 사용한다.
fn image_fill_mode_str(m: ImageFillMode) -> &'static str {
    match m {
        ImageFillMode::TileAll => "TILE",
        ImageFillMode::TileHorzTop => "TILE_HORZ_TOP",
        ImageFillMode::TileHorzBottom => "TILE_HORZ_BOTTOM",
        ImageFillMode::TileVertLeft => "TILE_VERT_LEFT",
        ImageFillMode::TileVertRight => "TILE_VERT_RIGHT",
        ImageFillMode::FitToSize => "TOTAL",
        ImageFillMode::Center => "CENTER",
        ImageFillMode::CenterTop => "CENTER_TOP",
        ImageFillMode::CenterBottom => "CENTER_BOTTOM",
        ImageFillMode::LeftTop => "TOP_LEFT_ALIGN",
        // 아래 variant 들은 rhwp parser 에 역매핑 문자열이 없다 (parser 한계).
        // OWPML 정식 명칭으로 내보낸다.
        ImageFillMode::LeftCenter => "LEFT_CENTER",
        ImageFillMode::LeftBottom => "LEFT_BOTTOM",
        ImageFillMode::RightCenter => "RIGHT_CENTER",
        ImageFillMode::RightTop => "RIGHT_TOP",
        ImageFillMode::RightBottom => "RIGHT_BOTTOM",
        ImageFillMode::None => "NONE",
    }
}
