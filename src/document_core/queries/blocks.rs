//! 문서 순서 블록 추출 — 문서 처리 파이프라인(RAG 적재 등)이 본문·제목·표·그림을
//! **문서 모델 순서 그대로**, 문단 주소와 렌더 페이지를 붙여 소비하기 위한 읽기 전용 질의.
//!
//! 왜 따로 있는가: `export-markdown` 은 페이지 렌더 트리를 직렬화하므로 페이지에 걸친 표가
//! 페이지마다 통째로 중복되고, 머리말·글상자·중첩 표는 (구역, 문단, 컨트롤) 주소가
//! 컨테이너 내부 주소라 엉뚱한 표를 내거나 평문으로 흘리며, 병합(rowSpan/colSpan)을
//! 버린다. 반면 `export-tables` 는 모델을 걷어 정확하지만 표만 내서 본문 어디에 놓였는지
//! 알 수 없다. 본 질의는 둘을 잇는다 —
//!
//! - 표는 `table_extract::build_grid` 와 **같은 격자**를 제자리에 싣는다(순번 `index` 도
//!   `export-tables` 와 같다 — 표를 만나는 순서가 `collect_from_paragraph` 와 동일하다).
//! - 한 문단 안의 순서도 지킨다: 표·그림·컨테이너는 문단 안 **문자 위치**
//!   (`Paragraph::control_text_positions`)에 놓이므로, 문단 텍스트를 그 위치에서 나눠
//!   "글 → 표 → 글" 순서 그대로 낸다(실측: 제목 문단 안에 표가 먼저 오고 설명이 뒤따르는
//!   공문서 배치가 흔하다).
//! - 제목 판정은 `structure::build_structure` 를 그대로 재사용한다(새 판정 로직 없음).
//! - 페이지는 렌더 트리의 (구역, 문단[, 컨트롤]) 주소를 역매핑해 붙인다. 컨테이너 안
//!   노드의 주소는 내부 값이라 본문 주소와 충돌하므로 기록하지 않고, 그 블록은 루트
//!   문단의 페이지를 받는다.
//!
//! 파서/렌더 무변경의 읽기 전용 질의(추가 기능). 자기 라운드트립·시각 충실도와 무관.

use std::collections::HashMap;

use serde::Serialize;

use super::structure::{build_structure, StructureMode, StructureNode};
use super::table_extract::{build_grid, TableContainerRef, TableGrid, MAX_NEST_DEPTH};
use crate::document_core::DocumentCore;
use crate::model::control::Control;
use crate::model::document::Document;
use crate::model::paragraph::Paragraph;
use crate::model::style::HeadType;
use crate::renderer::render_tree::{RenderNode, RenderNodeType};

/// 블록 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockKind {
    /// 본문 문단(제목이 아닌 것). 표·그림이 문단 가운데 놓이면 앞뒤 조각이 각각 블록이다.
    Paragraph,
    /// 제목 문단 — `export-structure` 와 같은 판정.
    Heading,
    /// 표 — `export-tables` 와 같은 격자.
    Table,
    /// 그림.
    Image,
}

/// 제목 판정 결과(`export-structure` 노드와 같은 어휘).
#[derive(Debug, Clone, Serialize)]
pub struct HeadingInfo {
    /// 계층 깊이(1=최상위).
    pub level: u8,
    /// 종류: "outline" | "편"|"장"|"절"|"관"|"조"|"항"|"호"|"목".
    pub kind: &'static str,
    /// 검출된 번호 마커(예: "제1조", "①"). 개요(자동번호)는 빈 문자열이라 생략된다.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub marker: String,
}

/// 블록 하나.
#[derive(Debug, Clone, Serialize)]
pub struct Block {
    /// 문서 순서(0부터).
    pub index: usize,
    pub kind: BlockKind,
    /// 루트(본문) 문단 주소 — 컨테이너 안 블록도 루트 주소를 가진다(`export-tables` 와 동일).
    pub section: usize,
    pub paragraph: usize,
    /// 표·그림의 부모 문단 내 컨트롤 인덱스.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control: Option<usize>,
    /// 글상자·머리말/꼬리말·각주/미주 안이면 그 경로.
    #[serde(rename = "containerPath", skip_serializing_if = "Vec::is_empty")]
    pub container_path: Vec<TableContainerRef>,
    /// 렌더 페이지(0부터). 레이아웃을 건너뛰었거나 역매핑이 없으면 생략.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    /// 문단·제목 텍스트(수식 스크립트 포함, 앞뒤 공백 제거). 표·그림은 생략.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// 제목 블록의 판정.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<HeadingInfo>,
    /// 문단 모양의 머리 종류(엔진값): outline | number | bullet. 없으면 생략.
    #[serde(rename = "headType", skip_serializing_if = "Option::is_none")]
    pub head_type: Option<&'static str>,
    /// 문단 수준(1~7). `headType` 이 있을 때만 싣는다.
    #[serde(rename = "paraLevel", skip_serializing_if = "Option::is_none")]
    pub para_level: Option<u8>,
    /// 표 격자 — `export-tables` 의 같은 표와 필드가 같다(`index` 포함).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<TableGrid>,
    /// 그림의 BinData 번호.
    #[serde(rename = "binDataId", skip_serializing_if = "Option::is_none")]
    pub bin_data_id: Option<u16>,
}

/// 추출 옵션.
#[derive(Debug, Clone, Copy)]
pub struct BlocksOptions {
    /// 제목 판정 방식(`export-structure --mode` 와 동일).
    pub mode: StructureMode,
    /// 렌더 페이지 역매핑 여부. 끄면 레이아웃을 돌리지 않는다.
    pub pages: bool,
}

/// 추출 결과.
#[derive(Debug, Clone, Serialize)]
pub struct BlocksDoc {
    /// 실제로 적용된 제목 판정 방식(`auto` 는 해소된 값으로).
    pub mode: &'static str,
    #[serde(rename = "blockCount")]
    pub block_count: usize,
    /// 표 블록 수(중첩 표는 세지 않는다 — `export-tables` 의 `tableCount` 와 같다).
    #[serde(rename = "tableCount")]
    pub table_count: usize,
    /// 총 페이지 수. 페이지 역매핑을 껐으면 생략.
    #[serde(rename = "pageCount", skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u32>,
    /// 레이아웃에 성공해 역매핑에 쓴 페이지 수. `pageCount` 보다 작으면 일부 페이지의
    /// 블록은 `page` 가 빠진다 — 조용한 소실 대신 개수로 드러낸다.
    #[serde(rename = "pagesMapped", skip_serializing_if = "Option::is_none")]
    pub pages_mapped: Option<u32>,
    pub blocks: Vec<Block>,
}

/// 렌더 트리에서 역매핑한 페이지 표.
#[derive(Default)]
struct PageMap {
    /// (구역, 문단) → 그 문단의 첫 줄이 놓인 페이지.
    paragraphs: HashMap<(usize, usize), u32>,
    /// (구역, 문단, 컨트롤) → 본문 표·그림이 처음 놓인 페이지(분할 표는 첫 조각).
    controls: HashMap<(usize, usize, usize), u32>,
    mapped: u32,
}

impl PageMap {
    fn build(core: &DocumentCore) -> (u32, Self) {
        let page_count = core.page_count();
        let mut map = Self::default();
        for page in 0..page_count {
            // 한 페이지의 레이아웃 실패는 그 페이지의 역매핑만 잃는다 — `pagesMapped` 로 드러난다.
            let Ok(tree) = core.build_page_tree_cached(page) else {
                continue;
            };
            map.collect(&tree.root, page, false);
            map.mapped += 1;
        }
        (page_count, map)
    }

    fn collect(&mut self, node: &RenderNode, page: u32, in_container: bool) {
        match &node.node_type {
            // 표·그림은 본문 주소를 직접 가진 노드로 잡는다. 컨테이너(글상자·머리말·각주·표)
            // 안의 노드는 주소가 컨테이너 내부 값이라 본문 주소와 충돌하므로 기록하지 않는다.
            RenderNodeType::Table(t) => {
                if !in_container && t.cell_context.is_none() {
                    if let (Some(s), Some(p), Some(c)) =
                        (t.section_index, t.para_index, t.control_index)
                    {
                        self.controls.entry((s, p, c)).or_insert(page);
                    }
                }
                for child in &node.children {
                    self.collect(child, page, true);
                }
            }
            RenderNodeType::Image(img) => {
                if !in_container {
                    if let (Some(s), Some(p), Some(c)) =
                        (img.section_index, img.para_index, img.control_index)
                    {
                        self.controls.entry((s, p, c)).or_insert(page);
                    }
                }
            }
            RenderNodeType::TextLine(tl) => {
                if !in_container {
                    if let (Some(s), Some(p)) = (tl.section_index, tl.para_index) {
                        self.paragraphs.entry((s, p)).or_insert(page);
                    }
                }
            }
            RenderNodeType::Header
            | RenderNodeType::Footer
            | RenderNodeType::FootnoteArea
            | RenderNodeType::TextBox
            | RenderNodeType::MasterPage => {
                for child in &node.children {
                    self.collect(child, page, true);
                }
            }
            _ => {
                for child in &node.children {
                    self.collect(child, page, in_container);
                }
            }
        }
    }
}

/// `build_structure` 결과에서 (구역, 문단) → 제목 판정 표를 만든다.
fn heading_map(
    doc: &Document,
    mode: StructureMode,
) -> (&'static str, HashMap<(usize, usize), HeadingInfo>) {
    fn visit(nodes: &[StructureNode], out: &mut HashMap<(usize, usize), HeadingInfo>) {
        for n in nodes {
            out.insert(
                (n.section, n.paragraph),
                HeadingInfo {
                    level: n.level,
                    kind: n.kind,
                    marker: n.marker.clone(),
                },
            );
            visit(&n.children, out);
        }
    }
    let structure = build_structure(doc, mode);
    let mut map = HashMap::new();
    visit(&structure.roots, &mut map);
    (structure.mode, map)
}

/// 블록을 내는(문단 텍스트를 가르는) 컨트롤인가 — 표·그림과 내부 문단을 가진 컨테이너.
fn is_block_control(control: &Control) -> bool {
    match control {
        Control::Table(_)
        | Control::Picture(_)
        | Control::Header(_)
        | Control::Footer(_)
        | Control::Footnote(_)
        | Control::Endnote(_) => true,
        Control::Shape(shape) => shape.drawing().and_then(|d| d.text_box.as_ref()).is_some(),
        _ => false,
    }
}

/// 문단 텍스트의 `[from, to)` 구간(꼬리 구간이면 `to` 위치의 수식까지)에 수식 스크립트를
/// 합친다 — `rendering::paragraph_text_with_equations` 와 같은 규칙(#3413)을 구간에 적용.
fn text_segment(
    text: &[char],
    equations: &[(usize, &str)],
    from: usize,
    to: usize,
    tail: bool,
) -> String {
    let mut output = String::new();
    let mut eq = equations.partition_point(|(p, _)| *p < from);
    for position in from..=to {
        while eq < equations.len() && equations[eq].0 == position && (position < to || tail) {
            let script = equations[eq].1;
            if !output.is_empty() && !output.chars().last().is_some_and(char::is_whitespace) {
                output.push(' ');
            }
            output.push_str(script);
            if position < to && !text[position].is_whitespace() {
                output.push(' ');
            }
            eq += 1;
        }
        if position < to {
            output.push(text[position]);
        }
    }
    output
}

struct Walk<'a> {
    doc: &'a Document,
    headings: HashMap<(usize, usize), HeadingInfo>,
    pages: Option<PageMap>,
    blocks: Vec<Block>,
    table_count: usize,
}

impl Walk<'_> {
    fn page_of_paragraph(&self, section: usize, paragraph: usize) -> Option<u32> {
        self.pages
            .as_ref()
            .and_then(|m| m.paragraphs.get(&(section, paragraph)).copied())
    }

    fn page_of_control(&self, section: usize, paragraph: usize, control: usize) -> Option<u32> {
        self.pages
            .as_ref()
            .and_then(|m| m.controls.get(&(section, paragraph, control)).copied())
            .or_else(|| self.page_of_paragraph(section, paragraph))
    }

    fn push(&mut self, mut block: Block) {
        block.index = self.blocks.len();
        self.blocks.push(block);
    }

    fn blank(
        &self,
        kind: BlockKind,
        section: usize,
        paragraph: usize,
        container_path: &[TableContainerRef],
    ) -> Block {
        Block {
            index: 0,
            kind,
            section,
            paragraph,
            control: None,
            container_path: container_path.to_vec(),
            page: None,
            text: None,
            heading: None,
            head_type: None,
            para_level: None,
            table: None,
            bin_data_id: None,
        }
    }

    /// 문단 텍스트 조각 하나를 블록으로 낸다. 제목 판정은 본문 문단의 **첫 조각**에만 붙는다
    /// (`build_structure` 가 본문만, 문단 단위로 걷는다).
    #[allow(clippy::too_many_arguments)]
    fn emit_text(
        &mut self,
        para: &Paragraph,
        segment: &str,
        section: usize,
        root_paragraph: usize,
        container_path: &[TableContainerRef],
        first: bool,
    ) -> bool {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            return false;
        }
        let heading = if first && container_path.is_empty() {
            self.headings.get(&(section, root_paragraph)).cloned()
        } else {
            None
        };
        let shape = self
            .doc
            .doc_info
            .para_shapes
            .get(para.para_shape_id as usize);
        let head_type = shape.and_then(|ps| match ps.head_type {
            HeadType::Outline => Some("outline"),
            HeadType::Number => Some("number"),
            HeadType::Bullet => Some("bullet"),
            HeadType::None => None,
        });
        let para_level = head_type.and(shape.map(|ps| ps.para_level + 1));
        let kind = if heading.is_some() {
            BlockKind::Heading
        } else {
            BlockKind::Paragraph
        };
        let mut block = self.blank(kind, section, root_paragraph, container_path);
        block.page = self.page_of_paragraph(section, root_paragraph);
        block.text = Some(trimmed.to_string());
        block.heading = heading;
        block.head_type = head_type;
        block.para_level = para_level;
        self.push(block);
        true
    }

    /// 한 문단을 블록으로 낸다 — 표를 만나는 순서는 `table_extract::collect_from_paragraph`
    /// 와 **같아야** 표 순번(`index`)이 `export-tables` 와 일치한다(컨트롤 인덱스 순으로
    /// 만나되, 문단 텍스트만 컨트롤의 문자 위치에서 갈라 끼운다).
    fn walk_paragraph(
        &mut self,
        para: &Paragraph,
        section: usize,
        root_paragraph: usize,
        container_path: &[TableContainerRef],
        depth: usize,
    ) {
        if depth >= MAX_NEST_DEPTH {
            return;
        }

        // [#3413] `para.text` 는 수식 자리에 컨트롤 문자만 남긴다 — export-text·export-structure
        // 와 같은 규칙으로 수식 script 를 합친다.
        let text: Vec<char> = para.text.chars().collect();
        let positions = para.control_text_positions();
        let position_of = |control_index: usize| {
            positions
                .get(control_index)
                .copied()
                .unwrap_or(text.len())
                .min(text.len())
        };
        let mut equations: Vec<(usize, &str)> = para
            .controls
            .iter()
            .enumerate()
            .filter_map(|(i, control)| match control {
                Control::Equation(e) if !e.script.trim().is_empty() => {
                    Some((position_of(i), e.script.trim()))
                }
                _ => None,
            })
            .collect();
        equations.sort_by_key(|(p, _)| *p);

        // 블록 컨트롤은 컨트롤 순서대로(표 순번 계약), 텍스트는 그 컨트롤의 문자 위치까지 낸다.
        // 위치가 앞선 컨트롤보다 뒤로 물러나면(손상·비인라인 컨트롤) 커서를 되돌리지 않는다.
        let mut cursor = 0usize;
        let mut first = true;
        for (control_index, control) in para.controls.iter().enumerate() {
            if !is_block_control(control) {
                continue;
            }
            let at = position_of(control_index).max(cursor);
            let segment = text_segment(&text, &equations, cursor, at, false);
            if self.emit_text(
                para,
                &segment,
                section,
                root_paragraph,
                container_path,
                first,
            ) {
                first = false;
            }
            cursor = at;
            self.emit_control(
                control_index,
                control,
                section,
                root_paragraph,
                container_path,
                depth,
            );
        }
        let tail = text_segment(&text, &equations, cursor, text.len(), true);
        self.emit_text(para, &tail, section, root_paragraph, container_path, first);
    }

    fn emit_control(
        &mut self,
        control_index: usize,
        control: &Control,
        section: usize,
        root_paragraph: usize,
        container_path: &[TableContainerRef],
        depth: usize,
    ) {
        match control {
            Control::Table(table) => {
                let index = self.table_count;
                self.table_count += 1;
                let grid = build_grid(
                    table,
                    index,
                    section,
                    root_paragraph,
                    control_index,
                    container_path,
                    depth,
                );
                let mut block =
                    self.blank(BlockKind::Table, section, root_paragraph, container_path);
                block.control = Some(control_index);
                block.page = if container_path.is_empty() {
                    self.page_of_control(section, root_paragraph, control_index)
                } else {
                    self.page_of_paragraph(section, root_paragraph)
                };
                block.table = Some(grid);
                self.push(block);
            }
            Control::Picture(picture) => {
                let mut block =
                    self.blank(BlockKind::Image, section, root_paragraph, container_path);
                block.control = Some(control_index);
                block.page = if container_path.is_empty() {
                    self.page_of_control(section, root_paragraph, control_index)
                } else {
                    self.page_of_paragraph(section, root_paragraph)
                };
                block.bin_data_id = Some(picture.image_attr.bin_data_id);
                self.push(block);
            }
            // 컨테이너 컨트롤 — 내부 문단을 재귀한다(경로 어휘는 export-tables 와 동일).
            Control::Shape(shape) => {
                if let Some(tb) = shape.drawing().and_then(|d| d.text_box.as_ref()) {
                    self.walk_container(
                        "textbox",
                        &tb.paragraphs,
                        section,
                        root_paragraph,
                        control_index,
                        container_path,
                        depth,
                    );
                }
            }
            Control::Header(h) => self.walk_container(
                "header",
                &h.paragraphs,
                section,
                root_paragraph,
                control_index,
                container_path,
                depth,
            ),
            Control::Footer(f) => self.walk_container(
                "footer",
                &f.paragraphs,
                section,
                root_paragraph,
                control_index,
                container_path,
                depth,
            ),
            Control::Footnote(f) => self.walk_container(
                "footnote",
                &f.paragraphs,
                section,
                root_paragraph,
                control_index,
                container_path,
                depth,
            ),
            Control::Endnote(e) => self.walk_container(
                "endnote",
                &e.paragraphs,
                section,
                root_paragraph,
                control_index,
                container_path,
                depth,
            ),
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_container(
        &mut self,
        kind: &'static str,
        paragraphs: &[Paragraph],
        section: usize,
        root_paragraph: usize,
        control_index: usize,
        container_path: &[TableContainerRef],
        depth: usize,
    ) {
        for (paragraph, p) in paragraphs.iter().enumerate() {
            let mut path = container_path.to_vec();
            path.push(TableContainerRef {
                kind,
                control: control_index,
                paragraph,
                cell: None,
            });
            self.walk_paragraph(p, section, root_paragraph, &path, depth + 1);
        }
    }
}

/// 문서를 문서 순서 블록으로 추출한다.
pub fn export_blocks(core: &DocumentCore, opts: &BlocksOptions) -> BlocksDoc {
    let doc = &core.document;
    let (mode, headings) = heading_map(doc, opts.mode);
    let (page_count, pages) = if opts.pages {
        let (count, map) = PageMap::build(core);
        (Some(count), Some(map))
    } else {
        (None, None)
    };
    let pages_mapped = pages.as_ref().map(|m| m.mapped);

    let mut walk = Walk {
        doc,
        headings,
        pages,
        blocks: Vec::new(),
        table_count: 0,
    };
    for (sec_idx, section) in doc.sections.iter().enumerate() {
        for (para_idx, para) in section.paragraphs.iter().enumerate() {
            walk.walk_paragraph(para, sec_idx, para_idx, &[], 0);
        }
    }

    BlocksDoc {
        mode,
        block_count: walk.blocks.len(),
        table_count: walk.table_count,
        page_count,
        pages_mapped,
        blocks: walk.blocks,
    }
}
