/**
 * E2E 테스트: 도구 상자(아이콘 툴바) 커맨드 배선
 *
 * 대상: 오려두기/복사하기/붙이기 버튼 배선(data-cmd), 프로그래밍 방식 붙여넣기
 *       (performPaste — 내부 클립보드 fast path + navigator.clipboard.read()),
 *       모양 복사 비활성 표시, 문단 번호 토글, 수준▲/수준▼(개요 외 문단 paraLevel).
 *
 * 시나리오:
 *   1. 빈 문서 → 시스템 클립보드에 텍스트 기록 → [붙이기] 클릭 → 외부 클립보드 경로 검증
 *   2. 전체 선택 → [복사하기] 클릭 → 내부 클립보드 채워짐 검증
 *   3. End → [붙이기] 클릭 → 내부 fast path로 텍스트 두 배
 *   4. 전체 선택 → [오려두기] 클릭 → 텍스트 제거 → [붙이기]로 복원
 *   5. [모양 복사] 버튼 disabled 검증
 *   6. [문단 번호] 클릭 → headType=Number
 *   7. [수준▼]/[수준▲] 클릭 → paraLevel 1→0 왕복
 *   8. 번호 해제 후 [수준▼] → no-op + 토스트 안내
 */
import {
  runTest, createNewDocument, clickEditArea, typeText,
  screenshot, assert, getParaText,
} from './helpers.mjs';

/** 도구 상자 버튼 클릭 (mousedown 바인딩이므로 실제 마우스 클릭 사용) */
async function clickToolbarButton(page, cmd) {
  const sel = `#icon-toolbar .tb-btn[data-cmd="${cmd}"]`;
  await page.waitForSelector(sel, { timeout: 5000 });
  await page.click(sel);
  await page.evaluate(() => new Promise(r => setTimeout(r, 400)));
}

/** 본문 문단의 ParaProperties 조회 */
async function getParaProps(page, sec = 0, para = 0) {
  return await page.evaluate(
    (s, p) => window.__wasm?.getParaPropertiesAt(s, p) ?? null, sec, para,
  );
}

async function selectAll(page) {
  await page.keyboard.down('Control');
  await page.keyboard.press('a');
  await page.keyboard.up('Control');
  await page.evaluate(() => new Promise(r => setTimeout(r, 300)));
}

runTest('도구 상자 커맨드 (오려두기/복사/붙이기/모양복사/문단번호/수준)', async ({ page }) => {
  // 클립보드 권한 부여 (navigator.clipboard.read/write — 외부 클립보드 경로용)
  const origin = new URL(page.url()).origin;
  await page.browserContext().overridePermissions(origin, [
    'clipboard-read', 'clipboard-write', 'clipboard-sanitized-write',
  ]);

  await createNewDocument(page);
  await clickEditArea(page);

  // ── 1. 외부 클립보드 경로: navigator.clipboard.read() ──────────────
  // 내부 클립보드가 비어 있는 상태에서 시스템 클립보드에 텍스트를 넣고 [붙이기] 클릭
  const internalBefore = await page.evaluate(() => window.__wasm?.hasInternalClipboard());
  assert(internalBefore === false, `초기 내부 클립보드 비어 있음 (${internalBefore})`);
  await page.evaluate(() => navigator.clipboard.writeText('웹클립보드'));
  await clickToolbarButton(page, 'edit:paste');
  await page.evaluate(() => new Promise(r => setTimeout(r, 500))); // 비동기 read 대기
  const t1 = await getParaText(page, 0, 0, 100);
  await screenshot(page, 'tbcmd-01-external-paste');
  assert(t1.includes('웹클립보드'), `[붙이기] 외부 클립보드 경로: "${t1}"`);

  // ── 2. [복사하기] 버튼 → 내부 클립보드 ─────────────────────────────
  await typeText(page, '가나다');
  await selectAll(page);
  await clickToolbarButton(page, 'edit:copy');
  const hasInternal = await page.evaluate(() => window.__wasm?.hasInternalClipboard());
  const clipText = await page.evaluate(() => window.__wasm?.getClipboardText() ?? '');
  assert(hasInternal === true, `[복사하기] 후 내부 클립보드 존재 (${hasInternal})`);
  assert(clipText.includes('가나다'), `[복사하기] 내부 클립보드 텍스트: "${clipText}"`);
  // 시스템 클립보드에도 기록됐는지 (performCopy → execCommand('copy') → onCopy)
  const sysText = await page.evaluate(() => navigator.clipboard.readText().catch(() => null));
  assert(sysText === null || sysText.includes('가나다'),
    `[복사하기] 시스템 클립보드: ${sysText === null ? '(읽기 불가 — 내부 검증으로 충분)' : `"${sysText}"`}`);

  // ── 3. [붙이기] 버튼 → 내부 클립보드 fast path ─────────────────────
  await page.keyboard.press('End');
  await page.evaluate(() => new Promise(r => setTimeout(r, 300)));
  const before = await getParaText(page, 0, 0, 200);
  await clickToolbarButton(page, 'edit:paste');
  const after = await getParaText(page, 0, 0, 200);
  await screenshot(page, 'tbcmd-02-internal-paste');
  assert(after.length > before.length && after.includes('가나다'),
    `[붙이기] 내부 fast path: "${before}" → "${after}"`);

  // ── 4. [오려두기] 버튼 → 텍스트 제거 + [붙이기]로 복원 ─────────────
  await selectAll(page);
  await clickToolbarButton(page, 'edit:cut');
  const afterCut = await getParaText(page, 0, 0, 200);
  await screenshot(page, 'tbcmd-03-cut');
  assert(afterCut === '', `[오려두기] 후 본문 비어 있음: "${afterCut}"`);
  const cutClip = await page.evaluate(() => window.__wasm?.getClipboardText() ?? '');
  assert(cutClip.includes('가나다'), `[오려두기] 내부 클립보드에 보존: "${cutClip}"`);
  await clickToolbarButton(page, 'edit:paste');
  const restored = await getParaText(page, 0, 0, 200);
  await screenshot(page, 'tbcmd-04-paste-restore');
  assert(restored.includes('가나다'), `[붙이기] 오려둔 텍스트 복원: "${restored}"`);

  // ── 5. [모양 복사] 비활성 표시 ──────────────────────────────────────
  const fmtCopyDisabled = await page.$eval(
    '#icon-toolbar .tb-btn[data-cmd="edit:format-copy"]', el => el.disabled,
  );
  assert(fmtCopyDisabled === true, `[모양 복사] 버튼 disabled (${fmtCopyDisabled})`);

  // ── 6. [문단 번호] 토글 ────────────────────────────────────────────
  await clickEditArea(page);
  const propsBefore = await getParaProps(page);
  assert(!propsBefore?.headType || propsBefore.headType === 'None',
    `문단 번호 전 headType=None (${propsBefore?.headType})`);
  await clickToolbarButton(page, 'format:toggle-numbering');
  const propsNum = await getParaProps(page);
  await screenshot(page, 'tbcmd-05-numbering');
  assert(propsNum?.headType === 'Number', `[문단 번호] 후 headType=Number (${propsNum?.headType})`);
  assert((propsNum?.paraLevel ?? -1) === 0, `[문단 번호] 후 paraLevel=0 (${propsNum?.paraLevel})`);

  // ── 7. [수준▼]/[수준▲] — 번호 문단 paraLevel 증감 ──────────────────
  await clickToolbarButton(page, 'format:level-decrease'); // 수준▼ = 한 수준 감소 = paraLevel +1
  const propsDown = await getParaProps(page);
  await screenshot(page, 'tbcmd-06-level-down');
  assert(propsDown?.paraLevel === 1, `[수준▼] paraLevel 0→1 (${propsDown?.paraLevel})`);
  assert(propsDown?.headType === 'Number', `[수준▼] headType 보존 (${propsDown?.headType})`);
  await clickToolbarButton(page, 'format:level-increase'); // 수준▲ = 한 수준 증가 = paraLevel -1
  const propsUp = await getParaProps(page);
  await screenshot(page, 'tbcmd-07-level-up');
  assert(propsUp?.paraLevel === 0, `[수준▲] paraLevel 1→0 (${propsUp?.paraLevel})`);

  // 경계: paraLevel 0에서 [수준▲] → no-op (범위 밖)
  await clickToolbarButton(page, 'format:level-increase');
  const propsEdge = await getParaProps(page);
  assert(propsEdge?.paraLevel === 0, `[수준▲] 경계(0)에서 no-op (${propsEdge?.paraLevel})`);

  // ── 8. 번호 없는 일반 문단 — 수준 변경 no-op + 토스트 안내 ─────────
  await clickToolbarButton(page, 'format:toggle-numbering'); // 번호 해제
  const propsPlain = await getParaProps(page);
  assert(propsPlain?.headType === 'None' || !propsPlain?.headType,
    `번호 해제 후 headType=None (${propsPlain?.headType})`);
  await clickToolbarButton(page, 'format:level-decrease');
  const propsPlainAfter = await getParaProps(page);
  const toastText = await page.evaluate(
    () => document.getElementById('rhwp-toast-container')?.textContent ?? '',
  );
  await screenshot(page, 'tbcmd-08-plain-level-toast');
  assert((propsPlainAfter?.paraLevel ?? 0) === (propsPlain?.paraLevel ?? 0),
    `일반 문단 [수준▼] no-op (paraLevel ${propsPlain?.paraLevel} → ${propsPlainAfter?.paraLevel})`);
  assert(toastText.includes('수준 변경'), `일반 문단 [수준▼] 토스트 안내: "${toastText.trim()}"`);
});
