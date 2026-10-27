# bare-x-to-pdf

Convert office documents to PDF for [Bare](https://github.com/holepunchto/bare).
A native addon with a Rust engine that lays pages out with
[Typst](https://typst.app).

`x` is every format it reads into a PDF today:

| Family                    | Extensions                         |
| ------------------------- | ---------------------------------- |
| Word                      | `.docx`, `.docm`, `.dotx`, `.dotm` |
| Word 97–2003              | `.doc`, `.dot`                     |
| Excel                     | `.xlsx`, `.xlsm`, `.xltx`, `.xltm` |
| Excel 97–2003             | `.xls`, `.xlt`                     |
| PowerPoint                | `.pptx`                            |
| OpenDocument Text         | `.odt`, `.ott`, `.fodt`            |
| OpenDocument Spreadsheet  | `.ods`, `.ots`, `.fods`            |
| OpenDocument Presentation | `.odp`, `.otp`, `.fodp`            |
| Markdown                  | `.md`, `.markdown`                 |

The `from` name for each is the extension without its dot, and PDF is the only output.
Markdown is CommonMark plus GitHub tables, task lists and strikethrough. Legacy
PowerPoint (`.ppt`), `.pptm`, `.potx`, `.ppsx`, Rich Text and HTML are not read.

## Install

```sh
npm install bare-x-to-pdf
```

The native prebuild is an optional dependency per platform (`bare-x-to-pdf-<platform>-<arch>`),
so an install downloads only the package for its own platform, about 17 MB packed and
42 MB unpacked, plus the main package with the fonts and sources, about 4 MB packed.
Prebuilds exist for android (arm, arm64, x64), darwin (arm64, x64), ios (arm64 and the
arm64 and x64 simulators), linux (arm64, x64) and win32 (x64); other hosts build from a
checkout with the Bare toolchain, CMake 4 and Rust.

To bundle for a different platform, ask npm for that prebuild as well:

```sh
npm install --os=android --cpu=arm64
npm install --os=ios --cpu=arm64
```

### WebAssembly

On Bare 1.36 and later the converter can also run as a WebAssembly addon, sandboxed through
WASI. The `.wasm` prebuild ships in the main package, so there is nothing extra to install.
Import with the `wasi-wasm32` host and a WASI provider. The attributes must be written
inline in the `require` call. The addon needs a random source and read access to the fonts:

```js
const converter = require('bare-x-to-pdf', {
  with: { host: 'wasi-wasm32', wasi: './wasi.js' }
})
```

```js
// wasi.js
const crypto = require('bare-crypto')
const { DirectorySync } = require('bare-directory')
const { HostDirectory } = require('bare-wasi/host')

const fonts = require.resolve('bare-x-to-pdf/package').replace(/package\.json$/, 'fonts')

module.exports = (url) => ({
  version: 'preview1',
  random: (b) => crypto.randomFillSync(b),
  preopens: { [fonts]: new HostDirectory(DirectorySync.open(fonts)) }
})
```

The WebAssembly addon has only `convertSync`. `convert()` falls back to it, so it blocks the
calling thread; run conversions in a [`bare-thread`](https://github.com/holepunchto/bare-thread).
A hostile document can crash the addon with a `RuntimeError` without taking down the process,
but the addon stays unusable in that thread afterwards, so replace the thread.

## Usage

```js
const converter = require('bare-x-to-pdf')

const pdf = await converter.convert(docxBytes, { to: 'pdf' })
```

Input is a `Uint8Array` or `ArrayBuffer` and the result is a `Buffer`. `convert()` runs on
the libuv thread pool and does not block the JavaScript thread; `convertSync()` runs on the
calling thread.

## API

### `const pdf = await convert(input, options)`

Convert `input` off the JavaScript thread and resolve with a `Buffer`. The input is copied,
so it may be changed or transferred once the call returns. Invalid options and conversion
errors reject the promise.

- `options.to` — target format, `'pdf'`. Required.
- `options.from` — source format; sniffed from the bytes when omitted. Markdown has no
  signature to sniff, so pass `from: 'md'` for it. Markdown images load only from `data:`
  URIs, since the input is bytes with no directory to resolve relative paths against.
- `options.fontsDir` — a directory of fonts to use instead of the bundled set.

### `const pdf = convertSync(input, options)`

Convert `input` on the calling thread and return a `Buffer`. Takes the same options as
`convert()`.

### `detect(input)`

Sniff the format from the bytes and return `'docx'`, `'pptx'`, `'xlsx'`, `'doc'`, `'xls'`,
`'odt'`, `'ods'`, `'odp'`, `'fodt'`, `'fods'`, `'fodp'` or `null`. Templates and
macro-enabled files report their base format (a `.dotx` is `'docx'`, an `.ots` is
`'ods'`), which converts the same way. Markdown is plain text and always returns `null`.

### `supports(from, to)`

Return whether the `from` → `to` conversion is supported.

### `conversions()`

Return every supported pair as `{ from, to }`.

### `FONTS_DIR`

Path to the bundled fonts directory, resolved with `require.asset`.

## Limits

Documents are often untrusted, so the engine caps what a small file can make it allocate or
lay out. A document past a hard limit is rejected with an error. One past a soft limit still
converts, with the value clamped.

Rejected:

- An archive part larger than 128 MB uncompressed.
- XML nested more than 512 elements deep.
- A spreadsheet that would print more than 30,000 cells.

Clamped:

- Table spans: 1–1,000 columns.
- Text columns: 1–64.
- Pages: 72–14,400 pt on each side, with margins scaled to fit.
- Lengths: NaN and infinity become 0; any other length is held within ±1,000,000 pt.
- List counters: above 32,767, letter and Roman labels fall back to decimal.
- Charts: up to 100,000 points per series (in .xls, 32,000 per series and 256,000 per chart).
- Formulas: nesting depth up to 256.
- Images: more than 50 megapixels, or ones that cannot be decoded, are drawn as blank
  space.
- Tables nested more than 12 deep: the inner tables are flattened into paragraphs.

Spreadsheet pages with no content are not printed, as in LibreOffice Calc.

## Development

The platform packages in `npm/` are npm workspaces, and each declares its own `os` and `cpu`
so that installs download only one. In a checkout, install with `npm install --force` so npm
links all of them, then build the host prebuild with `npm run prebuild` and run `npm test`.
The fidelity benchmark in `bench/` has its own dependencies: `cd bench && npm install`.

## Releases

`.github/workflows/publish.yml` runs when a `v*` tag is pushed. Tag the commit whose
`package.json` has that version, after running `npm run packages` to bring the platform
packages to it. The workflow builds every platform, publishes the platform packages and then
the main package through npm trusted publishing, and creates a GitHub release. A tag newer
than npm's current `latest` publishes as `latest`; an older one as `release-<major>.<minor>`.

## License

Apache-2.0. Typst, pulldown-cmark and the other Rust dependencies are Apache-2.0 or MIT.
Liberation, Carlito and Caladea are under the SIL Open Font License (see `fonts/`);
OpenSymbol comes from LibreOffice.
