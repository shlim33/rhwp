# `export-blocks` — 문서 순서 블록 JSON (처리 결과 문서)

브랜치 `feat/export-blocks` (트렁크 `xyren/integration-v0.8.4` 기준). 소비자는 xyren-studio 의
`xyren_parse` HwpHandler(HWP/HWPX 적재 파서).

## 왜

소비자 실측(2026-09-26, xyren-studio `research/table-fidelity-2026-09-26`):

- HWPX 격자표 인식 92% 이나, 핸들러가 표를 `export-markdown` 본문과 짝지을 수 없어 XML 에서
  따로 읽은 표를 **문서 끝에 덧붙였고**, 그 결과 실적재 문서의 표 조각 34/34·130/130 이
  **문서 마지막 제목**의 문맥을 받고 페이지가 없었다.
- 원인은 `export-markdown` 이 페이지 렌더 트리를 직렬화하는 설계다(`rendering.rs
  extract_page_markdown_with_images_native`): 페이지에 걸친 표는 페이지마다 통째로 중복,
  머리말·글상자·중첩 표는 `(section, para, control)` 주소가 컨테이너 내부 값인데
  `lookup_table` 이 본문 주소로 해석해 엉뚱한 표를 내거나 평문으로 흘림, `table_to_markdown`
  은 rowSpan/colSpan 을 버리고 첫 행을 무조건 머리 행으로 만든다.
- 반면 `export-tables`(`table_extract::extract_tables`)는 모델을 걷어 XML 정답과 문서 213/213
  개수·순서가 일치하고 병합·중첩·컨테이너 표를 보존한다 — 다만 표만 낸다.

## 무엇

`rhwp export-blocks <file> [--json] [-o out.json] [--mode auto|outline|clause] [--no-pages]`
(+ MCP `hwp_export_blocks`). 라이브러리 질의 `document_core::queries::blocks::export_blocks`.

- 문서 모델을 문서 순서대로 걸어 `paragraph`·`heading`·`table`·`image` 블록을 낸다.
- 표: `table_extract::build_grid` 와 같은 함수 → `export-tables` 와 격자·`index` 동일.
- 제목: `structure::build_structure` 결과를 (section, paragraph) 로 역참조 → 같은 판정.
  문단 모양의 머리 종류는 엔진값 `headType`/`paraLevel` 로 따로 싣는다.
- 한 문단 안의 순서: 표·그림·컨테이너의 문자 위치(`Paragraph::control_text_positions`)에서
  문단 텍스트를 갈라 "글 → 표 → 글" 순서를 지킨다.
- 페이지: 렌더 트리(`build_page_tree_cached`)의 TextLine/Table/Image 주소를 역매핑. 컨테이너
  안 노드는 주소가 내부 값이라 기록하지 않고 루트 문단 페이지를 준다. `pagesMapped` 로
  완전성을 공개한다. `--no-pages` 면 레이아웃을 돌리지 않는다.
- 출처 표지: `blocks[].text`·`blocks[].heading.marker`·`blocks[].table.caption`·
  `blocks[].table.cells[].text`·`blocks[].table.cells[].nested[]`.

## 실행 원문

```
$ rhwp export-blocks samples/80250_regulatory_analysis.hwp
문서 로드: samples/80250_regulatory_analysis.hwp (블록 78개: 문단 53, 제목 11, 표 14, 그림 0; 제목 판정 clause)
  페이지 17쪽 (역매핑 17쪽)

$ rhwp export-blocks --json samples/80250_regulatory_analysis.hwp | jq -c 'del(.blocks)'
{"blockCount":78,"mode":"clause","pageCount":17,"pagesMapped":17,"schemaVersion":"1.0",
 "source":"samples/80250_regulatory_analysis.hwp","tableCount":14,"untrustedContent":true,
 "untrustedFields":["blocks[].text","blocks[].heading.marker","blocks[].table.cells[].text","blocks[].table.cells[].nested[]"]}

$ rhwp export-blocks --json samples/80250_regulatory_analysis.hwp | jq -c '.blocks[2:5][] | del(.table.cells)'
{"index":2,"kind":"paragraph","page":0,"paragraph":7,"section":0,"text":"<목 차>"}
{"control":0,"index":3,"kind":"table","page":0,"paragraph":8,"section":0,"table":{"cellCount":1,"cols":1,"control":0,"index":2,"paragraph":8,"rows":1,"section":0}}
{"control":0,"index":4,"kind":"table","page":0,"paragraph":10,"section":0,"table":{"cellCount":22,"cols":7,"control":0,"index":3,"paragraph":10,"rows":5,"section":0}}
```

## 검증

- 계약 테스트 `tests/export_blocks_json_contract.rs` 13건 green: 봉투·문서 순서·표 격자 =
  `export-tables`(3 표본)·제목 = `export-structure`·페이지 단조·`--no-pages`·컨테이너 경로·
  headType·출처 표지·사람용 기본 출력·`-o`·exit 1/2·capabilities/MCP 등재.
- 이웃 스위트 무회귀: `provenance_contract`(10)·`cli_json_contract`(31)·`mcp_server_contract`(24)·
  `table_extract_json_contract`(7)·`output_axis_json_contract`.
- `cargo fmt --all -- --check` clean(변경 파일 기준), `cargo clippy --all-targets -- -D warnings` 0.
- 실측(rhwp `samples/` HWPX 275건, XML `hp:tbl` 정답): 최상위 표 2,617 ↔ 표 블록 2,617(문서
  275/275 개수 일치), 격자 = `export-tables` 275/275, 페이지 역매핑 275/275 완전(본문 블록
  49,206/49,216 에 page), 본문 page 단조 268/275. 순서: 표 직전 본문 블록 = XML 상 표 앞
  문단 75%, 앵커 없음(문서 첫머리·컨테이너 뒤) 14%, 검사기 한계로 미확정 11%(1개 문서 집중).
  소요: 275건 `--no-pages` 6초, 페이지 포함 16초.

## 한계(문서화)

- 자동번호·글머리표 **문자열**은 렌더 단계 값이라 `text` 에 없다(`headType`/`paraLevel` 로만).
- 컨테이너 안 문단은 제목 판정을 하지 않는다(`build_structure` 가 본문만 걷는다).
- 컨테이너 안 블록의 `page` 는 루트 문단의 페이지다.
