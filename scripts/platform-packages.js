const fs = require('bare-fs')
const path = require('bare-path')

const root = path.join(__dirname, '..')
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'))

// The wasi target comes first: importing with `host: 'wasi-wasm32'` adds its
// conditions after the native ones, and the first key of `#binding` that
// matches wins. It loads anywhere, so its package has no `os` or `cpu`, and it
// isn't an optional dependency: without a host, a missing native prebuild would
// fall back to it, and it traps without a WASI provider. Apps that import with
// the host install it themselves.
const TARGETS = [
  {
    target: 'wasi-wasm32',
    platform: 'wasi',
    arch: 'wasm32',
    label: 'WebAssembly (WASI)',
    portable: true
  },
  {
    target: 'android-arm',
    platform: 'android',
    arch: 'arm',
    label: 'Android ARM 32-bit'
  },
  {
    target: 'android-arm64',
    platform: 'android',
    arch: 'arm64',
    label: 'Android ARM 64-bit'
  },
  {
    target: 'android-x64',
    platform: 'android',
    arch: 'x64',
    label: 'Android x86 64-bit'
  },
  {
    target: 'darwin-arm64',
    platform: 'darwin',
    arch: 'arm64',
    label: 'macOS ARM 64-bit'
  },
  {
    target: 'darwin-x64',
    platform: 'darwin',
    arch: 'x64',
    label: 'macOS x86 64-bit'
  },
  {
    target: 'ios-arm64',
    platform: 'ios',
    arch: 'arm64',
    label: 'iOS ARM 64-bit'
  },
  {
    target: 'ios-arm64-simulator',
    platform: 'ios',
    arch: 'arm64',
    simulator: true,
    label: 'iOS ARM simulator 64-bit'
  },
  {
    target: 'ios-x64-simulator',
    platform: 'ios',
    arch: 'x64',
    simulator: true,
    label: 'iOS x86 simulator 64-bit'
  },
  {
    target: 'linux-arm64',
    platform: 'linux',
    arch: 'arm64',
    label: 'Linux ARM 64-bit'
  },
  {
    target: 'linux-x64',
    platform: 'linux',
    arch: 'x64',
    label: 'Linux x86 64-bit'
  },
  {
    target: 'win32-x64',
    platform: 'win32',
    arch: 'x64',
    label: 'Windows x86 64-bit'
  }
]

const imports = {}
const optional = {}

for (const t of TARGETS) {
  const name = `${pkg.name}-${t.target}`
  const dir = path.join(root, 'npm', t.target)
  fs.mkdirSync(dir, { recursive: true })

  const manifest = {
    name,
    version: pkg.version,
    description: `The ${t.label} prebuild of ${pkg.name}`,
    exports: {
      './package': './package.json',
      '.': './index.js'
    },
    files: ['index.js', 'prebuilds'],
    addon: true,
    ...(t.portable
      ? {
          peerDependencies: { [pkg.name]: pkg.version },
          peerDependenciesMeta: { [pkg.name]: { optional: true } }
        }
      : { os: [t.platform], cpu: [t.arch] }),
    license: pkg.license,
    repository: pkg.repository,
    bugs: pkg.bugs,
    homepage: pkg.homepage,
    publishConfig: pkg.publishConfig
  }
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest, null, 2) + '\n')

  fs.writeFileSync(path.join(dir, 'index.js'), ['exports.binding = require.addon()', ''].join('\n'))

  const byPlatform = (imports[t.platform] ||= {})
  if (t.platform === 'ios') {
    const byArch = (byPlatform[t.arch] ||= {})
    byArch[t.simulator ? 'simulator' : 'default'] = name
  } else {
    byPlatform[t.arch] = name
  }
  if (!t.portable) optional[name] = pkg.version
}

pkg.imports = { '#binding': imports }
pkg.optionalDependencies = optional
fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify(pkg, null, 2) + '\n')

console.log(`wrote ${TARGETS.length} platform packages at version ${pkg.version}`)
