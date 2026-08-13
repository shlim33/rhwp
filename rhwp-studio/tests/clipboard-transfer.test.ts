import test from 'node:test';
import assert from 'node:assert/strict';

import {
  clipboardItemsToDataTransfer,
  imageExtFromMime,
  type ClipboardItemLike,
} from '../src/engine/clipboard-transfer.ts';

// ─── node 환경용 DataTransfer 최소 fake ──────────────────────────────
// (브라우저 DataTransfer의 setData/getData/types/items.add 표면만 재현)

class FakeDataTransferItemList {
  files: File[] = [];
  add(file: File): void {
    this.files.push(file);
  }
  get length(): number {
    return this.files.length;
  }
}

class FakeDataTransfer {
  private data = new Map<string, string>();
  items = new FakeDataTransferItemList();

  setData(type: string, value: string): void {
    this.data.set(type, value);
  }
  getData(type: string): string {
    return this.data.get(type) ?? '';
  }
  get types(): string[] {
    const t = [...this.data.keys()];
    if (this.items.length > 0) t.push('Files');
    return t;
  }
}

(globalThis as Record<string, unknown>).DataTransfer = FakeDataTransfer;

function textItem(types: Record<string, string>): ClipboardItemLike {
  return {
    types: Object.keys(types),
    getType: async (type: string) => new Blob([types[type]], { type }),
  };
}

test('clipboardItemsToDataTransfer: text/plain·text/html은 setData로 담는다', async () => {
  const dt = await clipboardItemsToDataTransfer([
    textItem({ 'text/plain': '안녕', 'text/html': '<p>안녕</p>' }),
  ]);
  assert.equal(dt.getData('text/plain'), '안녕');
  assert.equal(dt.getData('text/html'), '<p>안녕</p>');
  assert.deepEqual([...dt.types].sort(), ['text/html', 'text/plain']);
});

test('clipboardItemsToDataTransfer: image/*는 File 항목으로 담는다', async () => {
  const png = new Blob([new Uint8Array([137, 80, 78, 71])], { type: 'image/png' });
  const dt = await clipboardItemsToDataTransfer([
    { types: ['image/png'], getType: async () => png },
  ]);
  const files = (dt.items as unknown as FakeDataTransferItemList).files;
  assert.equal(files.length, 1);
  assert.equal(files[0].name, 'clipboard.png');
  assert.equal(files[0].type, 'image/png');
  assert.ok(dt.types.includes('Files'));
});

test('clipboardItemsToDataTransfer: 소비하지 않는 타입은 무시, 빈 텍스트는 미기록', async () => {
  const dt = await clipboardItemsToDataTransfer([
    textItem({ 'text/plain': '', 'web application/json': '{}' }),
  ]);
  assert.equal(dt.types.length, 0);
});

test('clipboardItemsToDataTransfer: 한 표현의 읽기 실패는 나머지 표현을 막지 않는다', async () => {
  const dt = await clipboardItemsToDataTransfer([
    {
      types: ['text/html', 'text/plain'],
      getType: async (type: string) => {
        if (type === 'text/html') throw new Error('read fail');
        return new Blob(['살아남은 텍스트'], { type });
      },
    },
  ]);
  assert.equal(dt.getData('text/plain'), '살아남은 텍스트');
  assert.equal(dt.getData('text/html'), '');
});

test('imageExtFromMime: jpeg→jpg, 기본 png', () => {
  assert.equal(imageExtFromMime('image/png'), 'png');
  assert.equal(imageExtFromMime('image/jpeg'), 'jpg');
  assert.equal(imageExtFromMime('image/webp'), 'webp');
  assert.equal(imageExtFromMime('image'), 'png');
});
