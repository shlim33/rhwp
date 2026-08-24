import assert from 'node:assert/strict';
import { createNewDocument, loadApp, runTest } from './helpers.mjs';

const sleep = (page, ms) => page.evaluate((delay) => new Promise((resolve) => setTimeout(resolve, delay)), ms);

runTest('xyren 신규 HWPX 마지막 표 아래 입력', async ({ page }) => {
  page.on('console', (message) => console.log(`[browser:${message.type()}] ${message.text()}`));
  page.on('pageerror', (error) => console.error(`[browser:pageerror] ${error.message}`));
  await loadApp(page);
  await createNewDocument(page);
  const created = await page.evaluate(() => {
    const bridge = window.__wasm;
    const result = JSON.parse(bridge.doc.createTableEx(JSON.stringify({
      sectionIdx: 0, paraIdx: 0, charOffset: 0,
      rowCount: 2, colCount: 2, treatAsChar: true,
    })));
    window.__inputHandler.cursor.moveTo({
      sectionIndex: 0, paragraphIndex: 0, charOffset: 0,
      parentParaIndex: result.paraIdx, controlIndex: result.controlIdx,
      cellIndex: 3, cellParaIndex: 0,
    });
    window.__inputHandler.focus();
    return {
      result,
      sourceFormat: bridge.getSourceFormat(),
      fileName: bridge.fileName,
      paragraphCount: bridge.doc.getParagraphCount(0),
    };
  });
  assert.equal(created.sourceFormat, 'hwpx');
  assert.equal(created.fileName, '새 문서.hwpx');
  assert.equal(created.paragraphCount, 1);
  assert.equal(created.result.ok, true);

  await page.keyboard.type('값3');
  await sleep(page, 100);
  const filledCell = await page.evaluate(() => window.__inputHandler.cursor.getPosition());
  assert.equal(filledCell.charOffset, 2);

  await page.keyboard.press('ArrowDown');
  await sleep(page, 150);
  const escaped = await page.evaluate(() => window.__inputHandler.cursor.getPosition());
  assert.equal(escaped.paragraphIndex, 0);
  assert.equal(escaped.charOffset, 1);
  assert.equal(escaped.parentParaIndex, undefined);

  // 줄 끝 affinity가 설정된 End 뒤에도 같은 구조적 경계에서 빠져나와야 한다.
  await page.evaluate(({ paraIdx, controlIdx }) => {
    window.__inputHandler.cursor.moveTo({
      sectionIndex: 0, paragraphIndex: 0, charOffset: 2,
      parentParaIndex: paraIdx, controlIndex: controlIdx,
      cellIndex: 3, cellParaIndex: 0,
    });
    window.__inputHandler.focus();
  }, created.result);
  await page.keyboard.press('End');
  await page.keyboard.press('ArrowDown');
  await sleep(page, 150);
  const escapedAfterEnd = await page.evaluate(() => window.__inputHandler.cursor.getPosition());
  assert.equal(escapedAfterEnd.paragraphIndex, 0);
  assert.equal(escapedAfterEnd.charOffset, 1);
  assert.equal(escapedAfterEnd.parentParaIndex, undefined);

  await page.keyboard.press('Enter');
  await page.keyboard.type('표 아래 본문');
  await sleep(page, 200);
  const after = await page.evaluate(() => {
    const doc = window.__wasm.doc;
    const count = doc.getParagraphCount(0);
    const last = count - 1;
    return {
      count,
      text: doc.getTextRange(0, last, 0, doc.getParagraphLength(0, last)),
    };
  });
  assert.equal(after.count, 2);
  assert.equal(after.text, '표 아래 본문');

  // embed 호스트 좌표는 편집기 내부 logical offset이 아니라 UTF-16 평문 offset이다.
  // surrogate pair 두 개와 inline table slot이 함께 있어도 둘을 각각 정확히 변환한다.
  await createNewDocument(page);
  const hostFixture = await page.evaluate(() => {
    const bridge = window.__wasm;
    bridge.doc.insertTextLogical(0, 0, 0, 'A😀😀B');
    const table = JSON.parse(bridge.doc.createTableEx(JSON.stringify({
      sectionIdx: 0, paraIdx: 0, charOffset: 4,
      rowCount: 1, colCount: 1, treatAsChar: true,
    })));
    window.__inputHandler.cursor.moveTo({
      sectionIndex: 0, paragraphIndex: 0, charOffset: 0,
    });
    window.__inputHandler.focus();
    return table;
  });
  assert.equal(hostFixture.ok, true);
  await page.keyboard.press('Home');
  await page.keyboard.down('Shift');
  await page.keyboard.press('End');
  await page.keyboard.up('Shift');
  const hostSelection = await page.evaluate(() => ({
    raw: window.__inputHandler.getSelectionRange(),
    host: window.__inputHandler.getHostSelectionRange(),
    text: window.__inputHandler.getSelectedText(),
  }));
  assert.equal(hostSelection.text, 'A😀😀B');
  assert.equal(hostSelection.host.start.charOffset, 0);
  assert.equal(hostSelection.host.end.charOffset, 'A😀😀B'.length);
  assert.notEqual(hostSelection.raw.end.charOffset, hostSelection.host.end.charOffset);
}, { skipLoadApp: true });
