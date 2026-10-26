# WebAssembly target: findings

Working notes on building bare-x-to-pdf as a `wasi-wasm32` addon for Bare, to decide
whether the converter can run sandboxed in Keet. First measured 2026-10-12 on linux x64
with Bare 1.32 and a simulated 1.35 `js.h`, then rebuilt and re-run on 2026-10-13 with
Bare 1.35.0 and its stock headers, with identical results. Re-checked on 2026-10-23 with
Bare 1.36.0, which ships wasm addon loading (bare-module 7.5, holepunchto/bare-module#104).

## Short answer

Yes. The Rust core builds for `wasm32-wasip1` with no source changes. With this branch,
`bare-make --platform wasi --arch wasm32` builds the real addon (`binding.c` + core)
against the Bare 1.35 headers as published. Loaded through `bare-wasm-abi` on Bare 1.35:

- All 135 inputs that convert natively produce **byte-identical PDFs** (91 corpus files +
  44 hostile fixtures). All 10 hostile rejections give the same error. The 7 remaining
  differences are explained under "32-bit".
- About **1.85× native time** (median 1.86×, p90 3×) on desktop V8 with its JIT. Don't
  expect that to hold on an interpreter tier. The stripped `.wasm` is 25.8 MB, against
  30.2 MB for the linux-x64 `.bare`, and one file serves every platform.
- A 225 KB document that **OOM-kills the native process** (and so Keet) becomes a
  catchable `RuntimeError` (0.4 s under a 256 MiB cap, 4.9 s under the default 1 GiB).
  The host survives; the addon instance is dead after that (see "Recovering from a trap").

On Bare 1.36, `require('bare-x-to-pdf', { with: { host: 'wasi-wasm32', wasi } })` works end
to end, with the `.wasm` shipped inside the main package. It needs two package changes
(see "Loading through `require`"). Still open before shipping in Keet: does WebAssembly
exist on iOS and in lite-mode V8 at all (see "Mobile")?

## How Bare's wasm addons work

- Target `wasm32-wasip1`, toolchain `wasi-wasm32` from `cmake-toolchains` 2.1, built by
  `cmake-bare` ≥ 1.10 as a WASI reactor exporting `bare_register_module_v0`, `malloc`,
  `free`, memory and the function table. `bare-make` ≥ 2.1 brings the LLVM toolchain
  (`llvm-runtime` 23) and the sysroot comes from `wasi-sysroot`.
- `bare-wasm-abi` implements the libjs ABI in JavaScript on top of the engine's own
  `WebAssembly` (via `bare-wasm`). The addon gets nothing but JS values and the WASI
  capabilities it is handed through `bare-wasi`. A trap poisons only that instance.
- Prebuilds resolve from `prebuilds/wasi-wasm32/<name>.wasm`. Bare 1.36 loads them
  (bare-module 7.5, which depends on `bare-wasm-abi` directly). The importer chooses the
  host and a WASI provider, a module exporting `(url) => bareWasiOptions`:
  `require('bare-x-to-pdf', { with: { host: 'wasi-wasm32', wasi: './provider.js' } })`.
  Without a provider the addon gets no capabilities.
- The attributes must be literals. The static traversal reads them from the `require`
  call, so attributes held in a variable get the package resolved for the native host
  first, and the real import then fails with `ADDON_HOST_INCOMPATIBLE`.
- The host applies to the package **and all its dependencies**. Every addon in the tree
  needs a `wasi-wasm32` prebuild, or the import fails with `ADDON_NOT_FOUND`.
- A package's addons load in one context per loader: the host and provider it's first
  imported under. Importing it under another in the same loader throws.
- Excluded by design: event loop (`js_get_env_loop`, so no `uv_queue_work`), threads and
  threadsafe functions, shared array buffers. `js_add_deferred_teardown_callback` has
  marshalling but no implementation, so it is rejected too.

## Changes on this branch

- `cmake/rust.cmake`: map platform `wasi` to `wasm32-wasip1`.
- `binding.c`:
  - Include `<bare/module.h>` instead of `<bare.h>`, and `<uv.h>` only off-wasm. Bare's
    `bare.h` (still, at HEAD) and the 1.33 `js.h` pull in libuv's `uv/unix.h`, which
    needs `netdb.h`, and wasi-libc has none. libjs dropped that include from `js.h`
    (`8a8edc7`, in the 1.35 pin). `bare.h` still has it, so this change stays necessary.
  - Compile the async `convert()` (libuv thread pool) out under `__wasm__`. The wasm
    addon exports only `convertSync`, and everything that path uses is in the ABI.
- `CMakeLists.txt`, wasm only:
  - `--max-memory` caps linear memory. The default is 1 GiB, set by the
    `SIMPLE_CONVERTER_WASM_MAX_MEMORY` cache variable (see "Isolation" for choosing it).
  - `--strip-all` instead of `--strip-unneeded`, which keeps the 4.8 MB name section.
  - `-z stack-size=8 MiB`: wasm-ld's default is 64 KiB, which made
    `cf-nested-parens.xlsx` and `nested-tables-50.docx` trap.
  - The stack sits below the data (initial SP = stack size), so overflow traps rather
    than silently corrupting the heap.
- `index.js`: `convert()` falls back to `convertSync` when the binding has no `convert`.
- `package.json`: `cmake-bare` ^1.10.1. Like holepunchto/bare-jpeg#20, `bare-make` and
  `wasi-sysroot` are global tools rather than devDeps; in CI, `compile-prebuilds` installs
  them with `wasi: true`.
- `.github/workflows/prebuild.yml`: a `wasi-wasm32` job (Rust target `wasm32-wasip1`,
  `wasi: true` on `compile-prebuilds`). On publish, `publish.yml` moves its artifact from
  `npm/wasi-wasm32/prebuilds` into the main package's `prebuilds`.

- Packaging, as described under "Loading through `require`": the main package ships
  `prebuilds/wasi-wasm32/bare-x-to-pdf.wasm` and `wasi.js`, and
  `scripts/platform-packages.js` writes the `wasi` key first in `#binding`, pointing at
  `./wasi.js`. The wasi build keeps the package name (`CMakeLists.txt` drops the
  `-${target}` suffix for it), because `require.addon()` in the main package looks for
  `bare-x-to-pdf.wasm`. Native prebuilds stay in their platform packages, about 30 MB
  each. `publish.yml` publishes every `npm/*` package instead of walking
  `optionalDependencies`. `index.js` no longer imports
  `bare-path`, which moved to `devDependencies`, and the unused `bare-url` dependency is
  gone, so the package has no runtime dependencies.

Still to do:

- A `require`-based test with the `host` attribute in `test/`. `test/wasm.js` loads the
  built `.wasm` through `bare-wasm-abi` directly.

## Loading through `require`

Tested end to end on Bare 1.36.0 in a scratch app with local copies of `bare-x-to-pdf`,
`bare-x-to-pdf-linux-x64` and a hand-made `bare-x-to-pdf-wasi-wasm32` package (the
installed `.wasm` at `prebuilds/wasi-wasm32/bare-x-to-pdf-wasi-wasm32.wasm`, with the same
`index.js` as the other platform packages). On 1.35 the `host` attribute only changed how
`require.addon()` looked up prebuilds. On 1.36 it also adds the host's conditions (`wasi`,
`wasm32`) to the import map, **after** the native ones. Each `#binding` key is checked in
the order the package lists them, so the order of keys decides (all with `bare-path`
removed, see 2. below):

| `#binding` map           | `require` with `host: 'wasi-wasm32'`                       | plain `require` |
| ------------------------ | ---------------------------------------------------------- | --------------- |
| as today (no `wasi` key) | picks `linux-x64`, which has no `.wasm`: `ADDON_NOT_FOUND` | native          |
| `wasi` key last          | same: `linux` matches first                                | native          |
| `wasi` key **first**     | picks `bare-x-to-pdf-wasi-wasm32`, converts                | native          |

With the `wasi` key first, the wasm PDFs were byte-identical to native on the fixtures
tried (3 corpus files, 6 hostile ones), and the provider was called once with the addon's
URL. Two changes were needed for that, and both are made on this branch:

1. **`#binding` gets `"wasi": { "wasm32": "./wasi.js" }` as its first key**
   (`scripts/platform-packages.js` writes it). Under the wasi host, the
   `require.addon()` in `wasi.js` loads the main package's own
   `prebuilds/wasi-wasm32/bare-x-to-pdf.wasm`. Prettier and other tools that sort keys
   must leave this order alone. This was first tested with a separate
   `bare-x-to-pdf-wasi-wasm32` package, and again on 2026-10-26 with the 0.5.0 `.wasm`
   inside the main package: same PDF as native, provider called with the addon's URL.
2. **`index.js` drops `bare-path`.** It's only used for `path.join(__dirname, 'fonts')`,
   but `bare-path` has a native addon (for `cwd()`) with no wasm prebuild, so under the
   wasm host the import fails in `bare-path/binding.js`. Joining with a platform separator
   by hand fixed it; `index.js` now uses `require.asset('./fonts')`, which needs no import. `bare-url` was also listed as a dependency but unused, and is gone
   too. bare-sqlite, the release
   example, has no dependencies at all.

**Native fallback.** With no host, bare-module tries the native conditions first and then
`wasi`/`wasm32` as a fallback. The `.wasm` is always installed, so if the native package
is missing (a platform with no prebuild, such as win32-arm64 or android-ia32, or a failed
optional install), a plain `require('bare-x-to-pdf')` loads the wasm addon with no
provider, and the first conversion traps (`RuntimeError: unreachable`, from the denied
`random_get`) instead of the import failing with `ADDON_NOT_FOUND`. Tested on Bare 1.36
by removing `bare-x-to-pdf-darwin-arm64`. `wasi.js` can't tell the two cases apart:
`require.addon.host` is the native host either way. bare-sqlite, which ships all its
prebuilds in one package, behaves the same.

The cost of shipping it in the main package is size: every install gets the 25.8 MB
`.wasm` (34 MB unpacked and 13 MB packed in total, against 8.4 MB unpacked before).

## Capabilities the addon needs from Keet's WASI provider

- **Randomness, required.** Rust's `HashMap` seeds from `random_get` on first use, and
  `bare-wasi` denies it by default, so every conversion traps without it. Pass
  `random: (b) => crypto.getRandomValues(b)`. Don't use a fixed seed: predictable hashing
  invites hash-flooding by crafted documents.
- **Fonts.** The core reads `fontsDir` with `std::fs`, and `FONTS_DIR` is a host path the
  guest can't see. A `MemoryDirectory` preopen at `/fonts` holding the 7.4 MB of bundled
  fonts works, with `fontsDir: '/fonts'`. Simpler, with Bare 1.36's `bare-wasi/host`: preopen
  the host fonts directory read from disk **at its own host path**, and the default
  `FONTS_DIR` works with no option and no `index.js` change (tested, byte-identical):

  ```js
  const fonts = require.resolve('bare-x-to-pdf/package').replace(/package\.json$/, 'fonts')
  preopens: { [fonts]: new HostDirectory(DirectorySync.open(fonts)) }
  ```

  Untested on Windows, where the key would be a drive-letter path. Alternatives that need
  no filesystem at all: embed the fonts in the `.wasm`, or pass font bytes through the
  API. Without a fonts preopen, conversion fails with a clean `Error` ("fonts directory …:
  No such file or directory"), not a trap.

- Clocks are on by default and harmless. No stdio, no args, no env needed.

## Isolation

A 225 KB `.docx` with 400,000 short paragraphs (`many-paras.docx`, not yet in
`test/fixtures/hostile`):

| build             | result                                                              |
| ----------------- | ------------------------------------------------------------------- |
| native 0.4.0      | grew past 10 GB in 34 s, then the kernel OOM-killed the process     |
| wasm, no cap      | grew to the 4 GiB wasm32 ceiling in 29 s, `RuntimeError`, host fine |
| wasm, 1 GiB cap   | `RuntimeError` in 4.9 s, host fine, next conversion fine            |
| wasm, 256 MiB cap | `RuntimeError` in 0.3–0.5 s, host fine, next conversion fine        |

A 1 GiB inflate bomb gets "a part of the document is larger than 128 MB uncompressed"
natively and under a 1 GiB cap. Under 256 MiB it gets "io: out of memory". All three are
clean errors.

**Choosing the cap is a team decision.** The corpus peaks at 18–82 MB of linear memory,
but its images are small. `core/src/limits.rs` lets through 256 MB images and 128 MB
parts, so a 256 MiB cap would reject real photo-heavy decks that native converts. The
default of 1 GiB honors those limits and still bounds the damage. For chat previews,
something lower, with the core limits lowered to match, is reasonable.

### Recovering from a trap

`bare-wasm-abi` poisons an instance after a trap by design: every later call throws
"WebAssembly addon has crashed". Under `require` the instance belongs to the loader, which
caches it, so userland can't re-instantiate it. Wasm memory also never shrinks, so a
document that grew it to the cap keeps that memory. In Keet, convert in a thread (its own
loader and addon instance) and replace the thread after any `RuntimeError`, or use one
thread per conversion. Instantiation takes about 5 ms.

Tested on Bare 1.36 with `bare-thread` 1.4. Since 1.36 a `Bare.Thread` can't read modules
from disk, and `bare-thread` bundles the entry's module graph, including `.wasm` addons.
A worker importing with the `host`/`wasi` attributes converted `many-paras.docx`: a
`RuntimeError` after 4.7 s under the 1 GiB cap. A second call in the same thread got
"WebAssembly addon has crashed". The main thread carried on, and a fresh thread converted
a normal document.

What wasm does **not** bound is CPU time. Wasm addons are single-threaded with no event
loop, so `convertSync` blocks whichever thread calls it. In Keet, run it in a
`Bare.Thread` and terminate the thread to enforce a deadline.

Panics: stable `wasm32-wasip1` is `panic=abort`, so the `catch_unwind` in `sc_convert` is
inert. A panic becomes a trap (`unreachable`) instead of the "internal error" message,
and kills the instance like any other trap.

Separately, the core needs a content-volume limit (paragraphs or pages), since
`many-paras.docx` takes down every native build today.

## 32-bit

Seven hostile fixtures render differently (`text-space-count-*`, `gridspan-huge/max.docx`,
`colspan-huge.odt`). Each carries the value 2^44. `parse::<usize>()` fails on 32-bit, so
the fallback (`1`) applies where 64-bit clamps to 1000. Harmless, and the armv7 Android
prebuild already behaves this way. But the core has ~200 `as usize`/`as u32` casts that
have only been exercised on 64-bit, so the hostile suite should run on a 32-bit target
too. Parsing these attributes as `u64` before clamping would make the targets agree.

## Mobile

`bare-wasm` uses the engine's global `WebAssembly`. V8 has no WebAssembly under
`--jitless` unless built with its DrumBrake interpreter. libjs passes `--lite-mode`
(which implies jitless) when `optimize_for_memory` is set, which BareKit exposes and
keet-mobile-core's push extension sets. Bare's own docs say iOS runs V8 jitless, and
nothing in holepunchto mentions DrumBrake. There is also `holepunchto/libjsc`, a
JavaScriptCore backend. The question for the Bare team has two parts:

1. Which engine backs Bare on iOS, and is `typeof WebAssembly` defined there?
2. Same question for the lite-mode push extension.

If the answer is no, this path is desktop and Android only. If there is a wasm
interpreter, expect it to be much slower than the 1.85× measured here.

## Reproducing

```sh
npm i -g bare-make wasi-sysroot   # bare-make >= 2.1
rustup target add wasm32-wasip1
npm i --force                     # workspaces include other-cpu packages
bare-make generate --build build-wasm --platform wasi --arch wasm32
bare-make build --build build-wasm
# -> build-wasm/bare-x-to-pdf-wasi-wasm32@0.wasm
```

Load it through `require` on Bare ≥ 1.36 (see "Loading through `require`" for the package
layout), with a provider such as:

```js
// provider.js
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

Or load the built file directly with `bare-wasm-abi`, as `test/wasm.js` does:

```js
const instantiate = require('bare-wasm-abi')
const { MemoryDirectory } = require('bare-wasi/memory')
const binding = instantiate(bytes, {
  wasi: {
    version: 'preview1',
    random: (b) => crypto.getRandomValues(b),
    preopens: { '/fonts': new MemoryDirectory(fontFiles) }
  }
})
binding.convertSync(input, 'docx', 'pdf', '/fonts')
```
