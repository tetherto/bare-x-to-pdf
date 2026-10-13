const test = require('brittle')
const fs = require('bare-fs')
const path = require('bare-path')
const crypto = require('bare-crypto')
const instantiate = require('bare-wasm-abi')
const { MemoryDirectory } = require('bare-wasi/memory')
const converter = require('..')
const { binding: native } = require('../binding')

// The WebAssembly addon is built with
//   bare-make generate --build build-wasm --platform wasi --arch wasm32
//   bare-make build --build build-wasm
// and these tests are skipped when it hasn't been.
const build = path.join(__dirname, '..', 'build-wasm')
const name = fs.existsSync(build) && fs.readdirSync(build).find((n) => n.endsWith('.wasm'))
const bytes = name ? fs.readFileSync(path.join(build, name)) : null
const skip = bytes === null

const fixtures = path.join(__dirname, 'fixtures')
const hostile = path.join(fixtures, 'hostile')
const markdown = Buffer.from('# Title\n')

// These carry the value 2^44, which `usize` can't hold on wasm32, so the core
// falls back where 64-bit builds clamp. They render, just not identically.
const DIFFERS_ON_32_BIT = [
  'colspan-huge.odt',
  'gridspan-huge.docx',
  'gridspan-max.docx',
  'text-space-count-huge.odp',
  'text-space-count-huge.ods',
  'text-space-count-huge.odt',
  'text-space-count-max.odt'
]

const fonts = {}
for (const font of fs.readdirSync(converter.FONTS_DIR)) {
  fonts[font] = fs.readFileSync(path.join(converter.FONTS_DIR, font))
}

// The guest sees only what it's given: a random source for Rust's `HashMap`
// and the bundled fonts at `/fonts`.
function load(opts = { random: (b) => crypto.randomFillSync(b) }) {
  return instantiate(bytes, {
    wasi: { version: 'preview1', preopens: { '/fonts': new MemoryDirectory(fonts) }, ...opts }
  })
}

function attempt(fn) {
  try {
    return { pdf: Buffer.from(fn()) }
  } catch (err) {
    return { err }
  }
}

test('the wasm addon exports only convertSync', { skip }, (t) => {
  t.alike(Object.keys(load()), ['convertSync'])
})

test('the wasm addon matches the native build', { skip }, (t) => {
  const inputs = [
    ...fs
      .readdirSync(fixtures)
      .filter((n) => n.includes('.'))
      .map((n) => path.join(fixtures, n)),
    ...fs
      .readdirSync(hostile)
      .filter((n) => !n.startsWith('detect-'))
      .map((n) => path.join(hostile, n))
  ].sort()

  let binding = load()
  for (const file of inputs) {
    const label = path.basename(file)
    const input = fs.readFileSync(file)
    const from = path.extname(file).slice(1).toLowerCase()

    const expected = attempt(() => native.convertSync(input, from, 'pdf', converter.FONTS_DIR))
    const actual = attempt(() => binding.convertSync(input, from, 'pdf', '/fonts'))

    if (actual.err instanceof WebAssembly.RuntimeError) {
      t.fail(`${label} trapped: ${actual.err.message}`)
      binding = load()
    } else if (expected.err) {
      t.is(actual.err && actual.err.message, expected.err.message, `${label} fails the same way`)
    } else if (actual.err) {
      t.fail(`${label} threw ${actual.err.message}`)
    } else if (DIFFERS_ON_32_BIT.includes(label)) {
      t.is(actual.pdf.subarray(0, 5).toString(), '%PDF-', `${label} renders`)
    } else {
      t.ok(actual.pdf.equals(expected.pdf), `${label} is byte-identical`)
    }
  }
})

test('the wasm addon traps without a random source, and stays dead', { skip }, (t) => {
  const binding = load({})
  t.exception.all(() => binding.convertSync(markdown, 'md', 'pdf', '/fonts'), /unreachable/)
  t.exception.all(() => binding.convertSync(markdown, 'md', 'pdf', '/fonts'), /has crashed/)

  const pdf = Buffer.from(load().convertSync(markdown, 'md', 'pdf', '/fonts'))
  t.is(pdf.subarray(0, 5).toString(), '%PDF-', 'a fresh instance converts')
})

test('the wasm addon cannot see the host fonts directory', { skip }, (t) => {
  const binding = load()
  t.exception(() => binding.convertSync(markdown, 'md', 'pdf', converter.FONTS_DIR), /fonts/)

  const pdf = Buffer.from(binding.convertSync(markdown, 'md', 'pdf', '/fonts'))
  t.is(pdf.subarray(0, 5).toString(), '%PDF-', 'the instance survives the error')
})
