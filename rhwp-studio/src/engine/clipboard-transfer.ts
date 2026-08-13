/**
 * navigator.clipboard.read() 결과(ClipboardItem[]) → DataTransfer 변환.
 *
 * 도구 상자 '붙이기' 버튼 등 프로그래밍 방식 붙여넣기에서, 키보드 Ctrl+V 경로
 * (input-handler-keyboard.ts onPaste)가 소비하는 것과 동일한 DataTransfer 형태로
 * 시스템 클립보드 내용을 변환한다.
 *
 * - text/plain, text/html → dt.setData (onPaste의 getData 분기)
 * - image/*              → File 항목으로 추가 (onPaste의 items[i].kind==='file' 분기)
 * - 그 외 타입은 무시한다 (onPaste가 소비하지 않는 표현).
 *
 * 참고: 이 모듈은 node 단위 테스트에서 직접 import 되므로 경로 별칭(@/) 없이
 * 의존성 0으로 유지한다.
 */

/** ClipboardItem의 구조적 최소 인터페이스 (테스트 fake 주입용) */
export interface ClipboardItemLike {
  readonly types: readonly string[];
  getType(type: string): Promise<Blob>;
}

/** image MIME → 파일 확장자 (pasteImageFile의 ext 규칙과 동일: jpeg→jpg) */
export function imageExtFromMime(mime: string): string {
  return (mime.split('/')[1] || 'png').replace('jpeg', 'jpg');
}

/**
 * ClipboardItem 목록을 DataTransfer로 변환한다.
 *
 * 개별 표현(getType) 읽기 실패는 해당 표현만 건너뛰고 나머지 표현으로 계속한다
 * — 모든 표현이 실패하면 빈 DataTransfer가 반환되고, 호출측(performPaste)이
 * 빈 클립보드 안내를 표시하므로 무음 실패가 되지 않는다.
 */
export async function clipboardItemsToDataTransfer(
  items: readonly ClipboardItemLike[],
): Promise<DataTransfer> {
  const dt = new DataTransfer();
  for (const item of items) {
    for (const type of item.types) {
      try {
        if (type === 'text/plain' || type === 'text/html') {
          const text = await (await item.getType(type)).text();
          if (text) dt.setData(type, text);
        } else if (type.startsWith('image/')) {
          const blob = await item.getType(type);
          dt.items.add(new File([blob], `clipboard.${imageExtFromMime(type)}`, { type }));
        }
      } catch {
        // 이 표현만 건너뜀 — 남은 표현으로 붙여넣기 진행 (전부 실패 시 호출측이 안내)
      }
    }
  }
  return dt;
}
