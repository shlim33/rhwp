//! 그리기 개체 (도형) 직렬화 — Rectangle / Line / Container 뼈대.
//!
//! Stage 5 (#182): 대표 도형 3종(Rectangle, Line, Container)의 `<hp:rect>`, `<hp:line>`,
//! `<hp:container>` 요소 뼈대를 구현한다. 완전한 속성 커버리지는 별도 이슈로 이월.
//!
//! 속성·자식 순서는 한컴 OWPML 공식 (hancom-io/hwpx-owpml-model, Apache 2.0) 기준.
//!
//! ## 범위 한정
//!
//! - Stage 5 에서는 **도형 뼈대 출력** 기능만 제공 (section.rs dispatcher 연결은 #186).
//! - Arc / Polygon / Curve / Group 등은 향후 이슈에서 확장.
//! - DrawingObjAttr (선/채우기 세부 속성) 은 최소 기본값 출력.

#![allow(dead_code)]

use std::io::Write;

use quick_xml::Writer;

use crate::model::paragraph::Paragraph;
use crate::model::shape::{
    CommonObjAttr, HorzAlign, HorzRelTo, LineShape, RectangleShape, TextBox, TextWrap,
    VertAlign, VertRelTo,
};

use super::context::SerializeContext;
use super::utils::{empty_tag, end_tag, start_tag, start_tag_attrs, write_raw};
use super::SerializeError;

// =====================================================================
// <hp:rect>
// =====================================================================

/// `<hp:rect>` 직렬화 진입점. Rectangle IR → XML.
/// `ctx` 는 drawText 문단 내 인라인 컨트롤(그림/중첩 표 등) 직렬화에 사용.
pub fn write_rect<W: Write>(
    w: &mut Writer<W>,
    rect: &RectangleShape,
    ctx: &mut SerializeContext,
) -> Result<(), SerializeError> {
    let c = &rect.common;
    // 속성 (부모 AbstractShapeObjectType + 자신):
    // id, zOrder, numberingType, textWrap, textFlow, lock, dropcapstyle,
    // href, groupLevel, instid, ratio
    let id_str = c.instance_id.to_string();
    let z_order = c.z_order.to_string();
    let tw = text_wrap_str(c.text_wrap);

    start_tag_attrs(
        w,
        "hp:rect",
        &[
            ("id", &id_str),
            ("zOrder", &z_order),
            ("numberingType", "NONE"),
            ("textWrap", tw),
            ("textFlow", "BOTH_SIDES"),
            ("lock", "0"),
            ("dropcapstyle", "None"),
            ("href", ""),
            ("groupLevel", "0"),
            ("instid", &id_str),
            ("ratio", "0"),
        ],
    )?;

    // 기본 자식: sz, pos, outMargin
    write_sz(w, c)?;
    write_pos(w, c)?;
    write_out_margin(w, c)?;

    // drawText: 글상자 내부 문단
    if let Some(ref tb) = rect.drawing.text_box {
        if !tb.paragraphs.is_empty() {
            write_draw_text(w, tb, ctx)?;
        }
    }

    end_tag(w, "hp:rect")?;
    Ok(())
}

// =====================================================================
// <hp:line>
// =====================================================================

/// `<hp:line>` 직렬화 진입점. LineShape IR → XML.
pub fn write_line<W: Write>(w: &mut Writer<W>, line: &LineShape) -> Result<(), SerializeError> {
    let c = &line.common;
    let id_str = c.instance_id.to_string();
    let z_order = c.z_order.to_string();
    let tw = text_wrap_str(c.text_wrap);
    let sx = line.start.x.to_string();
    let sy = line.start.y.to_string();
    let ex = line.end.x.to_string();
    let ey = line.end.y.to_string();
    let srb = bool01(line.started_right_or_bottom);

    start_tag_attrs(
        w,
        "hp:line",
        &[
            ("id", &id_str),
            ("zOrder", &z_order),
            ("numberingType", "NONE"),
            ("textWrap", tw),
            ("textFlow", "BOTH_SIDES"),
            ("lock", "0"),
            ("dropcapstyle", "None"),
            ("href", ""),
            ("groupLevel", "0"),
            ("instid", &id_str),
            ("startX", &sx),
            ("startY", &sy),
            ("endX", &ex),
            ("endY", &ey),
            ("isReverseHV", srb),
        ],
    )?;

    write_sz(w, c)?;
    write_pos(w, c)?;
    write_out_margin(w, c)?;

    end_tag(w, "hp:line")?;
    Ok(())
}

// =====================================================================
// <hp:container> — 묶음 개체 (GroupShape). Stage 5 뼈대만.
// =====================================================================

/// `<hp:container>` 뼈대 — 내부 자식 도형 루프는 dispatcher에서 처리.
pub fn write_container_open<W: Write>(
    w: &mut Writer<W>,
    common: &CommonObjAttr,
) -> Result<(), SerializeError> {
    let id_str = common.instance_id.to_string();
    let z_order = common.z_order.to_string();
    let tw = text_wrap_str(common.text_wrap);

    start_tag_attrs(
        w,
        "hp:container",
        &[
            ("id", &id_str),
            ("zOrder", &z_order),
            ("numberingType", "NONE"),
            ("textWrap", tw),
            ("textFlow", "BOTH_SIDES"),
            ("lock", "0"),
            ("dropcapstyle", "None"),
            ("href", ""),
            ("groupLevel", "0"),
            ("instid", &id_str),
        ],
    )?;

    write_sz(w, common)?;
    write_pos(w, common)?;
    write_out_margin(w, common)?;

    Ok(())
}

pub fn write_container_close<W: Write>(w: &mut Writer<W>) -> Result<(), SerializeError> {
    end_tag(w, "hp:container")
}

// =====================================================================
// <hp:drawText> — 글상자 내부 텍스트
// =====================================================================

/// `<hp:drawText>` 직렬화 — TextBox의 paragraphs를 subList로 출력.
pub fn write_draw_text<W: Write>(
    w: &mut Writer<W>,
    tb: &TextBox,
    ctx: &mut SerializeContext,
) -> Result<(), SerializeError> {
    let ml = tb.margin_left.to_string();
    let mr = tb.margin_right.to_string();
    let mt = tb.margin_top.to_string();
    let mb = tb.margin_bottom.to_string();
    let mw = tb.max_width.to_string();

    start_tag_attrs(
        w,
        "hp:drawText",
        &[("lastWidth", &mw)],
    )?;

    empty_tag(
        w,
        "hp:textMargin",
        &[("left", &ml), ("right", &mr), ("top", &mt), ("bottom", &mb)],
    )?;

    start_tag_attrs(
        w,
        "hp:subList",
        &[
            ("id", ""),
            ("textDirection", "HORIZONTAL"),
            ("lineWrap", "BREAK"),
            ("vertAlign", "TOP"),
            ("linkListIDRef", "0"),
            ("linkListNextIDRef", "0"),
            ("textWidth", "0"),
            ("textHeight", "0"),
            ("hasTextRef", "0"),
            ("hasNumRef", "0"),
        ],
    )?;

    for (idx, p) in tb.paragraphs.iter().enumerate() {
        write_draw_text_paragraph(w, p, idx, ctx)?;
    }

    end_tag(w, "hp:subList")?;
    end_tag(w, "hp:drawText")?;
    Ok(())
}

fn write_draw_text_paragraph<W: Write>(
    w: &mut Writer<W>,
    p: &Paragraph,
    idx: usize,
    ctx: &mut SerializeContext,
) -> Result<(), SerializeError> {
    let id = idx.to_string();
    let ps_id = p.para_shape_id.to_string();
    let st_id = p.style_id.to_string();

    start_tag_attrs(
        w,
        "hp:p",
        &[
            ("id", &id),
            ("paraPrIDRef", &ps_id),
            ("styleIDRef", &st_id),
            ("pageBreak", "0"),
            ("columnBreak", "0"),
            ("merged", "0"),
        ],
    )?;

    let cs = p.char_shapes.first().map(|r| r.char_shape_id).unwrap_or(0);
    let cs_str = cs.to_string();
    start_tag_attrs(w, "hp:run", &[("charPrIDRef", &cs_str)])?;

    // 본문과 동일한 controls-aware 공유 writer — 텍스트(<hp:t>, escape 포함)와
    // 인라인 컨트롤(그림 등)을 char-offset 슬롯 위치에 emit 한다.
    let content = super::section::render_run_content(p, ctx);
    write_raw(w, &content)?;

    end_tag(w, "hp:run")?;

    // minimal lineseg
    start_tag(w, "hp:linesegarray")?;
    empty_tag(
        w,
        "hp:lineseg",
        &[
            ("textpos", "0"),
            ("vertpos", "0"),
            ("vertsize", "1000"),
            ("textheight", "1000"),
            ("baseline", "850"),
            ("spacing", "600"),
            ("horzpos", "0"),
            ("horzsize", "42520"),
            ("flags", "393216"),
        ],
    )?;
    end_tag(w, "hp:linesegarray")?;

    end_tag(w, "hp:p")?;
    Ok(())
}

// =====================================================================
// 공통 자식 요소 (sz / pos / outMargin)
// =====================================================================

fn write_sz<W: Write>(w: &mut Writer<W>, c: &CommonObjAttr) -> Result<(), SerializeError> {
    let width = c.width.to_string();
    let height = c.height.to_string();
    empty_tag(
        w,
        "hp:sz",
        &[
            ("width", &width),
            ("widthRelTo", "ABSOLUTE"),
            ("height", &height),
            ("heightRelTo", "ABSOLUTE"),
            ("protect", "0"),
        ],
    )
}

fn write_pos<W: Write>(w: &mut Writer<W>, c: &CommonObjAttr) -> Result<(), SerializeError> {
    let treat = bool01(c.treat_as_char);
    let vert_offset = c.vertical_offset.to_string();
    let horz_offset = c.horizontal_offset.to_string();
    empty_tag(
        w,
        "hp:pos",
        &[
            ("treatAsChar", treat),
            ("affectLSpacing", "0"),
            ("flowWithText", "1"),
            ("allowOverlap", "0"),
            ("holdAnchorAndSO", "0"),
            ("vertRelTo", vert_rel_to_str(c.vert_rel_to)),
            ("horzRelTo", horz_rel_to_str(c.horz_rel_to)),
            ("vertAlign", vert_align_str(c.vert_align)),
            ("horzAlign", horz_align_str(c.horz_align)),
            ("vertOffset", &vert_offset),
            ("horzOffset", &horz_offset),
        ],
    )
}

fn write_out_margin<W: Write>(w: &mut Writer<W>, c: &CommonObjAttr) -> Result<(), SerializeError> {
    let l = c.margin.left.to_string();
    let r = c.margin.right.to_string();
    let t = c.margin.top.to_string();
    let b = c.margin.bottom.to_string();
    empty_tag(
        w,
        "hp:outMargin",
        &[("left", &l), ("right", &r), ("top", &t), ("bottom", &b)],
    )
}

fn bool01(b: bool) -> &'static str {
    if b { "1" } else { "0" }
}

fn text_wrap_str(w: TextWrap) -> &'static str {
    use TextWrap::*;
    match w {
        Square => "SQUARE",
        Tight => "TIGHT",
        Through => "THROUGH",
        TopAndBottom => "TOP_AND_BOTTOM",
        BehindText => "BEHIND_TEXT",
        InFrontOfText => "IN_FRONT_OF_TEXT",
    }
}

fn vert_rel_to_str(v: VertRelTo) -> &'static str {
    use VertRelTo::*;
    match v {
        Paper => "PAPER",
        Page => "PAGE",
        Para => "PARA",
    }
}

fn horz_rel_to_str(h: HorzRelTo) -> &'static str {
    use HorzRelTo::*;
    match h {
        Paper => "PAPER",
        Page => "PAGE",
        Column => "COLUMN",
        Para => "PARA",
    }
}

fn vert_align_str(v: VertAlign) -> &'static str {
    use VertAlign::*;
    match v {
        Top => "TOP",
        Center => "CENTER",
        Bottom => "BOTTOM",
        Inside => "INSIDE",
        Outside => "OUTSIDE",
    }
}

fn horz_align_str(h: HorzAlign) -> &'static str {
    use HorzAlign::*;
    match h {
        Left => "LEFT",
        Center => "CENTER",
        Right => "RIGHT",
        Inside => "INSIDE",
        Outside => "OUTSIDE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Point;
    use crate::model::shape::{RectangleShape, LineShape};

    fn serialize_rect(rect: &RectangleShape) -> String {
        let doc = crate::model::document::Document::default();
        let mut ctx = SerializeContext::collect_from_document(&doc);
        let mut w: Writer<Vec<u8>> = Writer::new(Vec::new());
        write_rect(&mut w, rect, &mut ctx).expect("write_rect");
        String::from_utf8(w.into_inner()).unwrap()
    }

    fn serialize_line(line: &LineShape) -> String {
        let mut w: Writer<Vec<u8>> = Writer::new(Vec::new());
        write_line(&mut w, line).expect("write_line");
        String::from_utf8(w.into_inner()).unwrap()
    }

    #[test]
    fn rect_emits_root_tag() {
        let mut rect = RectangleShape::default();
        rect.common.width = 1000;
        rect.common.height = 500;
        let xml = serialize_rect(&rect);
        assert!(xml.contains("<hp:rect "));
        assert!(xml.contains("</hp:rect>"));
    }

    #[test]
    fn rect_has_canonical_attrs() {
        let rect = RectangleShape::default();
        let xml = serialize_rect(&rect);
        assert!(xml.contains(r#"id=""#));
        assert!(xml.contains(r#"zOrder=""#));
        assert!(xml.contains(r#"textWrap=""#));
        assert!(xml.contains(r#"textFlow="BOTH_SIDES""#));
    }

    #[test]
    fn line_emits_start_end_attrs() {
        let mut line = LineShape::default();
        line.start = Point { x: 100, y: 200 };
        line.end = Point { x: 300, y: 400 };
        let xml = serialize_line(&line);
        assert!(xml.contains(r#"startX="100""#));
        assert!(xml.contains(r#"startY="200""#));
        assert!(xml.contains(r#"endX="300""#));
        assert!(xml.contains(r#"endY="400""#));
    }

    #[test]
    fn rect_has_sz_pos_out_margin() {
        let rect = RectangleShape::default();
        let xml = serialize_rect(&rect);
        assert!(xml.contains("<hp:sz "));
        assert!(xml.contains("<hp:pos "));
        assert!(xml.contains("<hp:outMargin "));
    }

    // ---------------------------------------------------------------
    // drawText (글상자) 콘텐츠 라운드트립 — exportHwpx fidelity
    // ---------------------------------------------------------------

    /// 사각형 글상자에 텍스트 문단("제목") + 임베드 그림이 있는 경우
    /// serialize → parse 라운드트립에서 둘 다 보존돼야 한다.
    #[test]
    fn rect_draw_text_with_picture_roundtrips_through_hwpx() {
        use crate::model::bin_data::BinDataContent;
        use crate::model::control::Control;
        use crate::model::document::Document;
        use crate::model::image::{ImageAttr, Picture};
        use crate::model::shape::{CommonObjAttr, ShapeObject};
        use crate::parser::hwpx::parse_hwpx;
        use crate::serializer::hwpx::serialize_hwpx;

        let fake_png = b"\x89PNG\r\n\x1a\nfake_drawtext_image";
        let mut doc = Document::default();
        doc.doc_info.char_shapes.push(Default::default());
        doc.doc_info.para_shapes.push(Default::default());
        doc.doc_info.styles.push(Default::default());
        doc.bin_data_content.push(BinDataContent {
            id: 1,
            data: fake_png.to_vec(),
            extension: "png".to_string(),
        });

        // 글상자 문단: 그림 슬롯(8 유닛) 뒤에 "제목"
        let mut tb_para = Paragraph::default();
        tb_para.text = "제목".to_string();
        tb_para.char_offsets = vec![8, 9];
        tb_para.char_count = 11;
        tb_para.controls.push(Control::Picture(Box::new(Picture {
            common: CommonObjAttr {
                width: 5000,
                height: 3000,
                treat_as_char: true,
                ..Default::default()
            },
            image_attr: ImageAttr {
                bin_data_id: 1,
                ..Default::default()
            },
            ..Default::default()
        })));

        let mut rect = RectangleShape::default();
        rect.common.width = 10000;
        rect.common.height = 5000;
        rect.drawing.text_box = Some(TextBox {
            paragraphs: vec![tb_para],
            ..Default::default()
        });

        let mut section = crate::model::document::Section::default();
        let mut para = Paragraph::default();
        para.text = "A".to_string();
        para.char_offsets = vec![8];
        para.char_count = 10;
        para.controls
            .push(Control::Shape(Box::new(ShapeObject::Rectangle(rect))));
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize");
        let parsed = parse_hwpx(&bytes).expect("parse back");
        let rect = parsed.sections[0].paragraphs[0]
            .controls
            .iter()
            .find_map(|c| match c {
                Control::Shape(s) => match s.as_ref() {
                    ShapeObject::Rectangle(r) => Some(r),
                    _ => None,
                },
                _ => None,
            })
            .expect("rect shape must survive roundtrip");
        let tb = rect
            .drawing
            .text_box
            .as_ref()
            .expect("drawText (text_box) must survive roundtrip");
        assert!(
            !tb.paragraphs.is_empty(),
            "drawText paragraphs must survive"
        );
        assert!(
            tb.paragraphs[0].text.contains("제목"),
            "shape text must roundtrip: {:?}",
            tb.paragraphs[0].text
        );
        let pic = tb.paragraphs[0]
            .controls
            .iter()
            .find_map(|c| match c {
                Control::Picture(p) => Some(p),
                _ => None,
            })
            .expect("picture inside drawText must survive hwpx roundtrip");
        assert_eq!(
            pic.image_attr.bin_data_id, 1,
            "drawText picture must keep its bin_data reference"
        );
    }

    // ---------------------------------------------------------------
    // container (묶음 개체) 자식 라운드트립 — exportHwpx fidelity
    // ---------------------------------------------------------------

    /// 라운드트립 테스트용 Document 뼈대 + 그룹 컨트롤 1개를 담은 문서 생성.
    fn doc_with_group(
        group: crate::model::shape::GroupShape,
        bin_ids: &[u16],
    ) -> crate::model::document::Document {
        use crate::model::bin_data::BinDataContent;
        use crate::model::control::Control;
        use crate::model::document::Document;
        use crate::model::shape::ShapeObject;

        let mut doc = Document::default();
        doc.doc_info.char_shapes.push(Default::default());
        doc.doc_info.para_shapes.push(Default::default());
        doc.doc_info.styles.push(Default::default());
        for &id in bin_ids {
            doc.bin_data_content.push(BinDataContent {
                id,
                data: format!("\u{89}PNG_fake_container_image_{id}").into_bytes(),
                extension: "png".to_string(),
            });
        }

        let mut section = crate::model::document::Section::default();
        let mut para = Paragraph::default();
        para.text = "A".to_string();
        para.char_offsets = vec![8];
        para.char_count = 10;
        para.controls
            .push(Control::Shape(Box::new(ShapeObject::Group(group))));
        section.paragraphs.push(para);
        doc.sections.push(section);
        doc
    }

    /// 파싱된 문서의 첫 문단 controls 에서 GroupShape 를 찾아 반환.
    fn find_group(
        doc: &crate::model::document::Document,
    ) -> &crate::model::shape::GroupShape {
        use crate::model::control::Control;
        use crate::model::shape::ShapeObject;
        doc.sections[0].paragraphs[0]
            .controls
            .iter()
            .find_map(|c| match c {
                Control::Shape(s) => match s.as_ref() {
                    ShapeObject::Group(g) => Some(g),
                    _ => None,
                },
                _ => None,
            })
            .expect("container (GroupShape) must survive roundtrip")
    }

    fn make_child_picture(
        bin_data_id: u16,
        width: u32,
        height: u32,
        horz_offset: u32,
        vert_offset: u32,
    ) -> crate::model::image::Picture {
        use crate::model::image::{ImageAttr, Picture};
        Picture {
            common: CommonObjAttr {
                width,
                height,
                horizontal_offset: horz_offset,
                vertical_offset: vert_offset,
                ..Default::default()
            },
            image_attr: ImageAttr {
                bin_data_id,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// 묶음 개체(container) 안의 그림 2개가 (서로 다른 오프셋으로)
    /// serialize → parse 라운드트립에서 bin_data 참조·크기·오프셋까지 보존돼야 한다.
    #[test]
    fn container_with_two_pictures_roundtrips_through_hwpx() {
        use crate::model::shape::{GroupShape, ShapeObject};
        use crate::parser::hwpx::parse_hwpx;
        use crate::serializer::hwpx::serialize_hwpx;

        let group = GroupShape {
            common: CommonObjAttr {
                width: 20000,
                height: 10000,
                ..Default::default()
            },
            children: vec![
                ShapeObject::Picture(Box::new(make_child_picture(1, 5000, 3000, 100, 200))),
                ShapeObject::Picture(Box::new(make_child_picture(2, 7000, 4000, 6000, 1500))),
            ],
            ..Default::default()
        };
        let doc = doc_with_group(group, &[1, 2]);

        let bytes = serialize_hwpx(&doc).expect("serialize");
        let parsed = parse_hwpx(&bytes).expect("parse back");
        let group = find_group(&parsed);

        assert_eq!(
            group.common.width, 20000,
            "container sz must survive roundtrip"
        );
        assert_eq!(
            group.children.len(),
            2,
            "container must keep both picture children, got {:?}",
            group.children.len()
        );
        let pics: Vec<_> = group
            .children
            .iter()
            .filter_map(|ch| match ch {
                ShapeObject::Picture(p) => Some(p.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(pics.len(), 2, "both children must be pictures");
        let p1 = pics
            .iter()
            .find(|p| p.image_attr.bin_data_id == 1)
            .expect("picture with bin_data 1 must survive");
        let p2 = pics
            .iter()
            .find(|p| p.image_attr.bin_data_id == 2)
            .expect("picture with bin_data 2 must survive");
        assert_eq!((p1.common.width, p1.common.height), (5000, 3000));
        assert_eq!((p2.common.width, p2.common.height), (7000, 4000));
        assert_eq!(
            (p1.common.horizontal_offset, p1.common.vertical_offset),
            (100, 200),
            "child picture offsets must survive"
        );
        assert_eq!(
            (p2.common.horizontal_offset, p2.common.vertical_offset),
            (6000, 1500),
            "child picture offsets must survive"
        );
    }

    /// 묶음 개체 안에 그림 + 사각형이 섞여 있어도 두 자식 모두 보존돼야 한다.
    #[test]
    fn container_with_picture_and_rect_roundtrips_through_hwpx() {
        use crate::model::shape::{GroupShape, ShapeObject};
        use crate::parser::hwpx::parse_hwpx;
        use crate::serializer::hwpx::serialize_hwpx;

        let mut rect = RectangleShape::default();
        rect.common.width = 4000;
        rect.common.height = 2000;

        let group = GroupShape {
            common: CommonObjAttr {
                width: 15000,
                height: 8000,
                ..Default::default()
            },
            children: vec![
                ShapeObject::Picture(Box::new(make_child_picture(1, 5000, 3000, 0, 0))),
                ShapeObject::Rectangle(rect),
            ],
            ..Default::default()
        };
        let doc = doc_with_group(group, &[1]);

        let bytes = serialize_hwpx(&doc).expect("serialize");
        let parsed = parse_hwpx(&bytes).expect("parse back");
        let group = find_group(&parsed);

        assert_eq!(
            group.children.len(),
            2,
            "container must keep picture + rect children"
        );
        let pic = group
            .children
            .iter()
            .find_map(|ch| match ch {
                ShapeObject::Picture(p) => Some(p.as_ref()),
                _ => None,
            })
            .expect("picture child must survive");
        assert_eq!(pic.image_attr.bin_data_id, 1);
        assert_eq!((pic.common.width, pic.common.height), (5000, 3000));
        let rect = group
            .children
            .iter()
            .find_map(|ch| match ch {
                ShapeObject::Rectangle(r) => Some(r),
                _ => None,
            })
            .expect("rect child must survive");
        assert_eq!((rect.common.width, rect.common.height), (4000, 2000));
    }
}
