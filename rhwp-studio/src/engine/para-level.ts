/**
 * 문단 수준(paraLevel) 증감 계산.
 *
 * ParaShape.para_level의 유효 범위는 0~6 (=1~7수준, core/types.ts ParaProperties.paraLevel,
 * 번호 정의 level_formats: [String; 7] 과 일치).
 *
 * 참고: 이 모듈은 node 단위 테스트에서 직접 import 되므로 경로 별칭(@/) 없이
 * 의존성 0으로 유지한다.
 */

export const MIN_PARA_LEVEL = 0;
export const MAX_PARA_LEVEL = 6;

/**
 * 현재 수준에서 delta만큼 이동한 목표 수준을 반환한다.
 *
 * - 비정상 입력(범위 밖 current)은 먼저 0~6으로 정규화한다.
 * - 이동 결과가 범위를 벗어나면 null (호출측 no-op — 개요 스타일 경로의
 *   경계 동작과 동일하게 조용히 무시).
 */
export function nextParaLevel(current: number | undefined, delta: number): number | null {
  const base = Math.min(MAX_PARA_LEVEL, Math.max(MIN_PARA_LEVEL, Math.trunc(current ?? 0)));
  const target = base + delta;
  if (target < MIN_PARA_LEVEL || target > MAX_PARA_LEVEL) return null;
  return target;
}
