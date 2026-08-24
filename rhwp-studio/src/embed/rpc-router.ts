import type { HmlSaveState } from '../core/hml-save-capability.ts';
import type {
  CanvasKitRenderModeRequest,
  CanvasKitSurfaceRequest,
  LayerRenderProfile,
  RenderBackend,
  RenderBackendRequest,
} from '../view/render-backend.ts';

export interface EmbedNotifySavedResult {
  ok: true;
  wasDirty: boolean;
}

export interface EmbedRpcHandlers {
  ready(): Promise<boolean>;
  loadFile(
    data: Uint8Array,
    fileName: string,
    skipUnsavedGuard: boolean,
    suppressDialogs: boolean,
  ): Promise<{ pageCount: number }>;
  pageCount(): Promise<number>;
  getRendererDiagnostics(page: number): Promise<EmbedRendererDiagnosticsV1>;
  getPageSvg(page: number): Promise<string>;
  exportHwp(): Promise<Uint8Array>;
  exportHwpx(): Promise<Uint8Array>;
  exportHml(): Promise<Uint8Array>;
  getHmlSaveState(): Promise<HmlSaveState>;
  exportHwpVerify(): Promise<unknown>;
  notifySaved(fileName?: string): Promise<EmbedNotifySavedResult>;
  // ---- xyren-edit-v1: 호스트 주도 편집 표면 (craftnote AI 편집기 통합) ----
  createNewDocument(skipUnsavedGuard: boolean): Promise<{ pageCount: number }>;
  insertText(text: string): Promise<{ insertedChars: number; pageCount: number }>;
  hasSelection(): Promise<boolean>;
  getSelection(): Promise<unknown>;
  /** 비어 있지 않은 선택이 없어도 현재 본문 커서 좌표를 반환한다. */
  getCursorPosition(): Promise<unknown>;
  getSelectedText(): Promise<string>;
  getSelectedContent(): Promise<{ text: string; html: string }>;
  /** 개체(그림 등) 선택 정보 — 텍스트 선택과 별개. 없으면 null. */
  getSelectedObject(): Promise<unknown>;
  getSelectedImageData(): Promise<{ data: Uint8Array; mimeType: string }>;
  replaceSelection(text: string): Promise<{ replacedChars: number; pageCount: number }>;
  replaceSelectionWithImage(
    data: Uint8Array,
    extension: string,
    naturalWidth: number,
    naturalHeight: number,
    fileName: string,
  ): Promise<{ inserted: boolean; pageCount: number; controlIndex?: number }>;
  reflowLinesegs(): Promise<{ reflowed: number; pageCount: number }>;
  isDirty(): Promise<boolean>;
  getParagraphCount(section: number): Promise<number>;
  getParagraphLength(section: number, para: number): Promise<number>;
  getTextRange(section: number, para: number, charOffset: number, count: number): Promise<string>;
  getCharPropertiesAt(section: number, para: number, charOffset: number): Promise<unknown>;
}

export interface EmbedRendererDiagnosticsV1 {
  schemaVersion: 1;
  request: EmbedRendererRuntimeRequestV1 | null;
  initialized: boolean;
  initializationError: string | null;
  effectiveBackend: 'canvas2d' | 'canvaskit' | null;
  backendFallbackReason: string | null;
  selection: unknown;
  page: { index: number; canvaskit: unknown };
}

export interface EmbedRendererRuntimeRequestV1 {
  backend: Omit<RenderBackendRequest, 'backend'> & { backend: RenderBackend };
  canvaskitMode: CanvasKitRenderModeRequest;
  canvaskitSurface: CanvasKitSurfaceRequest;
  renderProfile: LayerRenderProfile;
}

function asParams(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null ? value as Record<string, unknown> : {};
}

function asBytes(value: unknown, allowLegacyArray: boolean): Uint8Array {
  if (value instanceof Uint8Array) return value;
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (allowLegacyArray && Array.isArray(value)) return new Uint8Array(value);
  throw new Error('binary data (Uint8Array/ArrayBuffer) is required');
}

export async function routeEmbedRequest(
  method: string,
  rawParams: unknown,
  handlers: EmbedRpcHandlers,
  allowLegacyArray = false,
): Promise<unknown> {
  const params = asParams(rawParams);
  switch (method) {
    case 'ready': return handlers.ready();
    case 'loadFile':
      return handlers.loadFile(
        asBytes(params.data, allowLegacyArray),
        typeof params.fileName === 'string' ? params.fileName : 'document.hwp',
        params.skipUnsavedGuard === true,
        params.suppressDialogs === true,
      );
    case 'pageCount': return handlers.pageCount();
    case 'getRendererDiagnostics': {
      const page = params.page ?? 0;
      if (!Number.isSafeInteger(page) || (page as number) < 0) {
        throw new Error('page must be a non-negative safe integer');
      }
      return handlers.getRendererDiagnostics(page as number);
    }
    case 'getPageSvg': return handlers.getPageSvg(
      typeof params.page === 'number' ? params.page : 0,
    );
    case 'exportHwp': return handlers.exportHwp();
    case 'exportHwpx': return handlers.exportHwpx();
    case 'exportHml': return handlers.exportHml();
    case 'getHmlSaveState': return handlers.getHmlSaveState();
    case 'exportHwpVerify': return handlers.exportHwpVerify();
    case 'notifySaved': return handlers.notifySaved(
      typeof params.fileName === 'string' && params.fileName.length > 0
        ? params.fileName
        : undefined,
    );
    // ---- xyren-edit-v1 ----
    case 'createNewDocument': return handlers.createNewDocument(params.skipUnsavedGuard !== false);
    case 'insertText': return handlers.insertText(String(params.text ?? ''));
    case 'hasSelection': return handlers.hasSelection();
    case 'getSelection': return handlers.getSelection();
    case 'getCursorPosition': return handlers.getCursorPosition();
    case 'getSelectedText': return handlers.getSelectedText();
    case 'getSelectedContent': return handlers.getSelectedContent();
    case 'getSelectedObject': return handlers.getSelectedObject();
    case 'getSelectedImageData': return handlers.getSelectedImageData();
    case 'replaceSelection': return handlers.replaceSelection(String(params.text ?? ''));
    case 'replaceSelectionWithImage': return handlers.replaceSelectionWithImage(
      asBytes(params.data, allowLegacyArray),
      typeof params.extension === 'string' && params.extension ? params.extension : 'png',
      typeof params.naturalWidth === 'number' ? params.naturalWidth : 1,
      typeof params.naturalHeight === 'number' ? params.naturalHeight : 1,
      typeof params.fileName === 'string' && params.fileName ? params.fileName : 'worksheet-ai-image.png',
    );
    case 'reflowLinesegs': return handlers.reflowLinesegs();
    case 'isDirty': return handlers.isDirty();
    case 'getParagraphCount': return handlers.getParagraphCount(
      typeof params.section === 'number' ? params.section : 0,
    );
    case 'getParagraphLength': return handlers.getParagraphLength(
      typeof params.section === 'number' ? params.section : 0,
      typeof params.para === 'number' ? params.para : 0,
    );
    case 'getTextRange': return handlers.getTextRange(
      typeof params.section === 'number' ? params.section : 0,
      typeof params.para === 'number' ? params.para : 0,
      typeof params.charOffset === 'number' ? params.charOffset : 0,
      typeof params.count === 'number' ? params.count : 0,
    );
    case 'getCharPropertiesAt': return handlers.getCharPropertiesAt(
      typeof params.section === 'number' ? params.section : 0,
      typeof params.para === 'number' ? params.para : 0,
      typeof params.charOffset === 'number' ? params.charOffset : 0,
    );
    default: throw new Error(`Unknown method: ${method}`);
  }
}
