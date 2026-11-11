const { binding } = require('./binding')

const FONTS_DIR = require.asset('./fonts')

const TARGETS = ['pdf', 'png']

const CONVERSIONS = {
  docx: TARGETS,
  docm: TARGETS,
  dotx: TARGETS,
  dotm: TARGETS,
  doc: TARGETS,
  dot: TARGETS,
  odt: TARGETS,
  ott: TARGETS,
  fodt: TARGETS,
  ods: TARGETS,
  ots: TARGETS,
  fods: TARGETS,
  odp: TARGETS,
  otp: TARGETS,
  fodp: TARGETS,
  pptx: TARGETS,
  pptm: TARGETS,
  potx: TARGETS,
  potm: TARGETS,
  ppsx: TARGETS,
  ppsm: TARGETS,
  xlsx: TARGETS,
  xlsm: TARGETS,
  xltx: TARGETS,
  xltm: TARGETS,
  xls: TARGETS,
  xlt: TARGETS,
  md: TARGETS,
  markdown: TARGETS
}

const ODF_MIMETYPES = [
  { format: 'odt', mimetype: 'application/vnd.oasis.opendocument.text' },
  { format: 'ods', mimetype: 'application/vnd.oasis.opendocument.spreadsheet' },
  { format: 'odp', mimetype: 'application/vnd.oasis.opendocument.presentation' }
]

const OOXML_FOLDERS = [
  { format: 'docx', folder: 'word/' },
  { format: 'pptx', folder: 'ppt/' },
  { format: 'xlsx', folder: 'xl/' }
]

const COMPOUND_STREAMS = [
  { format: 'doc', name: 'WordDocument' },
  { format: 'xls', name: 'Workbook' },
  { format: 'xls', name: 'Book' }
]

const MAX_ZIP_NAMES = 65536
const FLAT_XML_HEAD = 65536

function detect(input) {
  const bytes = view(toBytes(input))
  if (startsWith(bytes, [0x50, 0x4b, 0x03, 0x04])) return detectZip(bytes)
  if (startsWith(bytes, [0xd0, 0xcf, 0x11, 0xe0])) return detectCompound(bytes) || 'doc'
  return detectFlatXml(bytes)
}

function detectZip(bytes) {
  const entries = zipEntries(bytes)
  const mimetype = entries.find((entry) => entry.name === 'mimetype')
  if (mimetype) {
    const content = storedContent(bytes, mimetype.offset)
    const odf = content && ODF_MIMETYPES.find((m) => content.startsWith(m.mimetype))
    if (odf) return odf.format
  }
  for (const { format, folder } of OOXML_FOLDERS) {
    if (entries.some((entry) => entry.name.startsWith(folder))) return format
  }
  return null
}

function zipEntries(bytes) {
  return centralDirectory(bytes) || localHeaders(bytes)
}

function centralDirectory(bytes) {
  const lowest = Math.max(0, bytes.length - 65557)
  for (let eocd = bytes.length - 22; eocd >= lowest; eocd--) {
    if (bytes.readUInt32LE(eocd) !== 0x06054b50) continue
    const count = Math.min(bytes.readUInt16LE(eocd + 10), MAX_ZIP_NAMES)
    let pos = bytes.readUInt32LE(eocd + 16)
    const entries = []
    for (let i = 0; i < count; i++) {
      if (pos + 46 > bytes.length || bytes.readUInt32LE(pos) !== 0x02014b50) return null
      const nameLength = bytes.readUInt16LE(pos + 28)
      const extraLength = bytes.readUInt16LE(pos + 30)
      const commentLength = bytes.readUInt16LE(pos + 32)
      const offset = bytes.readUInt32LE(pos + 42)
      entries.push({ name: bytes.toString('latin1', pos + 46, pos + 46 + nameLength), offset })
      pos += 46 + nameLength + extraLength + commentLength
    }
    return entries
  }
  return null
}

function localHeaders(bytes) {
  const entries = []
  let pos = 0
  while (
    entries.length < MAX_ZIP_NAMES &&
    pos + 30 <= bytes.length &&
    bytes.readUInt32LE(pos) === 0x04034b50
  ) {
    const compressedSize = bytes.readUInt32LE(pos + 18)
    const nameLength = bytes.readUInt16LE(pos + 26)
    const extraLength = bytes.readUInt16LE(pos + 28)
    entries.push({ name: bytes.toString('latin1', pos + 30, pos + 30 + nameLength), offset: pos })
    if (bytes.readUInt16LE(pos + 6) & 0x08) break
    pos += 30 + nameLength + extraLength + compressedSize
  }
  return entries
}

function storedContent(bytes, offset) {
  if (offset + 30 > bytes.length || bytes.readUInt32LE(offset) !== 0x04034b50) return null
  if (bytes.readUInt16LE(offset + 8) !== 0) return null
  const size = Math.min(bytes.readUInt32LE(offset + 18), 256)
  const start = offset + 30 + bytes.readUInt16LE(offset + 26) + bytes.readUInt16LE(offset + 28)
  return bytes.toString('latin1', start, Math.min(start + size, bytes.length))
}

function detectCompound(bytes) {
  for (const { format, name } of COMPOUND_STREAMS) {
    if (bytes.includes(Buffer.from(name + '\0', 'utf16le'))) return format
  }
  return null
}

function detectFlatXml(bytes) {
  const start = startsWith(bytes, [0xef, 0xbb, 0xbf]) ? 3 : 0
  const head = bytes.toString('latin1', start, Math.min(bytes.length, start + FLAT_XML_HEAD))
  let pos = 0
  for (;;) {
    while (pos < head.length && /\s/.test(head[pos])) pos++
    if (head.startsWith('<?', pos)) pos = skipPast(head, pos, '?>')
    else if (head.startsWith('<!--', pos)) pos = skipPast(head, pos, '-->')
    else if (head.startsWith('<!', pos)) pos = skipPast(head, pos, '>')
    else break
    if (pos < 0) return null
  }
  const root = head.slice(pos, head.indexOf('>', pos) + 1 || undefined)
  if (!/^<[A-Za-z_][\w.-]*:document[\s>]/.test(root)) return null
  if (root.includes('opendocument.spreadsheet')) return 'fods'
  if (root.includes('opendocument.presentation')) return 'fodp'
  return 'fodt'
}

function skipPast(text, pos, terminator) {
  const end = text.indexOf(terminator, pos)
  return end < 0 ? -1 : end + terminator.length
}

function startsWith(bytes, magic) {
  return bytes.length >= magic.length && magic.every((b, i) => bytes[i] === b)
}

function view(bytes) {
  return Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength)
}

function prepare(input, opts) {
  const bytes = toBytes(input)
  if (opts.to === undefined || opts.to === null) throw new TypeError('convert: opts.to is required')
  if (typeof opts.to !== 'string') throw new TypeError('convert: opts.to must be a string')
  if (!optional(opts.from)) throw new TypeError('convert: opts.from must be a string')
  if (!optional(opts.fontsDir)) throw new TypeError('convert: opts.fontsDir must be a string')
  if (opts.fontsDir && opts.fontsDir.includes('\0')) {
    throw new TypeError('convert: opts.fontsDir must not contain NUL characters')
  }

  const to = normalize(opts.to)
  const from = normalize(opts.from) || detect(bytes)
  if (!from) throw new Error('convert: could not detect the input format, pass opts.from')

  if (!supports(from, to)) {
    throw new Error(`convert: ${from} to ${to} is not supported`)
  }

  const args = [bytes, from, to, opts.fontsDir || FONTS_DIR]
  const request = requestOf(opts, to)
  if (request !== null) args.push(request)
  return args
}

function requestOf(opts, to) {
  const parts = []
  if (opts.map !== undefined && opts.map !== null && typeof opts.map !== 'boolean') {
    throw new TypeError('convert: opts.map must be a boolean')
  }
  if (opts.map) parts.push('map=1')
  if (opts.pages !== undefined && opts.pages !== null) {
    const valid =
      Array.isArray(opts.pages) && opts.pages.every((p) => Number.isInteger(p) && p >= 0)
    if (!valid) throw new TypeError('convert: opts.pages must be an array of page indexes from 0')
    if (opts.pages.length > 0) parts.push('pages=' + opts.pages.join(','))
  }
  if (opts.scale !== undefined && opts.scale !== null) {
    if (typeof opts.scale !== 'number' || !(opts.scale > 0)) {
      throw new TypeError('convert: opts.scale must be a positive number')
    }
    if (to !== 'png') throw new TypeError('convert: opts.scale applies only to png output')
    parts.push('scale=' + opts.scale)
  }
  if (parts.length === 0 && to === 'pdf') return null
  return parts.join(';')
}

function finish(result, to) {
  if (!Array.isArray(result)) return Buffer.from(result)
  const [data, json] = result
  const meta = JSON.parse(json)
  const bytes = Buffer.from(data)
  const files = []
  let at = 0
  for (const size of meta.sizes) {
    files.push(bytes.subarray(at, at + size))
    at += size
  }
  const output = to === 'png' ? files : files[0]
  if (meta.map === null) return output
  return to === 'png' ? { png: output, map: meta.map } : { pdf: output, map: meta.map }
}

function optional(value) {
  return value === undefined || value === null || typeof value === 'string'
}

async function convert(input, opts = {}) {
  const args = prepare(input, opts)
  const to = args[2]
  // The WebAssembly addon has no thread pool, so it converts on the calling
  // thread; run it in a worker to keep the JavaScript thread free.
  if (binding.convert === undefined) return finish(binding.convertSync(...args), to)
  return finish(await binding.convert(...args), to)
}

function convertSync(input, opts = {}) {
  const args = prepare(input, opts)
  return finish(binding.convertSync(...args), args[2])
}

function supports(from, to) {
  const targets = CONVERSIONS[normalize(from)]
  return Array.isArray(targets) && targets.includes(normalize(to))
}

function conversions() {
  return Object.entries(CONVERSIONS).flatMap(([from, tos]) => tos.map((to) => ({ from, to })))
}

function normalize(format) {
  if (typeof format !== 'string') return null
  return format.toLowerCase().replace(/^\./, '')
}

function toBytes(input) {
  if (input instanceof Uint8Array) return input
  if (input instanceof ArrayBuffer) return new Uint8Array(input)
  throw new TypeError('expected a Uint8Array or ArrayBuffer')
}

exports.convert = convert
exports.convertSync = convertSync
exports.detect = detect
exports.supports = supports
exports.conversions = conversions
exports.FONTS_DIR = FONTS_DIR
