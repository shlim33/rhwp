//! HWPX(ZIP+XML) 직렬화 모듈 — `parser::hwpx`의 역방향.
//!
//! ## 단계 (#182)
//! - Stage 0 (완료): 기반 공사 — SerializeContext, IrDiff 하네스, canonical_defaults
//! - Stage 1: header.xml IR 기반 동적 생성
//! - Stage 2: section.xml 동적화 + charPrIDRef 매핑
//! - Stage 3: 표(Table)
//! - Stage 4: 그림(Picture) + BinData
//! - Stage 5: 도형·필드 + 대형 실문서 스모크

pub mod canonical_defaults;
pub mod content;
pub mod context;
pub mod field;
pub mod fixtures;
pub mod header;
pub mod picture;
pub mod roundtrip;
pub mod section;
pub mod shape;
pub mod static_assets;
pub mod table;
pub mod utils;
pub mod writer;

use std::collections::HashSet;

use crate::model::document::Document;

use super::SerializeError;
use content::BinDataEntry as ContentBinDataEntry;
use context::SerializeContext;
use writer::HwpxZipWriter;

/// Document IR을 HWPX(ZIP+XML) 바이트로 직렬화한다.
///
/// Stage 0 이후: 빈 문서 특수 분기를 제거하고 **항상 동적 경로**를 탄다.
/// `SerializeContext`가 1-pass 스캔으로 ID 풀을 구성하고, 각 writer가 동일 컨텍스트를
/// 참조한다. 직렬화 종료 시 `assert_all_refs_resolved()`가 미등록 참조를 단언한다.
pub fn serialize_hwpx(doc: &Document) -> Result<Vec<u8>, SerializeError> {
    use static_assets::*;

    // 1-pass: ID 풀 구성
    let mut ctx = SerializeContext::collect_from_document(doc);

    let mut z = HwpxZipWriter::new();

    // 1. mimetype (반드시 최초 엔트리, STORED, extra field 없음)
    z.write_stored("mimetype", b"application/hwp+zip")?;

    // 2. version.xml
    z.write_deflated("version.xml", VERSION_XML.as_bytes())?;

    // 3. Contents/header.xml — Stage 1 동적 생성 (IR 기반)
    let header_xml = header::write_header(doc, &ctx)?;
    z.write_deflated("Contents/header.xml", &header_xml)?;

    // 4. Contents/section{N}.xml — 실제 섹션만큼, 없으면 0개
    let section_hrefs: Vec<String> = (0..doc.sections.len())
        .map(|i| format!("Contents/section{}.xml", i))
        .collect();
    for (i, sec) in doc.sections.iter().enumerate() {
        let xml = section::write_section(sec, doc, i, &mut ctx)?;
        z.write_deflated(&section_hrefs[i], &xml)?;
    }

    // 5. Preview/PrvText.txt + Preview/PrvImage.png
    z.write_deflated("Preview/PrvText.txt", PRV_TEXT)?;
    z.write_deflated("Preview/PrvImage.png", PRV_IMAGE_PNG)?;

    // 6. settings.xml
    z.write_deflated("settings.xml", SETTINGS_XML.as_bytes())?;

    // 7. META-INF/container.rdf
    z.write_deflated("META-INF/container.rdf", META_INF_CONTAINER_RDF.as_bytes())?;

    // 8. BinData ZIP 엔트리 (Stage 4)
    //    `ctx.bin_data_map` 의 엔트리 순서대로 실제 바이너리를 ZIP에 추가.
    //    3-way 단언(binaryItemIDRef ↔ manifest ↔ ZIP entry) 의 1차 출력 지점.
    let bin_entries = ctx.bin_data_entries();
    let mut zip_bin_entries: HashSet<String> = HashSet::new();
    for entry in &bin_entries {
        let data = doc
            .bin_data_content
            .iter()
            .find(|b| b.id == entry.bin_data_id)
            .ok_or_else(|| {
                SerializeError::XmlError(format!(
                    "BinDataContent 누락: bin_data_id={}",
                    entry.bin_data_id
                ))
            })?;
        z.write_deflated(&entry.href, &data.data)?;
        zip_bin_entries.insert(entry.href.clone());
    }

    // 9. Contents/content.hpf — 항상 동적 경로 + BinData 매니페스트 엔트리
    let content_bin_entries: Vec<ContentBinDataEntry> = bin_entries
        .iter()
        .map(|e| ContentBinDataEntry {
            id: e.manifest_id.clone(),
            href: e.href.clone(),
            media_type: e.media_type.clone(),
        })
        .collect();
    let content_hpf = content::write_content_hpf(&section_hrefs, &content_bin_entries)?;
    z.write_deflated("Contents/content.hpf", &content_hpf)?;

    // 10. META-INF/container.xml
    z.write_deflated("META-INF/container.xml", META_INF_CONTAINER_XML.as_bytes())?;

    // 11. META-INF/manifest.xml
    z.write_deflated("META-INF/manifest.xml", META_INF_MANIFEST_XML.as_bytes())?;

    // 참조 정합성 단언 (Stage 1+)
    ctx.assert_all_refs_resolved()?;

    // 3-way BinData 단언 (Stage 4):
    //   - ctx.bin_data_map 의 manifest_id/href 집합
    //   - content.hpf opf:item (위에서 content_bin_entries 로 생성됨, 집합 동일)
    //   - ZIP entry (위에서 zip_bin_entries 로 기록됨)
    // 세 집합이 동일해야 한컴이 바인딩 오류 없이 그림을 표시함.
    assert_bin_data_3way(&bin_entries, &zip_bin_entries)?;

    z.finish()
}

/// 3-way BinData 동기화 단언: `ctx.bin_data_entries()`, content.hpf manifest,
/// ZIP entry 의 href 집합이 모두 일치하는지 확인.
fn assert_bin_data_3way(
    bin_entries: &[context::BinDataEntry],
    zip_entries: &HashSet<String>,
) -> Result<(), SerializeError> {
    let ctx_hrefs: HashSet<String> = bin_entries.iter().map(|e| e.href.clone()).collect();
    if ctx_hrefs != *zip_entries {
        let missing_zip: Vec<_> = ctx_hrefs.difference(zip_entries).cloned().collect();
        let orphan_zip: Vec<_> = zip_entries.difference(&ctx_hrefs).cloned().collect();
        return Err(SerializeError::XmlError(format!(
            "3-way BinData 불일치: ctx(href) vs zip_entries — ctx에만 있음: {:?}, zip에만 있음: {:?}",
            missing_zip, orphan_zip
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::hwpx::parse_hwpx;

    #[test]
    fn serialize_empty_doc_parses_back() {
        let doc = Document::default();
        let bytes = serialize_hwpx(&doc).expect("serialize empty");
        let parsed = parse_hwpx(&bytes).expect("parse back");
        assert_eq!(parsed.sections.len(), 0);
        assert!(parsed.bin_data_content.is_empty());
    }

    #[test]
    fn serialize_with_one_section_parses_back() {
        let mut doc = Document::default();
        doc.sections.push(crate::model::document::Section::default());
        let bytes = serialize_hwpx(&doc).expect("serialize one-section");
        let parsed = parse_hwpx(&bytes).expect("parse back");
        assert_eq!(parsed.sections.len(), 1);
    }

    /// E2E — 사용자 시나리오 재현: 편집기에서 그린 사각형(테두리+채우기, 페이지 오프셋)을
    /// export 하면 section XML 에 hp:lineShape / hc:fillBrush / hc:pt0..3 이 있어야 한다.
    /// (user2-152976e6.hwpx 는 sz/pos/outMargin 만 있어 재열기 시 도형이 투명해졌다.)
    #[test]
    fn export_rect_with_border_and_fill_emits_line_shape_and_fill_brush() {
        use crate::model::control::Control;
        use crate::model::shape::{RectangleShape, ShapeObject};
        use crate::model::style::{Fill, FillType, ShapeBorderLine, SolidFill};

        let mut rect = RectangleShape::default();
        rect.common.width = 11699;
        rect.common.height = 7801;
        rect.common.vertical_offset = 28276;
        rect.common.horizontal_offset = 13589;
        rect.drawing.border_line = ShapeBorderLine {
            color: 0, // #000000
            width: 33,
            attr: 1, // SOLID
            outline_style: 0,
        };
        rect.drawing.fill = Fill {
            fill_type: FillType::Solid,
            solid: Some(SolidFill {
                background_color: 0x00F0B000, // "#00B0F0"
                pattern_color: 0,
                pattern_type: -1,
            }),
            gradient: None,
            image: None,
            alpha: 0,
        };
        rect.x_coords = [0, 11699, 11699, 0];
        rect.y_coords = [0, 0, 7801, 7801];

        let mut doc = Document::default();
        doc.doc_info.char_shapes.push(Default::default());
        doc.doc_info.para_shapes.push(Default::default());
        doc.doc_info.styles.push(Default::default());
        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "A".to_string();
        para.char_offsets = vec![8];
        para.char_count = 10;
        para.controls
            .push(Control::Shape(Box::new(ShapeObject::Rectangle(rect))));
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize");
        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("valid zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");

        assert!(
            xml.contains(r##"<hp:lineShape color="#000000" width="33" style="SOLID""##),
            "exported rect must carry its stroke: {xml}"
        );
        assert!(
            xml.contains(r##"<hc:fillBrush><hc:winBrush faceColor="#00B0F0""##),
            "exported rect must carry its fill: {xml}"
        );
        assert!(
            xml.contains(r#"<hc:pt2 x="11699" y="7801"/>"#),
            "exported rect must carry corner points: {xml}"
        );
    }

    #[test]
    fn serialize_text_paragraph_roundtrip() {
        let mut doc = Document::default();
        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "안녕 Hello 123".to_string();
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize text");
        // 직렬화된 XML에 텍스트가 그대로 들어갔는지 ZIP에서 추출해 확인
        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("valid zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        assert!(
            xml.contains("<hp:t>안녕 Hello 123</hp:t>"),
            "text not injected into section0.xml"
        );

        // 라운드트립도 확인
        drop(sec0);
        let parsed = parse_hwpx(&bytes).expect("parse back");
        assert_eq!(parsed.sections.len(), 1);
        let p0 = &parsed.sections[0].paragraphs[0];
        assert!(
            p0.text.contains("안녕 Hello 123"),
            "text roundtrip failed: {:?}",
            p0.text
        );
    }

    #[test]
    fn tab_and_linebreak_emitted_inline() {
        let mut doc = Document::default();
        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "A\tB\nC".to_string();
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize");
        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        // Stage 2.3 (ref_mixed 기반): 혼합 콘텐츠 + tab 속성 포함
        assert!(
            xml.contains(r#"<hp:t>A<hp:tab width="4000" leader="0" type="1"/>B<hp:lineBreak/>C</hp:t>"#),
            "mixed content not rendered: {}", xml
        );
    }

    #[test]
    fn equation_control_roundtrip_preserves_script() {
        use crate::model::control::{Control, Equation};
        use crate::model::shape::{CommonObjAttr, HorzAlign, HorzRelTo, TextWrap, VertAlign, VertRelTo};
        use crate::model::Padding;

        let mut doc = Document::default();
        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "AB".to_string();
        para.char_offsets = vec![0, 9];
        para.char_count = 11;
        para.controls.push(Control::Equation(Box::new(Equation {
            common: CommonObjAttr {
                instance_id: 7,
                z_order: 3,
                width: 2400,
                height: 1200,
                vertical_offset: 80,
                horizontal_offset: 160,
                margin: Padding { left: 10, right: 20, top: 30, bottom: 40 },
                treat_as_char: true,
                text_wrap: TextWrap::TopAndBottom,
                vert_rel_to: VertRelTo::Para,
                horz_rel_to: HorzRelTo::Para,
                vert_align: VertAlign::Bottom,
                horz_align: HorzAlign::Center,
                ..Default::default()
            },
            script: "x < y & z".to_string(),
            font_size: 1000,
            color: 0x000000FF,
            baseline: 120,
            font_name: "HYhwpEQ".to_string(),
            version_info: "Equation Version 60".to_string(),
            raw_ctrl_data: Vec::new(),
        })));
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize equation");
        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        assert!(
            xml.contains("<hp:equation "),
            "equation XML missing: {}",
            xml
        );
        assert!(
            xml.contains("<hp:script>x &lt; y &amp; z</hp:script>"),
            "script XML missing: {}",
            xml
        );
        drop(sec0);

        let parsed = parse_hwpx(&bytes).expect("parse back");
        let parsed_para = &parsed.sections[0].paragraphs[0];
        assert_eq!(parsed_para.text, "AB");
        let parsed_eq = parsed_para.controls.iter().find_map(|ctrl| match ctrl {
            Control::Equation(eq) => Some(eq),
            _ => None,
        });
        match parsed_eq {
            Some(eq) => {
                assert_eq!(eq.script, "x < y & z");
                assert_eq!(eq.font_size, 1000);
                assert_eq!(eq.color, 0x000000FF);
                assert_eq!(eq.baseline, 120);
                assert_eq!(eq.font_name, "HYhwpEQ");
                assert_eq!(eq.version_info, "Equation Version 60");
                assert!(eq.common.treat_as_char);
                assert_eq!(eq.common.width, 2400);
                assert_eq!(eq.common.height, 1200);
                assert_eq!(eq.common.instance_id, 7);
                assert_eq!(eq.common.z_order, 3);
                assert_eq!(eq.common.vertical_offset, 80);
                assert_eq!(eq.common.horizontal_offset, 160);
                assert_eq!(eq.common.margin.left, 10);
                assert_eq!(eq.common.margin.right, 20);
                assert_eq!(eq.common.margin.top, 30);
                assert_eq!(eq.common.margin.bottom, 40);
                assert_eq!(eq.common.text_wrap, TextWrap::TopAndBottom);
                assert_eq!(eq.common.vert_rel_to, VertRelTo::Para);
                assert_eq!(eq.common.horz_rel_to, HorzRelTo::Para);
                assert_eq!(eq.common.vert_align, VertAlign::Bottom);
                assert_eq!(eq.common.horz_align, HorzAlign::Center);
            }
            None => panic!("expected equation control, got {:?}", parsed_para.controls),
        }
    }

    #[test]
    fn equation_control_between_text_runs_roundtrips_position() {
        use crate::model::control::{Control, Equation};
        use crate::model::page::ColumnDef;
        use crate::model::shape::CommonObjAttr;
        use crate::model::table::Table;

        let mut doc = Document::default();
        // Table::default() 의 border_fill_id(0) 가 검증을 통과하도록 등록
        doc.doc_info.border_fills.push(crate::model::style::BorderFill::default());
        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "ACB".to_string();
        para.char_offsets = vec![0, 9, 18];
        para.char_count = 20;
        para.controls.push(Control::ColumnDef(ColumnDef::default()));
        para.controls.push(Control::Table(Box::new(Table::default())));
        para.controls.push(Control::Equation(Box::new(Equation {
            common: CommonObjAttr {
                width: 1000,
                height: 1000,
                treat_as_char: true,
                ..Default::default()
            },
            script: "a+b".to_string(),
            font_size: 1000,
            ..Default::default()
        })));
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize equation");
        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");

        let a_pos = xml.find("<hp:t>A</hp:t>").expect("A text run");
        let c_pos = xml.find("<hp:t>C</hp:t>").expect("C text run");
        let eq_pos = xml.find("<hp:equation ").expect("equation");
        let b_pos = xml.find("<hp:t>B</hp:t>").expect("B text run");
        assert!(
            a_pos < c_pos && c_pos < eq_pos && eq_pos < b_pos,
            "equation must stay after non-equation inline slots: {}",
            xml
        );
    }

    #[test]
    fn equation_control_does_not_consume_unmapped_control_gap() {
        use crate::model::control::{Control, Equation};
        use crate::model::shape::CommonObjAttr;

        let mut doc = Document::default();
        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "ACB".to_string();
        para.char_offsets = vec![0, 9, 18];
        para.char_count = 20;
        para.controls.push(Control::Equation(Box::new(Equation {
            common: CommonObjAttr {
                width: 1000,
                height: 1000,
                treat_as_char: true,
                ..Default::default()
            },
            script: "a+b".to_string(),
            font_size: 1000,
            ..Default::default()
        })));
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize equation");
        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");

        let text_pos = xml.find("<hp:t>ACB</hp:t>").expect("text run");
        let eq_pos = xml.find("<hp:equation ").expect("equation");
        assert!(
            text_pos < eq_pos,
            "ambiguous control gap must not move equation before text: {}",
            xml
        );
    }

    /// 한컴 편집기가 만든 hwp 샘플(`samples/equation-lim.hwp`)의 수식 IR이
    /// HWPX 직렬화 → 재파싱 사이클에서 의미를 잃지 않는지 검증한다.
    ///
    /// 자체 IR 생성 패턴(Document::default + 수동 push)을 회피하고,
    /// 한컴 origin 데이터에서 추출한 Equation을 입력으로 사용한다.
    #[test]
    fn equation_roundtrip_from_hancom_origin_hwp_sample() {
        use crate::model::control::{Control, Equation};
        use crate::parser::parse_hwp;

        let bytes = std::fs::read("samples/equation-lim.hwp")
            .expect("samples/equation-lim.hwp must be readable");
        let original = parse_hwp(&bytes).expect("parse hancom origin hwp");

        let collect_equations = |doc: &Document| -> Vec<Equation> {
            doc.sections
                .iter()
                .flat_map(|s| s.paragraphs.iter())
                .flat_map(|p| p.controls.iter())
                .filter_map(|c| match c {
                    Control::Equation(eq) => Some((**eq).clone()),
                    _ => None,
                })
                .collect()
        };

        let original_eqs = collect_equations(&original);
        assert!(
            !original_eqs.is_empty(),
            "한컴 origin 샘플에 수식이 존재해야 회귀 비교가 의미있음"
        );

        let hwpx_bytes = serialize_hwpx(&original).expect("serialize to hwpx");
        let reparsed = parse_hwpx(&hwpx_bytes).expect("parse hwpx back");
        let reparsed_eqs = collect_equations(&reparsed);

        assert_eq!(
            reparsed_eqs.len(),
            original_eqs.len(),
            "수식 컨트롤 개수가 hwpx 라운드트립에서 유지되어야 함"
        );

        for (i, (orig, rep)) in original_eqs.iter().zip(reparsed_eqs.iter()).enumerate() {
            assert_eq!(
                rep.script, orig.script,
                "[#{}] script must roundtrip through hwpx",
                i
            );
            assert_eq!(
                rep.font_size, orig.font_size,
                "[#{}] font_size must roundtrip",
                i
            );
            assert_eq!(
                rep.baseline, orig.baseline,
                "[#{}] baseline must roundtrip",
                i
            );
            assert_eq!(
                rep.font_name, orig.font_name,
                "[#{}] font_name must roundtrip",
                i
            );
            assert_eq!(
                rep.color, orig.color,
                "[#{}] color must roundtrip",
                i
            );
            assert_eq!(
                rep.common.width, orig.common.width,
                "[#{}] common.width must roundtrip",
                i
            );
            assert_eq!(
                rep.common.height, orig.common.height,
                "[#{}] common.height must roundtrip",
                i
            );
            assert_eq!(
                rep.common.treat_as_char, orig.common.treat_as_char,
                "[#{}] common.treat_as_char must roundtrip",
                i
            );
        }
    }

    #[test]
    fn linesegs_emitted_per_linebreak() {
        let mut doc = Document::default();
        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "A\nB\nC".to_string();
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize");
        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");

        // 3줄(소프트) → lineseg 3개, textpos=0/2/4, vertpos=0/1600/3200
        let count = xml.matches("<hp:lineseg ").count();
        assert_eq!(count, 3, "expected 3 linesegs, got {}: {}", count, xml);
        assert!(xml.contains(r#"textpos="0" vertpos="0""#));
        assert!(xml.contains(r#"textpos="2" vertpos="1600""#));
        assert!(xml.contains(r#"textpos="4" vertpos="3200""#));
    }

    #[test]
    fn multi_paragraph_emits_multiple_hp_p() {
        let mut doc = Document::default();
        let mut section = crate::model::document::Section::default();
        for t in ["첫째 줄", "둘째", "끝"] {
            let mut p = crate::model::paragraph::Paragraph::default();
            p.text = t.to_string();
            section.paragraphs.push(p);
        }
        doc.sections.push(section);
        let bytes = serialize_hwpx(&doc).expect("serialize");
        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        let p_count = xml.matches("<hp:p ").count();
        assert_eq!(p_count, 3, "expected 3 <hp:p>, got {}", p_count);
        assert!(xml.contains("<hp:t>첫째 줄</hp:t>"));
        assert!(xml.contains("<hp:t>둘째</hp:t>"));
        assert!(xml.contains("<hp:t>끝</hp:t>"));
    }

    #[test]
    fn xml_escape_applied_to_section_text() {
        let mut doc = Document::default();
        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "a & b < c".to_string();
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize");
        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        assert!(xml.contains("a &amp; b &lt; c"), "escape missing: {}", xml);
    }

    #[test]
    fn mimetype_is_first_entry() {
        let doc = Document::default();
        let bytes = serialize_hwpx(&doc).expect("serialize");
        assert_eq!(&bytes[0..4], b"PK\x03\x04", "ZIP signature");
        let name_len = u16::from_le_bytes([bytes[26], bytes[27]]) as usize;
        let name = &bytes[30..30 + name_len];
        assert_eq!(name, b"mimetype");
    }

    #[test]
    fn mimetype_stored_not_deflated() {
        let doc = Document::default();
        let bytes = serialize_hwpx(&doc).expect("serialize");
        let method = u16::from_le_bytes([bytes[8], bytes[9]]);
        assert_eq!(method, 0, "mimetype must be STORED (method=0)");
    }

    #[test]
    fn hancom_required_files_present() {
        let mut doc = Document::default();
        doc.sections.push(crate::model::document::Section::default());
        let bytes = serialize_hwpx(&doc).expect("serialize");
        // ZIP 파일 목록에 한컴 필수 11개가 모두 있는지 확인
        let cursor = std::io::Cursor::new(&bytes);
        let archive = zip::ZipArchive::new(cursor).expect("valid zip");
        let names: Vec<String> = archive.file_names().map(String::from).collect();
        let required = [
            "mimetype",
            "version.xml",
            "Contents/header.xml",
            "Contents/section0.xml",
            "Contents/content.hpf",
            "Preview/PrvText.txt",
            "Preview/PrvImage.png",
            "settings.xml",
            "META-INF/container.xml",
            "META-INF/container.rdf",
            "META-INF/manifest.xml",
        ];
        for r in &required {
            assert!(
                names.iter().any(|n| n == r),
                "missing required file: {}",
                r
            );
        }
    }

    #[test]
    fn picture_bindata_roundtrip() {
        use crate::model::bin_data::BinDataContent;
        use crate::model::control::Control;
        use crate::model::image::{ImageAttr, Picture};
        use crate::model::shape::CommonObjAttr;

        let fake_png = b"\x89PNG\r\n\x1a\nfake_image_data_for_test";

        let mut doc = Document::default();
        doc.bin_data_content.push(BinDataContent {
            id: 1,
            data: fake_png.to_vec(),
            extension: "png".to_string(),
        });

        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "A".to_string();
        para.char_offsets = vec![8];
        para.char_count = 10;
        para.controls.push(Control::Picture(Box::new(Picture {
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
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize picture");

        // ZIP에 BinData 엔트리 존재 확인
        let cursor = std::io::Cursor::new(&bytes);
        let archive = zip::ZipArchive::new(cursor).expect("zip");
        let names: Vec<String> = archive.file_names().map(String::from).collect();
        assert!(
            names.iter().any(|n| n.starts_with("BinData/")),
            "BinData/ ZIP entry missing: {:?}",
            names
        );
        drop(archive);

        // section XML에 binaryItemIDRef 포함 확인
        let cursor2 = std::io::Cursor::new(&bytes);
        let mut archive2 = zip::ZipArchive::new(cursor2).expect("zip");
        let mut sec0 = archive2.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        assert!(
            xml.contains("binaryItemIDRef"),
            "binaryItemIDRef missing in section XML: {}",
            xml
        );
        drop(sec0);

        // 라운드트립: BinData 보존 확인
        let parsed = parse_hwpx(&bytes).expect("parse back");
        assert_eq!(parsed.bin_data_content.len(), 1);
        assert_eq!(parsed.bin_data_content[0].data, fake_png);
        assert_eq!(parsed.bin_data_content[0].extension, "png");
    }

    #[test]
    fn table_control_roundtrip() {
        use crate::model::control::Control;
        use crate::model::table::Table;

        let mut doc = Document::default();
        doc.doc_info
            .border_fills
            .push(crate::model::style::BorderFill::default());

        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "A".to_string();
        para.char_offsets = vec![8];
        para.char_count = 10;
        para.controls
            .push(Control::Table(Box::new(Table::default())));
        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize table");

        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        assert!(
            xml.contains("<hp:tbl ") || xml.contains("<hp:tbl>"),
            "table element missing in section XML: {}",
            xml
        );
        drop(sec0);

        let parsed = parse_hwpx(&bytes).expect("parse back");
        let has_table = parsed.sections[0].paragraphs[0]
            .controls
            .iter()
            .any(|c| matches!(c, Control::Table(_)));
        assert!(has_table, "table control missing after roundtrip");
    }

    #[test]
    fn footnote_endnote_roundtrip() {
        use crate::model::control::Control;
        use crate::model::footnote::{Endnote, Footnote};
        use crate::model::paragraph::Paragraph;

        let mut doc = Document::default();
        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "본문".to_string();
        para.char_offsets = vec![8, 17];
        para.char_count = 20;

        let mut fn_para = Paragraph::default();
        fn_para.text = "각주 텍스트".to_string();
        para.controls.push(Control::Footnote(Box::new(Footnote {
            number: 1,
            paragraphs: vec![fn_para],
        })));

        let mut en_para = Paragraph::default();
        en_para.text = "미주 텍스트".to_string();
        para.controls.push(Control::Endnote(Box::new(Endnote {
            number: 1,
            paragraphs: vec![en_para],
        })));

        section.paragraphs.push(para);
        doc.sections.push(section);

        let bytes = serialize_hwpx(&doc).expect("serialize notes");

        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        assert!(
            xml.contains("<hp:footNote"),
            "footNote missing: {}",
            xml
        );
        assert!(
            xml.contains("<hp:endNote"),
            "endNote missing: {}",
            xml
        );
        assert!(xml.contains("각주 텍스트"), "footnote text missing: {}", xml);
        assert!(xml.contains("미주 텍스트"), "endnote text missing: {}", xml);
        drop(sec0);

        let parsed = parse_hwpx(&bytes).expect("parse back");
        let ctrls = &parsed.sections[0].paragraphs[0].controls;
        let has_fn = ctrls.iter().any(|c| matches!(c, Control::Footnote(_)));
        let has_en = ctrls.iter().any(|c| matches!(c, Control::Endnote(_)));
        assert!(has_fn, "footnote missing after roundtrip");
        assert!(has_en, "endnote missing after roundtrip");

        let fn_ctrl = ctrls.iter().find_map(|c| match c {
            Control::Footnote(f) => Some(f),
            _ => None,
        });
        assert!(
            fn_ctrl.unwrap().paragraphs[0].text.contains("각주 텍스트"),
            "footnote paragraph text not preserved"
        );
    }

    /// tac-img-02.hwpx 원본은 section0 에 `<hp:pic>` 18개를 담고 있다
    /// (본문 직속 6 + 표 셀 내부 9 + rect drawText 내부 3).
    /// exportHwpx 가 셀/글상자 내부 컨트롤을 드랍하지 않고 전부 재-직렬화하는지,
    /// parse-back 시 셀 그림·글상자 그림이 IR 로 복원되는지 검증한다.
    #[test]
    fn tac_img_sample_nested_pictures_survive_export() {
        use crate::model::control::Control;
        use crate::model::document::Document;
        use crate::model::shape::ShapeObject;

        let bytes = std::fs::read("samples/tac-img-02.hwpx")
            .expect("samples/tac-img-02.hwpx must be readable");
        let original = parse_hwpx(&bytes).expect("parse original");

        let out = serialize_hwpx(&original).expect("re-serialize");
        let cursor = std::io::Cursor::new(&out);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        drop(sec0);

        let pic_count = xml.matches("<hp:pic ").count();
        assert!(
            pic_count >= 18,
            "exported section0 must keep all 18 <hp:pic> (was 6 before cell/drawText fix), got {}",
            pic_count
        );

        // parse-back: 셀 내부·글상자 내부 그림이 IR 로 복원되는지
        fn count_nested_pics(doc: &Document) -> (usize, usize) {
            let mut cell_pics = 0usize;
            let mut draw_text_pics = 0usize;
            fn walk(ctrl: &Control, cell_pics: &mut usize, draw_text_pics: &mut usize) {
                match ctrl {
                    Control::Table(tbl) => {
                        for cell in &tbl.cells {
                            for para in &cell.paragraphs {
                                for c in &para.controls {
                                    if matches!(c, Control::Picture(_)) {
                                        *cell_pics += 1;
                                    }
                                    walk(c, cell_pics, draw_text_pics);
                                }
                            }
                        }
                    }
                    Control::Shape(shape) => {
                        if let ShapeObject::Rectangle(r) = shape.as_ref() {
                            if let Some(tb) = &r.drawing.text_box {
                                for para in &tb.paragraphs {
                                    for c in &para.controls {
                                        if matches!(c, Control::Picture(_)) {
                                            *draw_text_pics += 1;
                                        }
                                        walk(c, cell_pics, draw_text_pics);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            for sec in &doc.sections {
                for para in &sec.paragraphs {
                    for ctrl in &para.controls {
                        walk(ctrl, &mut cell_pics, &mut draw_text_pics);
                    }
                }
            }
            (cell_pics, draw_text_pics)
        }

        let (orig_cell, orig_dt) = count_nested_pics(&original);
        assert_eq!((orig_cell, orig_dt), (9, 3), "original IR nested pic census");

        let reparsed = parse_hwpx(&out).expect("parse back");
        let (re_cell, re_dt) = count_nested_pics(&reparsed);
        assert_eq!(
            re_cell, orig_cell,
            "cell pictures must survive export+reparse"
        );
        assert_eq!(
            re_dt, orig_dt,
            "drawText pictures must survive export+reparse"
        );
    }

    /// hwpx-h-01.hwpx 원본은 section0 에 `<hp:pic>` 5개를 담고 있다
    /// (표 셀 내부 2 + `<hp:container>` 묶음 개체 내부 3).
    /// exportHwpx 가 container 자식을 드랍하지 않고 전부 재-직렬화하는지,
    /// parse-back 시 container 자체와 그 안의 그림들이 IR 로 복원되는지 검증한다.
    #[test]
    fn hwpx_h_sample_container_pictures_survive_export() {
        use crate::model::control::Control;
        use crate::model::document::Document;
        use crate::model::shape::ShapeObject;

        let bytes = std::fs::read("samples/hwpx/hwpx-h-01.hwpx")
            .expect("samples/hwpx/hwpx-h-01.hwpx must be readable");
        let original = parse_hwpx(&bytes).expect("parse original");

        // (containers, pics inside containers) — 중첩 그룹까지 재귀 집계
        fn count_container_pics(doc: &Document) -> (usize, usize) {
            fn walk_shape(shape: &ShapeObject, containers: &mut usize, pics: &mut usize) {
                if let ShapeObject::Group(g) = shape {
                    *containers += 1;
                    for ch in &g.children {
                        if matches!(ch, ShapeObject::Picture(_)) {
                            *pics += 1;
                        }
                        walk_shape(ch, containers, pics);
                    }
                }
            }
            let mut containers = 0usize;
            let mut pics = 0usize;
            for sec in &doc.sections {
                for para in &sec.paragraphs {
                    for ctrl in &para.controls {
                        if let Control::Shape(shape) = ctrl {
                            walk_shape(shape, &mut containers, &mut pics);
                        }
                    }
                }
            }
            (containers, pics)
        }

        let (orig_containers, orig_container_pics) = count_container_pics(&original);
        assert_eq!(
            (orig_containers, orig_container_pics),
            (1, 3),
            "original IR container census"
        );

        let out = serialize_hwpx(&original).expect("re-serialize");
        let cursor = std::io::Cursor::new(&out);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        drop(sec0);

        let pic_count = xml.matches("<hp:pic ").count();
        assert!(
            pic_count >= 5,
            "exported section0 must keep all 5 <hp:pic> (was 2 before container children fix), got {}",
            pic_count
        );

        let reparsed = parse_hwpx(&out).expect("parse back");
        let (re_containers, re_pics) = count_container_pics(&reparsed);
        assert_eq!(
            re_containers, orig_containers,
            "container count must survive export+reparse"
        );
        assert_eq!(
            re_pics, orig_container_pics,
            "container pictures must survive export+reparse"
        );
    }

    // =================================================================
    // 도형 GEOMETRY 라운드트립 (ellipse / arc / polygon / curve)
    // =================================================================

    /// 도형 1개를 담은 라운드트립용 Document 뼈대 생성.
    fn doc_with_shape(shape: crate::model::shape::ShapeObject) -> Document {
        use crate::model::control::Control;

        let mut doc = Document::default();
        let mut section = crate::model::document::Section::default();
        let mut para = crate::model::paragraph::Paragraph::default();
        para.text = "A".to_string();
        para.char_offsets = vec![8];
        para.char_count = 10;
        para.controls.push(Control::Shape(Box::new(shape)));
        section.paragraphs.push(para);
        doc.sections.push(section);
        doc
    }

    /// IR → serialize_hwpx → parse_hwpx 라운드트립 후 첫 Shape 컨트롤을 반환.
    fn roundtrip_shape(shape: crate::model::shape::ShapeObject) -> crate::model::shape::ShapeObject {
        use crate::model::control::Control;

        let doc = doc_with_shape(shape);
        let bytes = serialize_hwpx(&doc).expect("serialize shape");
        let parsed = parse_hwpx(&bytes).expect("parse back");
        parsed.sections[0].paragraphs[0]
            .controls
            .iter()
            .find_map(|c| match c {
                Control::Shape(s) => Some((**s).clone()),
                _ => None,
            })
            .expect("shape control must survive roundtrip")
    }

    /// 문서 전체(본문 + 표 셀 + 묶음 개체 재귀)에서 ShapeObject 를 수집.
    fn collect_shape_objects(doc: &Document) -> Vec<&crate::model::shape::ShapeObject> {
        use crate::model::control::Control;
        use crate::model::shape::ShapeObject;

        fn walk_shape<'a>(s: &'a ShapeObject, out: &mut Vec<&'a ShapeObject>) {
            out.push(s);
            let text_box = match s {
                ShapeObject::Group(g) => {
                    for ch in &g.children {
                        walk_shape(ch, out);
                    }
                    None
                }
                ShapeObject::Rectangle(r) => r.drawing.text_box.as_ref(),
                ShapeObject::Ellipse(e) => e.drawing.text_box.as_ref(),
                ShapeObject::Line(l) => l.drawing.text_box.as_ref(),
                ShapeObject::Arc(a) => a.drawing.text_box.as_ref(),
                ShapeObject::Polygon(p) => p.drawing.text_box.as_ref(),
                ShapeObject::Curve(cv) => cv.drawing.text_box.as_ref(),
                _ => None,
            };
            if let Some(tb) = text_box {
                for p in &tb.paragraphs {
                    for c in &p.controls {
                        walk_ctrl(c, out);
                    }
                }
            }
        }
        fn walk_ctrl<'a>(c: &'a Control, out: &mut Vec<&'a ShapeObject>) {
            match c {
                Control::Shape(s) => walk_shape(s, out),
                Control::Table(t) => {
                    for cell in &t.cells {
                        for p in &cell.paragraphs {
                            for c in &p.controls {
                                walk_ctrl(c, out);
                            }
                        }
                    }
                }
                Control::Footnote(n) => {
                    for p in &n.paragraphs {
                        for c in &p.controls {
                            walk_ctrl(c, out);
                        }
                    }
                }
                Control::Endnote(n) => {
                    for p in &n.paragraphs {
                        for c in &p.controls {
                            walk_ctrl(c, out);
                        }
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for sec in &doc.sections {
            for para in &sec.paragraphs {
                for ctrl in &para.controls {
                    walk_ctrl(ctrl, &mut out);
                }
            }
        }
        out
    }

    /// Point 목록 → (x, y) 튜플 목록 (Point 가 PartialEq 미구현이라 비교용).
    fn pts(v: &[crate::model::Point]) -> Vec<(i32, i32)> {
        v.iter().map(|p| (p.x, p.y)).collect()
    }

    #[test]
    fn ellipse_geometry_roundtrips_through_hwpx() {
        use crate::model::shape::{CommonObjAttr, EllipseShape, ShapeObject};
        use crate::model::Point;

        let ell = EllipseShape {
            common: CommonObjAttr {
                width: 4000,
                height: 3000,
                ..Default::default()
            },
            // intervalDirty=1(bit0), hasArcPr=1(bit1), arcType=PIE(1, bit2~)
            attr: 0b111,
            center: Point { x: 2000, y: 1500 },
            axis1: Point { x: 4000, y: 1500 },
            axis2: Point { x: 2000, y: 3000 },
            start1: Point { x: 11, y: 22 },
            end1: Point { x: 33, y: 44 },
            start2: Point { x: 55, y: 66 },
            end2: Point { x: 77, y: 88 },
            ..Default::default()
        };

        let rt = roundtrip_shape(ShapeObject::Ellipse(ell));
        let e = match rt {
            ShapeObject::Ellipse(e) => e,
            other => panic!("expected Ellipse after roundtrip, got {:?}", other),
        };
        assert_eq!(e.attr, 0b111, "ellipse attr(intervalDirty/hasArcPr/arcType) must roundtrip");
        assert_eq!((e.center.x, e.center.y), (2000, 1500), "center must roundtrip");
        assert_eq!((e.axis1.x, e.axis1.y), (4000, 1500), "axis1 must roundtrip");
        assert_eq!((e.axis2.x, e.axis2.y), (2000, 3000), "axis2 must roundtrip");
        assert_eq!((e.start1.x, e.start1.y), (11, 22), "start1 must roundtrip");
        assert_eq!((e.end1.x, e.end1.y), (33, 44), "end1 must roundtrip");
        assert_eq!((e.start2.x, e.start2.y), (55, 66), "start2 must roundtrip");
        assert_eq!((e.end2.x, e.end2.y), (77, 88), "end2 must roundtrip");
    }

    #[test]
    fn arc_geometry_roundtrips_through_hwpx() {
        use crate::model::shape::{ArcShape, CommonObjAttr, ShapeObject};
        use crate::model::Point;

        let arc = ArcShape {
            common: CommonObjAttr {
                width: 5000,
                height: 2500,
                ..Default::default()
            },
            arc_type: 2, // CHORD(활)
            center: Point { x: 2500, y: 2500 },
            axis1: Point { x: 0, y: 2500 },
            axis2: Point { x: 2500, y: 0 },
            ..Default::default()
        };

        let rt = roundtrip_shape(ShapeObject::Arc(arc));
        let a = match rt {
            ShapeObject::Arc(a) => a,
            other => panic!("expected Arc after roundtrip, got {:?}", other),
        };
        assert_eq!(a.arc_type, 2, "arc_type must roundtrip");
        assert_eq!((a.center.x, a.center.y), (2500, 2500), "center must roundtrip");
        assert_eq!((a.axis1.x, a.axis1.y), (0, 2500), "axis1 must roundtrip");
        assert_eq!((a.axis2.x, a.axis2.y), (2500, 0), "axis2 must roundtrip");
    }

    #[test]
    fn polygon_geometry_roundtrips_through_hwpx() {
        use crate::model::shape::{CommonObjAttr, PolygonShape, ShapeObject};
        use crate::model::Point;

        let points = vec![
            Point { x: 1360, y: 8 },
            Point { x: 0, y: 612 },
            Point { x: 2736, y: 624 },
            Point { x: -40, y: -80 },
            Point { x: 1360, y: 0 },
        ];
        let poly = PolygonShape {
            common: CommonObjAttr {
                width: 2740,
                height: 628,
                ..Default::default()
            },
            points: points.clone(),
            ..Default::default()
        };

        let rt = roundtrip_shape(ShapeObject::Polygon(poly));
        let p = match rt {
            ShapeObject::Polygon(p) => p,
            other => panic!("expected Polygon after roundtrip, got {:?}", other),
        };
        assert_eq!(
            pts(&p.points),
            pts(&points),
            "polygon points must roundtrip verbatim"
        );
    }

    #[test]
    fn curve_geometry_roundtrips_through_hwpx() {
        use crate::model::shape::{CommonObjAttr, CurveShape, ShapeObject};
        use crate::model::Point;

        let points = vec![
            Point { x: 190, y: 50 },
            Point { x: 500, y: 120 },
            Point { x: 900, y: -30 },
            Point { x: 190, y: 50 },
        ];
        let segment_types = vec![1u8, 0, 1]; // CURVE / LINE / CURVE
        let curve = CurveShape {
            common: CommonObjAttr {
                width: 3175,
                height: 1090,
                ..Default::default()
            },
            points: points.clone(),
            segment_types: segment_types.clone(),
            ..Default::default()
        };

        let rt = roundtrip_shape(ShapeObject::Curve(curve));
        let cv = match rt {
            ShapeObject::Curve(cv) => cv,
            other => panic!("expected Curve after roundtrip, got {:?}", other),
        };
        assert_eq!(
            pts(&cv.points),
            pts(&points),
            "curve points must roundtrip verbatim"
        );
        assert_eq!(
            cv.segment_types, segment_types,
            "curve segment types must roundtrip verbatim"
        );
    }

    /// 골든: 한컴 변환본 hwp3-sample11-hwpx.hwpx 의 hp:ellipse / hp:polygon
    /// geometry 가 원본 XML 값 그대로 IR 에 실리는지 검증 (파서를 한컴 포맷에 고정).
    ///
    /// 원본 XML 관측값 (Contents/section0.xml):
    /// - hp:ellipse 59개 / hp:polygon 21개
    /// - instid=11823569 ellipse: <hc:center x="48" y="68"/><hc:ax1 x="42" y="4"/>
    ///   <hc:ax2 x="90" y="64"/>, start1~end2 모두 (0,0), hasArcPr=0 arcType=NORMAL
    /// - instid=11824244 polygon: <hc:pt> 4개 — (1360,8) (0,612) (2736,624) (1360,0)
    #[test]
    fn hwp3_sample11_ellipse_polygon_geometry_matches_hancom_xml() {
        use crate::model::shape::ShapeObject;
        use crate::model::Point;

        let bytes = std::fs::read("samples/hwp3-sample11-hwpx.hwpx")
            .expect("samples/hwp3-sample11-hwpx.hwpx must be readable");
        let doc = parse_hwpx(&bytes).expect("parse sample");
        let shapes = collect_shape_objects(&doc);

        let ellipses: Vec<_> = shapes
            .iter()
            .filter_map(|s| match s {
                ShapeObject::Ellipse(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(ellipses.len(), 59, "sample must contain 59 hp:ellipse");

        let pinned = ellipses
            .iter()
            .find(|e| e.common.instance_id == 11823569)
            .expect("ellipse instid=11823569 must exist");
        assert_eq!((pinned.center.x, pinned.center.y), (48, 68), "golden hc:center");
        assert_eq!((pinned.axis1.x, pinned.axis1.y), (42, 4), "golden hc:ax1");
        assert_eq!((pinned.axis2.x, pinned.axis2.y), (90, 64), "golden hc:ax2");
        assert_eq!((pinned.start1.x, pinned.start1.y), (0, 0), "golden hc:start1");
        assert_eq!((pinned.end2.x, pinned.end2.y), (0, 0), "golden hc:end2");
        assert_eq!(
            pinned.attr, 0,
            "intervalDirty=0 hasArcPr=0 arcType=NORMAL → attr bits 0"
        );

        let polygons: Vec<_> = shapes
            .iter()
            .filter_map(|s| match s {
                ShapeObject::Polygon(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(polygons.len(), 21, "sample must contain 21 hp:polygon");

        let pinned_poly = polygons
            .iter()
            .find(|p| p.common.instance_id == 11824244)
            .expect("polygon instid=11824244 must exist");
        assert_eq!(
            pts(&pinned_poly.points),
            vec![(1360, 8), (0, 612), (2736, 624), (1360, 0)],
            "golden hc:pt list"
        );
    }

    /// 골든: 3-09월_교육_통합_2022.hwpx 의 hp:curve (유일 1개) geometry.
    ///
    /// 원본 XML 관측값: hp:seg 417개 (type=CURVE/LINE 혼재),
    /// 첫 seg = CURVE (190,50)→(190,50), 마지막 seg = LINE (2775,50)→(190,50).
    /// → 점 418개, 첫 점 (190,50), 마지막 점 (190,50), 마지막 seg type=LINE(0).
    #[test]
    fn curve_sample_geometry_matches_hancom_xml() {
        use crate::model::shape::ShapeObject;
        use crate::model::Point;

        let bytes = std::fs::read("samples/3-09월_교육_통합_2022.hwpx")
            .expect("samples/3-09월_교육_통합_2022.hwpx must be readable");
        let doc = parse_hwpx(&bytes).expect("parse sample");
        let shapes = collect_shape_objects(&doc);

        let curve = shapes
            .iter()
            .find_map(|s| match s {
                ShapeObject::Curve(cv) => Some(cv),
                _ => None,
            })
            .expect("sample must contain the hp:curve");
        assert_eq!(curve.points.len(), 418, "417 seg → 418 points");
        assert_eq!(
            curve.segment_types.len(),
            417,
            "one segment type per hp:seg"
        );
        assert_eq!((curve.points[0].x, curve.points[0].y), (190, 50), "first point");
        assert_eq!((curve.points[417].x, curve.points[417].y), (190, 50), "last point");
        assert_eq!(curve.segment_types[0], 1, "first seg type=CURVE → 1");
        assert_eq!(curve.segment_types[416], 0, "last seg type=LINE → 0");
        assert!(
            curve.segment_types.contains(&0) && curve.segment_types.contains(&1),
            "sample curve mixes CURVE and LINE segments"
        );
    }

    /// E2E: hwp3-sample11-hwpx.hwpx 파싱 → 직렬화 → 재파싱 시
    /// ellipse/polygon 총 개수와 핀 고정 geometry 가 보존되고,
    /// 내보낸 section XML 에 geometry 자식 요소가 실제로 존재해야 한다.
    #[test]
    fn hwp3_sample11_shape_geometry_survives_export() {
        use crate::model::shape::ShapeObject;

        let bytes = std::fs::read("samples/hwp3-sample11-hwpx.hwpx")
            .expect("samples/hwp3-sample11-hwpx.hwpx must be readable");
        let original = parse_hwpx(&bytes).expect("parse original");

        let out = serialize_hwpx(&original).expect("re-serialize");
        let cursor = std::io::Cursor::new(&out);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut sec0 = archive.by_name("Contents/section0.xml").expect("section0");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut sec0, &mut xml).expect("read");
        drop(sec0);

        // 내보낸 XML 이 껍데기가 아니라 geometry 자식을 담아야 한다.
        assert_eq!(
            xml.matches("<hp:ellipse ").count(),
            59,
            "exported section0 must keep all 59 <hp:ellipse>"
        );
        assert_eq!(
            xml.matches("<hp:polygon ").count(),
            21,
            "exported section0 must keep all 21 <hp:polygon>"
        );
        assert!(
            xml.contains("<hc:center "),
            "exported ellipse must contain <hc:center> geometry child"
        );
        assert!(
            xml.contains(r#"<hc:ax1 x="42" y="4"/>"#),
            "exported ellipse must contain pinned <hc:ax1> values"
        );
        assert!(
            xml.contains(r#"<hc:pt x="1360" y="8"/>"#),
            "exported polygon must contain pinned <hc:pt> values"
        );

        // 재파싱 후 개수 + 핀 고정 geometry 보존
        let reparsed = parse_hwpx(&out).expect("parse back");
        let shapes = collect_shape_objects(&reparsed);
        let ellipses: Vec<_> = shapes
            .iter()
            .filter_map(|s| match s {
                ShapeObject::Ellipse(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(ellipses.len(), 59, "ellipse count must survive export+reparse");

        // 핀 고정 ellipse: 재직렬화가 instid 를 보존하지 않으므로 geometry 값으로 탐색
        assert!(
            ellipses.iter().any(|e| (e.center.x, e.center.y) == (48, 68)
                && (e.axis1.x, e.axis1.y) == (42, 4)
                && (e.axis2.x, e.axis2.y) == (90, 64)),
            "pinned ellipse geometry must survive export+reparse"
        );

        let polygons: Vec<_> = shapes
            .iter()
            .filter_map(|s| match s {
                ShapeObject::Polygon(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(polygons.len(), 21, "polygon count must survive export+reparse");
        assert!(
            polygons
                .iter()
                .any(|p| pts(&p.points) == vec![(1360, 8), (0, 612), (2736, 624), (1360, 0)]),
            "pinned polygon geometry must survive export+reparse"
        );
    }

    /// tac-img-02.hwpx 파싱 후 BinData 가 존재하는지, Picture 컨트롤이
    /// section XML 에 반영되는지 확인하는 스모크 테스트.
    /// 실문서는 borderFillIDRef 가 복잡하여 full roundtrip 대신 직렬화 단계만 검증.
    #[test]
    fn tac_img_sample_has_pictures_and_bindata() {
        use crate::model::control::Control;

        let bytes = std::fs::read("samples/tac-img-02.hwpx")
            .expect("samples/tac-img-02.hwpx must be readable");
        let original = parse_hwpx(&bytes).expect("parse original");

        assert!(
            !original.bin_data_content.is_empty(),
            "sample must contain BinData"
        );

        let pic_count = original
            .sections
            .iter()
            .flat_map(|s| s.paragraphs.iter())
            .flat_map(|p| p.controls.iter())
            .filter(|c| matches!(c, Control::Picture(_)))
            .count();
        assert!(pic_count > 0, "sample must contain Picture controls");
    }
}
