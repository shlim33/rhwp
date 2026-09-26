//! `export-blocks` 출력 계약 회귀 테스트.
//!
//! 계약: 문서 모델을 **문서 순서 그대로** 블록(문단·제목·표·그림)으로 낸다. 표는
//! `export-tables` 와 같은 격자(병합·중첩 보존)를 **제자리에** 싣고, 제목은
//! `export-structure` 와 같은 판정을 따르며, 블록마다 문단 주소(section/paragraph)와
//! 렌더 페이지를 붙인다. `--json` 의 stdout 은 순수 JSON 한 덩어리이고 `schemaVersion` 을
//! 포함한다. 종료 코드는 #2707 계약(0/1/2)을 따른다.
//!
//! 존재 이유: `export-markdown` 은 페이지 렌더 트리를 직렬화해 분할 표가 페이지마다
//! 중복되고 머리말·글상자·중첩 표의 주소를 오해석하며 병합을 버린다. `export-tables` 는
//! 정확하지만 표가 본문 어디에 있는지 모른다. 문서 처리 파이프라인(RAG 적재)이 둘을
//! 잇는 데 쓰는 것이 이 명령이다.
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// 조문형 제목(clause) 11개·표 14개(본문 최상위 + 컨테이너)를 가진 규제영향분석서.
const SAMPLE_REGULATORY: &str = "samples/80250_regulatory_analysis.hwp";
/// 19행×9열, colSpan=3·rowSpan=3 병합을 모두 가진 표 문서.
const SAMPLE_MERGED: &str = "samples/table-001.hwp";
/// 본문 최상위가 아닌 **컨테이너(글상자 등) 안에 표가 있는** 문서(최상위만 보면 1개, 실제 3개).
const SAMPLE_CONTAINER: &str = "samples/basic/treatise sample.hwp";
/// 개요(Outline) 문단 모양을 실제로 쓰는 문서 — `export-structure` 가 outline 모드를 고른다.
const SAMPLE_OUTLINE: &str = "samples/HWP5-nopassword-123456.hwp";

fn sample(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// [#3289] 아카이브 실행 시 컴파일타임 경로는 빌드 러너 전용이므로,
/// nextest 가 런타임에 재매핑해 주입하는 CARGO_BIN_EXE_rhwp 를 우선한다.
fn rhwp_bin() -> String {
    std::env::var("CARGO_BIN_EXE_rhwp").unwrap_or_else(|_| env!("CARGO_BIN_EXE_rhwp").to_string())
}

fn run(args: &[&str]) -> Output {
    Command::new(rhwp_bin())
        .args(args)
        .output()
        .expect("rhwp 실행 실패")
}

fn describe(args: &[&str], output: &Output) -> String {
    format!(
        "명령: rhwp {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn parse_stdout_json(args: &[&str], output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout 이 순수 JSON 이 아닙니다 ({e}).\n{}",
            describe(args, output)
        )
    })
}

fn run_json(args: &[&str]) -> Value {
    let output = run(args);
    assert_eq!(output.status.code(), Some(0), "{}", describe(args, &output));
    parse_stdout_json(args, &output)
}

fn blocks_json(rel: &str, extra: &[&str]) -> Value {
    let p = sample(rel);
    let mut args = vec!["export-blocks", p.to_str().unwrap(), "--json"];
    args.extend_from_slice(extra);
    run_json(&args)
}

fn blocks_of(v: &Value) -> &Vec<Value> {
    v["blocks"].as_array().expect("blocks 배열")
}

fn is_body(b: &Value) -> bool {
    b.get("containerPath").is_none()
}

#[test]
fn export_blocks_json_envelope_contract() {
    let v = blocks_json(SAMPLE_REGULATORY, &[]);
    assert_eq!(v["schemaVersion"], "1.0", "{v}");
    assert!(v["source"].is_string(), "{v}");
    let mode = v["mode"].as_str().expect("mode");
    assert!(
        mode == "outline" || mode == "clause",
        "auto 는 해소된 값으로 나와야 한다: {mode}"
    );

    let blocks = blocks_of(&v);
    assert!(!blocks.is_empty(), "{v}");
    assert_eq!(
        blocks.len() as u64,
        v["blockCount"].as_u64().expect("blockCount"),
        "blockCount 는 blocks 길이와 같다"
    );
    let table_blocks = blocks.iter().filter(|b| b["kind"] == "table").count() as u64;
    assert_eq!(
        table_blocks,
        v["tableCount"].as_u64().expect("tableCount"),
        "tableCount 는 표 블록 수와 같다"
    );

    for (i, b) in blocks.iter().enumerate() {
        assert_eq!(
            b["index"].as_u64(),
            Some(i as u64),
            "index 는 문서 순서다: {b}"
        );
        let kind = b["kind"].as_str().expect("kind");
        assert!(
            matches!(kind, "paragraph" | "heading" | "table" | "image"),
            "알 수 없는 kind: {b}"
        );
        assert!(b["section"].as_u64().is_some(), "{b}");
        assert!(
            b["paragraph"].as_u64().is_some(),
            "문단 주소가 있어야 인용된다: {b}"
        );
        match kind {
            "paragraph" | "heading" => {
                let text = b["text"].as_str().expect("text");
                assert!(!text.trim().is_empty(), "빈 문단은 블록이 아니다: {b}");
                assert!(b.get("table").is_none(), "{b}");
            }
            "table" => {
                assert!(b["table"].is_object(), "표 블록은 격자를 싣는다: {b}");
                assert!(b["control"].as_u64().is_some(), "{b}");
                assert!(b.get("text").is_none(), "표 텍스트는 격자가 정본이다: {b}");
            }
            "image" => {
                assert!(b["control"].as_u64().is_some(), "{b}");
            }
            _ => unreachable!(),
        }
    }

    // 본문 블록은 문서 순서(구역, 문단, 컨트롤)가 감소하지 않는다.
    let mut prev = (0u64, 0u64, 0u64);
    for b in blocks.iter().filter(|b| is_body(b)) {
        let cur = (
            b["section"].as_u64().unwrap(),
            b["paragraph"].as_u64().unwrap(),
            b["control"].as_u64().unwrap_or(0),
        );
        assert!(cur >= prev, "문서 순서가 역전됐다: {prev:?} → {cur:?}\n{b}");
        prev = cur;
    }
}

#[test]
fn export_blocks_tables_equal_export_tables() {
    // 이 테스트가 본 기능의 핵심 가치다 — 표는 export-tables 와 **같은 격자**를 제자리에 싣는다.
    for rel in [SAMPLE_REGULATORY, SAMPLE_MERGED, SAMPLE_CONTAINER] {
        let p = sample(rel);
        let tables = run_json(&["export-tables", p.to_str().unwrap(), "--json"]);
        let expected = tables["tables"].as_array().expect("tables");
        let v = blocks_json(rel, &["--no-pages"]);
        let got: Vec<&Value> = blocks_of(&v)
            .iter()
            .filter(|b| b["kind"] == "table")
            .collect();
        assert_eq!(
            got.len(),
            expected.len(),
            "{rel}: 표 블록 수가 export-tables 와 다르다"
        );
        for (b, t) in got.iter().zip(expected) {
            assert_eq!(&b["table"], t, "{rel}: 표 격자가 export-tables 와 다르다");
            assert_eq!(b["section"], t["section"], "{rel}: {b}");
            assert_eq!(b["paragraph"], t["paragraph"], "{rel}: {b}");
            assert_eq!(b["control"], t["control"], "{rel}: {b}");
            assert_eq!(
                b.get("containerPath"),
                t.get("containerPath"),
                "{rel}: 컨테이너 경로가 다르다: {b}"
            );
        }
    }
}

#[test]
fn export_blocks_headings_match_export_structure() {
    let p = sample(SAMPLE_REGULATORY);
    let structure = run_json(&["export-structure", p.to_str().unwrap(), "--json"]);
    assert!(
        structure["nodeCount"].as_u64().unwrap() >= 3,
        "픽스처에 제목이 있어야 한다: {structure}"
    );
    let v = blocks_json(SAMPLE_REGULATORY, &["--no-pages"]);
    assert_eq!(v["mode"], structure["mode"], "제목 판정 방식이 같아야 한다");

    fn visit(nodes: &[Value], out: &mut Vec<(u64, u64, u64, String)>) {
        for n in nodes {
            out.push((
                n["section"].as_u64().unwrap(),
                n["paragraph"].as_u64().unwrap(),
                n["level"].as_u64().unwrap(),
                n["kind"].as_str().unwrap().to_string(),
            ));
            if let Some(children) = n["children"].as_array() {
                visit(children, out);
            }
        }
    }
    let mut expected = Vec::new();
    visit(
        structure["structure"]["roots"].as_array().unwrap(),
        &mut expected,
    );
    assert!(!expected.is_empty());

    let blocks = blocks_of(&v);
    for (section, paragraph, level, kind) in &expected {
        let b = blocks
            .iter()
            .find(|b| {
                b["kind"] == "heading"
                    && b["section"].as_u64() == Some(*section)
                    && b["paragraph"].as_u64() == Some(*paragraph)
                    && is_body(b)
            })
            .unwrap_or_else(|| panic!("제목 [{section}:{paragraph}] 이 블록에 없다"));
        assert_eq!(b["heading"]["level"].as_u64(), Some(*level), "{b}");
        assert_eq!(b["heading"]["kind"].as_str(), Some(kind.as_str()), "{b}");
    }
    let heading_blocks = blocks
        .iter()
        .filter(|b| b["kind"] == "heading" && is_body(b))
        .count();
    assert_eq!(
        heading_blocks,
        expected.len(),
        "제목 블록은 export-structure 노드와 1:1 이다"
    );
}

#[test]
fn export_blocks_pages_follow_document_order() {
    let v = blocks_json(SAMPLE_REGULATORY, &[]);
    let page_count = v["pageCount"].as_u64().expect("pageCount");
    assert!(page_count >= 1, "{v}");
    assert_eq!(
        v["pagesMapped"].as_u64().expect("pagesMapped"),
        page_count,
        "모든 페이지가 레이아웃돼야 역매핑이 완전하다"
    );
    let body: Vec<&Value> = blocks_of(&v).iter().filter(|b| is_body(b)).collect();
    let with_page = body.iter().filter(|b| b["page"].is_u64()).count();
    assert!(
        with_page * 10 >= body.len() * 9,
        "본문 블록 대부분은 페이지를 가져야 한다: {with_page}/{}",
        body.len()
    );
    let mut prev = 0u64;
    for b in &body {
        if let Some(page) = b["page"].as_u64() {
            assert!(page < page_count, "페이지 범위 밖: {b}");
            assert!(
                page >= prev,
                "본문 흐름의 페이지가 역전됐다: {prev} → {page}\n{b}"
            );
            prev = page;
        }
    }
}

#[test]
fn export_blocks_no_pages_skips_layout() {
    let v = blocks_json(SAMPLE_MERGED, &["--no-pages"]);
    assert!(v.get("pageCount").is_none(), "{v}");
    assert!(v.get("pagesMapped").is_none(), "{v}");
    assert!(
        blocks_of(&v).iter().all(|b| b.get("page").is_none()),
        "--no-pages 면 어떤 블록에도 page 가 없다: {v}"
    );
}

#[test]
fn export_blocks_container_blocks_carry_path() {
    let v = blocks_json(SAMPLE_CONTAINER, &["--no-pages"]);
    let contained: Vec<&Value> = blocks_of(&v).iter().filter(|b| !is_body(b)).collect();
    assert!(
        !contained.is_empty(),
        "컨테이너 안 블록이 하나는 있어야 한다: {v}"
    );
    for b in contained {
        for step in b["containerPath"].as_array().unwrap() {
            let kind = step["kind"].as_str().unwrap();
            assert!(
                matches!(
                    kind,
                    "textbox" | "header" | "footer" | "footnote" | "endnote" | "tableCell"
                ),
                "알 수 없는 컨테이너 종류: {b}"
            );
            assert!(step["control"].as_u64().is_some(), "{b}");
            assert!(step["paragraph"].as_u64().is_some(), "{b}");
        }
    }
}

#[test]
fn export_blocks_head_type_is_engine_fact() {
    // 문단 모양의 머리 종류(outline/number/bullet)는 판정이 아니라 엔진값이다 — 제목 판정
    // (`export-structure` outline 모드: Outline·Number 문단 = 제목, level = 문단 수준)과
    // 따로 실려야 소비자가 둘을 대조할 수 있다.
    let v = blocks_json(SAMPLE_OUTLINE, &["--no-pages"]);
    assert_eq!(v["mode"], "outline", "{v}");
    let numbered: Vec<&Value> = blocks_of(&v)
        .iter()
        .filter(|b| b["headType"] == "outline" || b["headType"] == "number")
        .collect();
    assert!(
        !numbered.is_empty(),
        "개요/번호 문단 모양이 headType 으로 나와야 한다: {v}"
    );
    for b in numbered {
        let level = b["paraLevel"].as_u64().expect("paraLevel");
        assert!((1..=7).contains(&level), "{b}");
        assert_eq!(
            b["kind"], "heading",
            "개요/번호 문단은 outline 모드의 제목이다: {b}"
        );
        assert_eq!(b["heading"]["kind"], "outline", "{b}");
        assert_eq!(b["heading"]["level"].as_u64(), Some(level), "{b}");
    }
    assert!(
        blocks_of(&v)
            .iter()
            .filter(|b| b["kind"] == "paragraph")
            .all(|b| b.get("headType").is_none() || b["headType"] == "bullet"),
        "제목이 아닌 문단에 outline/number 머리가 남아 있다: {v}"
    );
}

#[test]
fn export_blocks_provenance_marks_document_text() {
    let v = blocks_json(SAMPLE_MERGED, &["--no-pages"]);
    assert_eq!(v["untrustedContent"], true, "{v}");
    let fields: Vec<&str> = v["untrustedFields"]
        .as_array()
        .expect("untrustedFields")
        .iter()
        .filter_map(|f| f.as_str())
        .collect();
    assert!(
        fields.contains(&"blocks[].table.cells[].text"),
        "표 셀 텍스트는 문서 파생 값이다: {fields:?}"
    );
}

#[test]
fn export_blocks_default_output_is_human_summary() {
    let p = sample(SAMPLE_MERGED);
    let args = ["export-blocks", p.to_str().unwrap(), "--no-pages"];
    let output = run(&args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        describe(&args, &output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        serde_json::from_str::<Value>(stdout.trim()).is_err(),
        "--json 없이는 사람용 요약이어야 한다:\n{stdout}"
    );
    assert!(stdout.contains("블록"), "{stdout}");
}

#[test]
fn export_blocks_output_file_is_pretty_json() {
    let dir = std::env::temp_dir().join(format!("rhwp-export-blocks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("blocks.json");
    let p = sample(SAMPLE_MERGED);
    let args = [
        "export-blocks",
        p.to_str().unwrap(),
        "--no-pages",
        "-o",
        out.to_str().unwrap(),
    ];
    let output = run(&args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        describe(&args, &output)
    );
    let v: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).expect("파일이 JSON");
    assert_eq!(v["schemaVersion"], "1.0");
    assert!(v["blockCount"].as_u64().unwrap() >= 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_blocks_missing_file_exit_runtime_silent_stdout() {
    let args = ["export-blocks", "/nonexistent/없는파일.hwp", "--json"];
    let output = run(&args);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        describe(&args, &output)
    );
    assert!(
        output.stdout.is_empty(),
        "실패 경로의 stdout 은 비어야 한다: {}",
        describe(&args, &output)
    );
}

#[test]
fn export_blocks_usage_errors_exit_two() {
    let p = sample(SAMPLE_MERGED);
    let file = p.to_str().unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec!["export-blocks"],
        vec!["export-blocks", file, "--bogus"],
        vec!["export-blocks", file, file],
        vec!["export-blocks", file, "--mode", "nope"],
        vec!["export-blocks", file, "--mode"],
        vec!["export-blocks", file, "-o"],
    ];
    for args in cases {
        let output = run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{}",
            describe(&args, &output)
        );
        assert!(output.stdout.is_empty(), "{}", describe(&args, &output));
    }
}

#[test]
fn export_blocks_is_registered_in_capabilities_and_mcp() {
    let cap = run_json(&["capabilities"]);
    let cmd = cap["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "export-blocks")
        .unwrap_or_else(|| panic!("capabilities 에 export-blocks 가 없다: {cap}"));
    assert_eq!(cmd["json"], true, "{cmd}");
    let flags: Vec<&str> = cmd["flags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f.as_str())
        .collect();
    for f in ["--json", "-o", "--mode", "--no-pages"] {
        assert!(flags.contains(&f), "flags 에 {f} 가 없다: {cmd}");
    }

    let mcp = run_json(&["capabilities", "--mcp"]);
    let tool = mcp["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "hwp_export_blocks")
        .unwrap_or_else(|| panic!("MCP 도구 hwp_export_blocks 가 없다"));
    assert_eq!(tool["cli"]["command"], "export-blocks", "{tool}");
}
