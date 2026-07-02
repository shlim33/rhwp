import test from 'node:test';
import assert from 'node:assert/strict';

import { nextParaLevel, MIN_PARA_LEVEL, MAX_PARA_LEVEL } from '../src/engine/para-level.ts';

test('nextParaLevel: 범위 내 증감은 delta 적용 결과를 반환한다', () => {
  assert.equal(nextParaLevel(0, 1), 1);
  assert.equal(nextParaLevel(3, 1), 4);
  assert.equal(nextParaLevel(3, -1), 2);
  assert.equal(nextParaLevel(6, -1), 5);
});

test('nextParaLevel: 경계(0~6)를 벗어나면 null (호출측 no-op)', () => {
  assert.equal(nextParaLevel(MIN_PARA_LEVEL, -1), null);
  assert.equal(nextParaLevel(MAX_PARA_LEVEL, 1), null);
});

test('nextParaLevel: undefined/비정상 current는 0~6으로 정규화 후 계산한다', () => {
  assert.equal(nextParaLevel(undefined, 1), 1);   // 없음 → 0 기준
  assert.equal(nextParaLevel(undefined, -1), null);
  assert.equal(nextParaLevel(99, -1), 5);          // 6으로 클램프 후 -1
  assert.equal(nextParaLevel(-5, 1), 1);           // 0으로 클램프 후 +1
  assert.equal(nextParaLevel(2.7, 1), 3);          // 정수 절삭(2) 후 +1
});
