/**
 * @rhwp/editor — HWP 에디터를 iframe으로 임베드
 *
 * 사용법:
 *   import { createEditor } from '@rhwp/editor';
 *   const editor = await createEditor('#container');
 *   await editor.loadFile(buffer, 'document.hwp');
 *
 * 본 제품은 한글과컴퓨터의 한글 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.
 */

import { EditorTransport } from './transport.js';

const DEFAULT_STUDIO_URL = 'https://edwardkim.github.io/rhwp/';

/**
 * HWP 에디터를 생성하여 지정된 컨테이너에 마운트합니다.
 *
 * @param container - CSS 셀렉터 또는 HTMLElement
 * @param options - 에디터 옵션
 * @returns RhwpEditor 인스턴스
 *
 * @example
 * ```javascript
 * const editor = await createEditor('#editor');
 * await editor.loadFile(hwpBuffer, 'sample.hwp');
 * console.log(await editor.pageCount());
 * ```
 */
export async function createEditor(container, options = {}) {
  const el = typeof container === 'string'
    ? document.querySelector(container)
    : container;

  if (!el) {
    throw new Error(`Container not found: ${container}`);
  }

  let studioUrl = options.studioUrl || DEFAULT_STUDIO_URL;
  if (options.renderer !== undefined) {
    if (!['auto', 'canvas2d', 'canvaskit'].includes(options.renderer)) {
      throw new TypeError(`Unsupported renderer: ${options.renderer}`);
    }
    const resolvedStudioUrl = new URL(studioUrl, document.baseURI);
    resolvedStudioUrl.searchParams.set('renderer', options.renderer);
    studioUrl = resolvedStudioUrl.href;
  }

  // iframe 생성
  const iframe = document.createElement('iframe');
  iframe.src = studioUrl;
  iframe.style.width = options.width || '100%';
  iframe.style.height = options.height || '100%';
  iframe.style.border = 'none';
  iframe.allow = 'clipboard-read; clipboard-write';
  el.appendChild(iframe);

  // iframe 로드 대기
  await new Promise((resolve) => {
    iframe.addEventListener('load', resolve, { once: true });
  });

  // WASM 초기화 대기 (ready 메서드로 확인)
  let transport;
  try {
    transport = new EditorTransport(iframe, studioUrl, {
      requestTimeoutMs: options.requestTimeoutMs,
      handshakeTimeoutMs: options.handshakeTimeoutMs,
    });
    await transport.connect();
    const editor = new RhwpEditor(iframe, transport);
    await editor._waitReady();
    return editor;
  } catch (error) {
    transport?.destroy();
    iframe.remove();
    throw error;
  }
}

/**
 * HWP 에디터 인스턴스
 *
 * iframe 내부의 rhwp-studio와 postMessage로 통신합니다.
 */
export class RhwpEditor {
  constructor(iframe, transport) {
    this._iframe = iframe;
    this._transport = transport;
  }

  /**
   * iframe에 요청을 보내고 응답을 기다립니다.
   * @internal
   */
  _request(method, params = {}) {
    return this._transport.request(method, params);
  }

  /** WASM 초기화 완료 대기 @internal */
  async _waitReady() {
    for (let i = 0; i < 30; i++) {
      try {
        const result = await this._request('ready');
        if (result) return;
      } catch {
        // 아직 준비 안 됨 — 재시도
      }
      await new Promise((r) => setTimeout(r, 500));
    }
    throw new Error('Editor initialization timeout');
  }

  /**
   * HWP 파일을 로드합니다.
   *
   * @param data - HWP 파일의 ArrayBuffer 또는 Uint8Array
   * @param fileName - 파일 이름 (선택)
   * @param options - 로드 옵션 (선택)
   * @param options.skipUnsavedGuard - 미저장 변경 확인 없이 문서 교체
   * @param options.suppressDialogs - 로드 후 안내창(HWPX 검증, 로컬 글꼴 감지) 없이 열기.
   *   임베드 환경에서 안내창의 사용자 선택을 기다리느라 loadFile 응답이 지연/교착되는
   *   것을 방지한다. 검증 경고는 '그대로 열기'로 처리되고, 글꼴은 웹 대체 글꼴로 표시된다.
   * @returns { pageCount: number }
   *
   * @example
   * ```javascript
   * const resp = await fetch('document.hwp');
   * const buffer = await resp.arrayBuffer();
   * const result = await editor.loadFile(buffer, 'document.hwp');
   * console.log(`${result.pageCount}페이지`);
   * ```
   */
  async loadFile(data, fileName = 'document.hwp', options = {}) {
    return this._request('loadFile', {
      data,
      fileName,
      skipUnsavedGuard: options.skipUnsavedGuard === true,
      suppressDialogs: options.suppressDialogs === undefined || options.suppressDialogs === true,
    });
  }

  /**
   * 현재 문서의 페이지 수를 반환합니다.
   * @returns 페이지 수
   */
  async pageCount() {
    return this._request('pageCount');
  }

  /**
   * 특정 페이지를 SVG 문자열로 렌더링합니다.
   * @param page - 0부터 시작하는 페이지 번호
   * @returns SVG 문자열
   */
  async getPageSvg(page = 0) {
    return this._request('getPageSvg', { page });
  }

  /**
   * 선택된 renderer와 페이지별 CanvasKit readiness 진단을 반환합니다.
   * @param page - 0부터 시작하는 페이지 번호
   */
  async getRendererDiagnostics(page = 0) {
    if (!Number.isSafeInteger(page) || page < 0) {
      throw new TypeError('page must be a non-negative safe integer');
    }
    if (!this._transport.supports('renderer-diagnostics-v1')) {
      throw new Error('Renderer diagnostics v1 is not supported by this Studio');
    }
    const result = await this._request('getRendererDiagnostics', { page });
    if (result?.schemaVersion !== 1 || result?.page?.index !== page) {
      throw new Error('Studio returned invalid renderer diagnostics v1');
    }
    return result;
  }

  /**
   * 현재 문서를 HWP 바이너리로 내보냅니다.
   * @returns {Promise<Uint8Array>} HWP 파일 bytes
   */
  async exportHwp() {
    const result = await this._request('exportHwp');
    return result instanceof Uint8Array ? result : new Uint8Array(result || []);
  }

  /**
   * 현재 문서를 HWPX(ZIP+XML) 바이너리로 내보냅니다.
   * @returns {Promise<Uint8Array>} HWPX 파일 bytes
   */
  async exportHwpx() {
    const result = await this._request('exportHwpx');
    return result instanceof Uint8Array ? result : new Uint8Array(result || []);
  }

  /** 현재 문서를 HML(XML) 바이너리로 내보냅니다. */
  async exportHml() {
    const result = await this._request('exportHml');
    return result instanceof Uint8Array ? result : new Uint8Array(result || []);
  }

  /** 현재 문서의 HML 저장 가능 여부와 blocker를 반환합니다. */
  async getHmlSaveState() {
    return this._request('getHmlSaveState');
  }

  /**
   * HWP 직렬화 + 자기 재로드 검증 메타데이터를 반환합니다 (#178).
   *
   * 검증 메타데이터만 반환하며, 실제 HWP bytes 가 필요하면 `exportHwp()` 를 별도 호출하세요.
   *
   * @returns {Promise<{bytesLen: number, pageCountBefore: number, pageCountAfter: number, recovered: boolean}>}
   */
  async exportHwpVerify() {
    return this._request('exportHwpVerify');
  }

  /**
   * 내보내기 바이트의 영속화(업로드/핸드오프) 완료를 스튜디오에 통지합니다.
   *
   * dirty 상태를 해제하고 자동복구 draft의 IndexedDB 삭제 "완료"까지 기다린 뒤
   * resolve합니다 — resolve 이후 창을 닫아도 안전합니다. 업로드 실패 시에는
   * 호출하지 마세요(백업 draft가 보존되어야 합니다).
   *
   * 스튜디오가 `notify-saved-v1` capability를 광고하지 않으면(구버전 또는
   * legacy 폴백 연결) 요청을 보내지 않고 명시적으로 실패합니다.
   *
   * @param fileName - 호스트가 저장에 사용한 파일 이름 (선택)
   * @returns {Promise<{ ok: true, wasDirty: boolean }>}
   */
  async notifySaved(fileName) {
    if (!this._transport.supports('notify-saved-v1')) {
      throw new Error('notifySaved is not supported by this Studio');
    }
    const params = typeof fileName === 'string' && fileName.length > 0 ? { fileName } : {};
    return this._request('notifySaved', params);
  }

  /**
   * xyren-edit-v1 capability 확인 — 미지원 스튜디오에는 요청을 보내지 않는다.
   * @internal
   */
  _requireEdit(method) {
    if (!this._transport.supports('xyren-edit-v1')) {
      throw new Error(`${method} is not supported by this Studio (xyren-edit-v1)`);
    }
  }

  /**
   * 새 빈 문서를 생성합니다.
   * @param options.skipUnsavedGuard - 편집 중 문서 교체 확인창을 건너뜁니다. 기본 true
   * @returns {Promise<{pageCount: number}>}
   */
  async createNewDocument(options = {}) {
    this._requireEdit('createNewDocument');
    return this._request('createNewDocument', {
      skipUnsavedGuard: options.skipUnsavedGuard ?? true,
    });
  }

  /**
   * 현재 커서 위치에 일반 텍스트를 삽입합니다 (줄바꿈은 문단 분리).
   * @param {string} text
   * @returns {Promise<{insertedChars: number, pageCount: number}>}
   */
  async insertText(text) {
    this._requireEdit('insertText');
    return this._request('insertText', { text });
  }

  /**
   * 선택 영역 존재 여부를 반환합니다.
   * @returns {Promise<boolean>}
   */
  async hasSelection() {
    this._requireEdit('hasSelection');
    return this._request('hasSelection');
  }

  /**
   * 현재 선택 범위(start/end DocumentPosition)를 반환합니다. 없으면 null.
   * @returns {Promise<object|null>}
   */
  async getSelection() {
    this._requireEdit('getSelection');
    return this._request('getSelection');
  }

  /**
   * 현재 선택 영역의 plain text를 반환합니다.
   * @returns {Promise<string>}
   */
  async getSelectedText() {
    this._requireEdit('getSelectedText');
    return this._request('getSelectedText');
  }

  /**
   * 현재 선택 영역을 plain text로 대체합니다.
   * @param {string} text
   * @returns {Promise<{replacedChars: number, pageCount: number}>}
   */
  async replaceSelection(text) {
    this._requireEdit('replaceSelection');
    return this._request('replaceSelection', { text });
  }

  /**
   * 현재 본문 선택 영역을 이미지로 대체합니다.
   * @param {Uint8Array|ArrayBuffer} data - 이미지 바이트
   * @param options.extension - 확장자 (기본 png)
   * @param options.naturalWidth / options.naturalHeight - 원본 픽셀 크기
   * @param options.fileName - 설명에 쓸 파일 이름
   * @returns {Promise<{inserted: boolean, pageCount: number, controlIndex?: number}>}
   */
  async replaceSelectionWithImage(data, options = {}) {
    this._requireEdit('replaceSelectionWithImage');
    return this._request('replaceSelectionWithImage', {
      data: data instanceof Uint8Array ? data : new Uint8Array(data),
      extension: options.extension ?? 'png',
      naturalWidth: options.naturalWidth ?? 1,
      naturalHeight: options.naturalHeight ?? 1,
      fileName: options.fileName ?? 'worksheet-ai-image.png',
    });
  }

  /**
   * linesegs를 전체 재계산(reflow)합니다.
   * @returns {Promise<{reflowed: number, pageCount: number}>}
   */
  async reflowLinesegs() {
    this._requireEdit('reflowLinesegs');
    return this._request('reflowLinesegs');
  }

  /**
   * 미저장 변경 여부를 반환합니다 (저장 완료 통지는 notifySaved 사용).
   * @returns {Promise<boolean>}
   */
  async isDirty() {
    this._requireEdit('isDirty');
    return this._request('isDirty');
  }

  /**
   * 구역의 문단 수를 반환합니다.
   * @returns {Promise<number>}
   */
  async getParagraphCount(section = 0) {
    this._requireEdit('getParagraphCount');
    return this._request('getParagraphCount', { section });
  }

  /**
   * 문단의 글자 수를 반환합니다.
   * @returns {Promise<number>}
   */
  async getParagraphLength(section, para) {
    this._requireEdit('getParagraphLength');
    return this._request('getParagraphLength', { section, para });
  }

  /**
   * 문단 텍스트 일부를 반환합니다.
   * @returns {Promise<string>}
   */
  async getTextRange(section, para, charOffset, count) {
    this._requireEdit('getTextRange');
    return this._request('getTextRange', { section, para, charOffset, count });
  }

  /**
   * 지정 위치의 글자 서식을 반환합니다.
   * @returns {Promise<object>}
   */
  async getCharPropertiesAt(section, para, charOffset) {
    this._requireEdit('getCharPropertiesAt');
    return this._request('getCharPropertiesAt', { section, para, charOffset });
  }

  /**
   * iframe 엘리먼트를 반환합니다.
   */
  get element() {
    return this._iframe;
  }

  /**
   * 에디터를 제거합니다.
   */
  destroy() {
    this._transport.destroy();
    this._iframe.remove();
  }
}
