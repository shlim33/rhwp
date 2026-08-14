//! 삽입 직후 그림의 HWPX export 생존 계약 — 2026-08-14 실사용 신고("이미지가 있는
//! 부분이 사라졌다") 재현 시험. AI 이미지 교체(insertPicture) → 저장(exportHwpx)
//! → 재열람 흐름에서 그림·BinData 가 왕복을 살아남아야 한다.
use rhwp::document_core::DocumentCore;

/// 1×1 PNG (67 bytes).
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
    0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
    0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
    0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
    0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn picture_count(core: &DocumentCore) -> usize {
    use rhwp::model::control::Control;
    let mut n = 0;
    for sec in &core.document().sections {
        for p in &sec.paragraphs {
            for c in &p.controls {
                if matches!(c, Control::Picture(_)) {
                    n += 1;
                }
            }
        }
    }
    n
}

#[test]
fn inserted_picture_survives_hwpx_export_roundtrip() {
    let sample =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("samples/hwpx/table-text.hwpx");
    let bytes = std::fs::read(&sample).expect("read table-text.hwpx");
    let mut core = DocumentCore::from_bytes(&bytes).expect("parse");
    let before = picture_count(&core);

    let result = core
        .insert_picture_native(0, 0, 0, &[], TINY_PNG, 7500, 7500, 1, 1, "png", "테스트 그림", None, None)
        .expect("insert picture");
    let _ = result;
    assert_eq!(picture_count(&core), before + 1, "삽입 직후 IR 에 그림이 있어야 한다");

    let exported = core.export_hwpx_native().expect("export hwpx");
    let reloaded = DocumentCore::from_bytes(&exported).expect("reparse exported");
    assert_eq!(
        picture_count(&reloaded),
        before + 1,
        "export→reparse 후에도 그림이 남아야 한다(저장이 그림을 소실하면 안 된다)"
    );

    // BinData 실체(이미지 바이트)도 패키지에 실려야 한다.
    let cursor = std::io::Cursor::new(&exported);
    let mut zip = zip::ZipArchive::new(cursor).expect("zip");
    let bindata = (0..zip.len())
        .filter(|i| {
            zip.by_index(*i)
                .map(|f| f.name().starts_with("BinData/"))
                .unwrap_or(false)
        })
        .count();
    assert!(bindata >= 1, "BinData 항목이 최소 1개 있어야 한다");
}
