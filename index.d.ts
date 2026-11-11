export type Format =
  | 'docx'
  | 'docm'
  | 'dotx'
  | 'dotm'
  | 'doc'
  | 'dot'
  | 'odt'
  | 'ott'
  | 'fodt'
  | 'ods'
  | 'ots'
  | 'fods'
  | 'odp'
  | 'otp'
  | 'fodp'
  | 'pptx'
  | 'pptm'
  | 'potx'
  | 'potm'
  | 'ppsx'
  | 'ppsm'
  | 'xlsx'
  | 'xlsm'
  | 'xltx'
  | 'xltm'
  | 'xls'
  | 'xlt'
  | 'md'
  | 'markdown'

export type Target = 'pdf' | 'png'

export interface ConvertOptions {
  to: Target | string
  from?: Format | string
  fontsDir?: string
  pages?: number[]
  scale?: number
  map?: boolean
}

export interface LayoutBox {
  x: number
  y: number
  width: number
  height: number
}

export type LayoutKind =
  | 'paragraph'
  | 'line'
  | 'run'
  | 'table'
  | 'cell'
  | 'image'
  | 'chart'
  | 'shape'
  | 'slide'
  | 'sheetCell'

export interface LayoutItem {
  page: number
  kind: LayoutKind
  ref: Record<string, string | number | boolean>
  box: LayoutBox
  text?: { start: number; end: number }
}

export interface LayoutMap {
  version: 1
  pages: Array<{ width: number; height: number }>
  items: LayoutItem[]
}

type Result<O> = O extends { map: true }
  ? O extends { to: 'png' }
    ? { png: Buffer[]; map: LayoutMap }
    : { pdf: Buffer; map: LayoutMap }
  : O extends { to: 'png' }
    ? Buffer[]
    : Buffer

export function convert<O extends ConvertOptions>(
  input: Uint8Array | ArrayBuffer,
  opts: O
): Promise<Result<O>>

export function convertSync<O extends ConvertOptions>(
  input: Uint8Array | ArrayBuffer,
  opts: O
): Result<O>

export function detect(input: Uint8Array | ArrayBuffer): Format | null

export function supports(from: string, to: string): boolean

export function conversions(): Array<{ from: string; to: string }>

export const FONTS_DIR: string
