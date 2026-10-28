const test = require('brittle')
const fs = require('bare-fs')
const path = require('bare-path')
const converter = require('..')

const fixture = (name) => fs.readFileSync(path.join(__dirname, 'fixtures', name))

function inspect(pdf) {
  const source = pdf.toString('latin1')
  const pages = source.match(/\/Type\s*\/Page\b/g) || []
  const box = source.match(/\/MediaBox\s*\[\s*([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s*\]/)
  return {
    header: source.slice(0, 5),
    pageCount: pages.length,
    width: box ? Number(box[3]) - Number(box[1]) : 0,
    height: box ? Number(box[4]) - Number(box[2]) : 0,
    hasFont: /\/Font\b/.test(source),
    hasImage: /\/Subtype\s*\/Image\b/.test(source)
  }
}

test('detects the input format from magic bytes', (t) => {
  t.is(converter.detect(fixture('minimal-table-unicode.docx')), 'docx')
  t.is(converter.detect(Buffer.from('%PDF-1.4\n')), null)
  t.is(converter.detect(Buffer.from([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1])), 'doc')
  t.is(converter.detect(Buffer.from('plain text')), null)
})

test('lists supported conversions', (t) => {
  t.ok(converter.supports('docx', 'pdf'))
  t.ok(converter.supports('.DOCX', 'PDF'))
  t.absent(converter.supports('pdf', 'pdf'))
  t.ok(converter.supports('potx', 'png'))
  const formats = [
    'docx',
    'docm',
    'dotx',
    'dotm',
    'doc',
    'dot',
    'odt',
    'ott',
    'fodt',
    'ods',
    'ots',
    'fods',
    'odp',
    'otp',
    'fodp',
    'pptx',
    'pptm',
    'potx',
    'potm',
    'ppsx',
    'ppsm',
    'xlsx',
    'xlsm',
    'xltx',
    'xltm',
    'xls',
    'xlt',
    'md',
    'markdown'
  ]
  t.alike(
    converter.conversions().map((c) => `${c.from}>${c.to}`),
    formats.flatMap((f) => [`${f}>pdf`, `${f}>png`])
  )
})

test('converts a minimal docx with a table and unicode text to pdf', async (t) => {
  const pdf = inspect(await converter.convert(fixture('minimal-table-unicode.docx'), { to: 'pdf' }))
  t.is(pdf.header, '%PDF-')
  t.ok(pdf.pageCount >= 1)
  t.ok(pdf.hasFont)
  t.absent(pdf.hasImage)
})

test('converts a styled docx to a letter sized pdf', async (t) => {
  const pdf = inspect(
    await converter.convert(fixture('sdk-sample.docx'), { from: 'docx', to: 'pdf' })
  )
  t.ok(Math.abs(pdf.width - 612) < 1)
  t.ok(Math.abs(pdf.height - 792) < 1)
})

test('converts a pptx deck to one pdf page per slide', async (t) => {
  const pptx = fs.readFileSync(path.join(__dirname, '..', 'bench', 'corpus', 'sdk-sample.pptx'))
  t.is(converter.detect(pptx), 'pptx')
  const pdf = inspect(await converter.convert(pptx, { to: 'pdf' }))
  t.is(pdf.pageCount, 4)
  t.ok(Math.abs(pdf.width - 720) < 1)
  t.ok(Math.abs(pdf.height - 405) < 1)
  t.ok(pdf.hasFont)
})

test('converts an xls workbook to pdf', async (t) => {
  const xls = fixture('lo-formats.xls')
  t.is(converter.detect(xls), 'xls')
  const pdf = inspect(await converter.convert(xls, { to: 'pdf' }))
  t.ok(pdf.pageCount >= 1)
  t.ok(pdf.hasFont)
})

test('converts markdown to pdf when the format is given', async (t) => {
  const markdown = Buffer.from('# Title\n\nSome **bold** text.\n\n- one\n- two\n')
  t.is(converter.detect(markdown), null)
  const pdf = inspect(await converter.convert(markdown, { from: 'md', to: 'pdf' }))
  t.is(pdf.pageCount, 1)
  t.ok(pdf.hasFont)
})

test('converts OpenDocument spreadsheets and presentations to pdf', async (t) => {
  const ods = fs.readFileSync(path.join(__dirname, '..', 'bench', 'corpus', 'odf-cond-format.ods'))
  t.is(converter.detect(ods), 'ods')
  const sheet = inspect(await converter.convert(ods, { to: 'pdf' }))
  t.is(sheet.pageCount, 1)
  t.ok(sheet.hasFont)
  const odp = fs.readFileSync(path.join(__dirname, '..', 'bench', 'corpus', 'odf-sdk-sample.odp'))
  t.is(converter.detect(odp), 'odp')
  const slides = inspect(await converter.convert(odp, { to: 'pdf' }))
  t.is(slides.pageCount, 4)
  t.ok(Math.abs(slides.width - 720) < 1)
})

test('rejects unsupported and undetectable input', async (t) => {
  await t.exception(converter.convert(Buffer.from('nope'), { to: 'pdf' }), /could not detect/)
  await t.exception(converter.convert(fixture('sdk-sample.docx'), { to: 'html' }), /not supported/)
  await t.exception.all(converter.convert(fixture('sdk-sample.docx'), {}), /opts.to is required/)
  await t.exception.all(converter.convert('not bytes', { to: 'pdf' }), /expected a Uint8Array/)
})

test('rejects corrupt and truncated documents without crashing', async (t) => {
  const docx = fixture('minimal-table-unicode.docx')
  await t.exception(converter.convert(Buffer.from('PK\x03\x04garbage'), { to: 'pdf' }))
  await t.exception(
    converter.convert(docx.subarray(0, docx.length / 2), { from: 'docx', to: 'pdf' })
  )
  await t.exception(converter.convert(Buffer.alloc(4096, 0xd0), { from: 'doc', to: 'pdf' }))
  await t.exception(converter.convert(Buffer.alloc(0), { from: 'xlsx', to: 'pdf' }))
})

test('accepts an ArrayBuffer and a Uint8Array view into a larger buffer', async (t) => {
  const docx = fixture('minimal-table-unicode.docx')
  const copy = new ArrayBuffer(docx.length)
  new Uint8Array(copy).set(docx)
  t.is(inspect(await converter.convert(copy, { to: 'pdf' })).header, '%PDF-')

  const padded = Buffer.alloc(docx.length + 64, 0xff)
  docx.copy(padded, 32)
  const view = padded.subarray(32, 32 + docx.length)
  t.is(converter.detect(view), 'docx')
  t.is(inspect(await converter.convert(view, { to: 'pdf' })).header, '%PDF-')
})

test('uses the fonts directory it is given', async (t) => {
  const docx = fixture('minimal-table-unicode.docx')
  const pdf = inspect(await converter.convert(docx, { to: 'pdf', fontsDir: converter.FONTS_DIR }))
  t.ok(pdf.hasFont)
  await t.exception(
    converter.convert(docx, { to: 'pdf', fontsDir: path.join(__dirname, 'fixtures') }),
    /no fonts found/
  )
})

test('copies the input so it can change while the conversion runs', async (t) => {
  const docx = Buffer.from(fixture('minimal-table-unicode.docx'))
  const pending = converter.convert(docx, { to: 'pdf' })
  docx.fill(0)
  t.is(inspect(await pending).header, '%PDF-')
})

test('runs conversions concurrently and matches the synchronous output', async (t) => {
  const names = ['minimal-table-unicode.docx', 'sdk-sample.docx', 'lo-formats.xls']
  const results = await Promise.all(
    names.map((name) => converter.convert(fixture(name), { to: 'pdf' }))
  )
  for (let i = 0; i < names.length; i++) {
    const sync = converter.convertSync(fixture(names[i]), { to: 'pdf' })
    t.is(inspect(results[i]).pageCount, inspect(sync).pageCount, names[i])
    t.is(results[i].length, sync.length, names[i])
  }
})

test('convertSync converts on the calling thread and throws on failure', (t) => {
  const pdf = inspect(converter.convertSync(fixture('minimal-table-unicode.docx'), { to: 'pdf' }))
  t.is(pdf.header, '%PDF-')
  t.exception(() => converter.convertSync(Buffer.from('nope'), { to: 'pdf' }), /could not detect/)
  t.exception(() => converter.convertSync(Buffer.from('PK\x03\x04garbage'), { to: 'pdf' }))
})

test('returns a layout map next to the same pdf bytes', async (t) => {
  const input = fixture('office-features.docx')
  const plain = await converter.convert(input, { to: 'pdf' })
  const { pdf, map } = await converter.convert(input, { to: 'pdf', map: true })
  t.alike(pdf, plain)
  t.is(map.version, 1)
  t.alike(map.pages, [{ width: 792, height: 612 }])
  const first = map.items.find((i) => i.kind === 'paragraph' && i.ref.part === 'body')
  t.is(first.ref.block, 0)
  t.ok(/^[0-9A-F]{8}$/.test(first.ref.id))
  t.ok(first.box.width > 0 && first.box.height > 0)
  t.ok(map.items.some((i) => i.kind === 'run' && i.text.end > i.text.start))
  const sync = converter.convertSync(input, { to: 'pdf', map: true })
  t.alike(sync.map, map)
})

test('renders selected pages to pdf and png', async (t) => {
  const deck = fs.readFileSync(path.join(__dirname, '..', 'bench', 'corpus', 'sdk-sample.pptx'))
  t.is(inspect(await converter.convert(deck, { to: 'pdf', pages: [2] })).pageCount, 1)
  const images = await converter.convert(deck, { to: 'png', pages: [3, 0], scale: 0.5 })
  t.is(images.length, 2)
  for (const png of images) {
    t.is(png.subarray(1, 4).toString(), 'PNG')
    t.is(png.readUInt32BE(16), 360)
  }
  const { png, map } = await converter.convert(deck, { to: 'png', pages: [1], map: true })
  t.is(png.length, 1)
  t.is(map.pages.length, 4)
  t.ok(map.items.every((i) => i.page === 1))
  t.ok(map.items.some((i) => i.kind === 'slide' && i.ref.slide === 1))
  await t.exception(converter.convert(deck, { to: 'pdf', pages: [4] }), /out of range/)
  await t.exception.all(converter.convert(deck, { to: 'pdf', pages: [-1] }), /pages/)
  await t.exception.all(converter.convert(deck, { to: 'pdf', scale: 2 }), /scale/)
  await t.exception.all(converter.convert(deck, { to: 'pdf', map: 'yes' }), /map/)
})

test('maps printed spreadsheet cells to their A1 refs', async (t) => {
  const { map } = await converter.convert(fixture('office-uncached-formulas.xlsx'), {
    to: 'pdf',
    map: true
  })
  const cells = map.items.filter((i) => i.kind === 'sheetCell').map((i) => i.ref.cell)
  t.ok(cells.includes('A1'))
  t.ok(map.items.some((i) => i.kind === 'line' && i.ref.cell === 'D6'))
})
