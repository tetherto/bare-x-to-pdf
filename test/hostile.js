const test = require('brittle')
const fs = require('bare-fs')
const os = require('bare-os')
const path = require('bare-path')
const converter = require('..')
const { binding } = require('../binding')

const dir = path.join(__dirname, 'fixtures', 'hostile')
const hostile = (name) => fs.readFileSync(path.join(dir, name))
const markdown = Buffer.from('# Title\n')

const REJECTED = {
  'deep-custom-xml.docx': /nested/,
  'deep-hyperlink.docx': /nested/,
  'deep-sdt.docx': /nested/,
  'deep-unknown-elements.docx': /nested/,
  'deep-unknown-elements.pptx': /nested/,
  'nested-tables-2000.docx': /nested/,
  'dense-cells.xlsx': /too large/,
  'ods-repeated-cells.ods': /too large/
}

const cases = fs
  .readdirSync(dir)
  .filter((name) => !name.startsWith('detect-'))
  .sort()

for (const name of cases) {
  test(`hostile input ${name} converts or fails cleanly`, async (t) => {
    const from = name.endsWith('.md') ? 'md' : undefined
    const pending = converter.convert(hostile(name), { from, to: 'pdf' })
    if (REJECTED[name]) {
      await t.exception(pending, REJECTED[name])
    } else {
      const pdf = await pending
      t.is(pdf.subarray(0, 5).toString(), '%PDF-')
    }
  })
}

test('rejects option values that are not strings', async (t) => {
  for (const fontsDir of [123, true, new String(converter.FONTS_DIR), {}, [converter.FONTS_DIR]]) {
    await t.exception.all(
      converter.convert(markdown, { from: 'md', to: 'pdf', fontsDir }),
      /fontsDir must be a string/
    )
    t.exception.all(
      () => converter.convertSync(markdown, { from: 'md', to: 'pdf', fontsDir }),
      /fontsDir must be a string/
    )
  }
  await t.exception.all(
    converter.convert(markdown, { from: 123, to: 'pdf' }),
    /from must be a string/
  )
  await t.exception.all(
    converter.convert(markdown, { from: 'md', to: new String('pdf') }),
    /to must be a string/
  )
  await t.exception.all(
    converter.convert(markdown, { from: 'md', to: 'pdf', fontsDir: converter.FONTS_DIR + '\0x' }),
    /NUL/
  )
})

test('the raw binding validates its arguments', (t) => {
  const fonts = converter.FONTS_DIR
  for (const input of [123, {}, null, new ArrayBuffer(8), new DataView(new ArrayBuffer(8))]) {
    t.exception.all(() => binding.convertSync(input, 'md', 'pdf', fonts), /Uint8Array/)
  }
  t.exception.all(
    () => binding.convertSync(new Int16Array(markdown.length), 'md', 'pdf', fonts),
    /Uint8Array/
  )
  t.exception.all(() => binding.convertSync(markdown, 123, 'pdf', fonts), /strings/)
  t.exception.all(() => binding.convertSync(markdown, 'md\0x', 'pdf', fonts), /NUL/)
  t.exception.all(() => binding.convertSync(markdown, 'md', 'pdf', fonts + '\0x'), /NUL/)
  t.exception.all(() => binding.convert(123, 'md', 'pdf', fonts), /Uint8Array/)
  t.exception.all(() => binding.convert(markdown, 'md', 'pdf', 123), /strings/)
})

test('detects formats from zip entry names and the flat XML root', (t) => {
  t.is(converter.detect(hostile('detect-xlsx-with-word-text.xlsx')), 'xlsx')
  t.is(converter.detect(hostile('detect-bom.fodt')), 'fodt')
  t.is(converter.detect(hostile('detect-late-marker.fodt')), 'fodt')
  t.is(converter.detect(hostile('detect-zip-no-markers.zip')), null)
  t.is(converter.detect(Buffer.from('<?xml version="1.0"?><!-- office:document --><html/>')), null)
  t.is(converter.detect(Buffer.from('PK\x03\x04')), null)
})

test('skips font files it cannot read', async (t) => {
  const fonts = path.join(os.tmpdir(), `bare-x-to-pdf-fonts-${Date.now()}`)
  fs.mkdirSync(path.join(fonts, 'directory.ttf'), { recursive: true })
  fs.copyFileSync(
    path.join(converter.FONTS_DIR, 'LiberationSerif-Regular.ttf'),
    path.join(fonts, 'LiberationSerif-Regular.ttf')
  )
  t.teardown(() => fs.rmSync(fonts, { recursive: true, force: true }))
  const pdf = await converter.convert(markdown, { from: 'md', to: 'pdf', fontsDir: fonts })
  t.is(pdf.subarray(0, 5).toString(), '%PDF-')
})
