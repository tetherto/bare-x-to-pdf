include_guard(GLOBAL)

set(CORROSION_COMMIT "1499b14e4906a2890f5cee1547c8848db261753d" CACHE STRING "Corrosion commit (v0.6.1)")

bare_platform(_sys)
bare_arch(_arch)

if(_sys STREQUAL "darwin")
  if(_arch STREQUAL "arm64")
    set(_triple aarch64-apple-darwin)
  else()
    set(_triple x86_64-apple-darwin)
  endif()
elseif(_sys STREQUAL "linux")
  if(_arch STREQUAL "arm64")
    set(_triple aarch64-unknown-linux-gnu)
  else()
    set(_triple x86_64-unknown-linux-gnu)
  endif()
elseif(_sys STREQUAL "android")
  if(_arch STREQUAL "arm64")
    set(_triple aarch64-linux-android)
  elseif(_arch STREQUAL "arm")
    set(_triple armv7-linux-androideabi)
  elseif(_arch STREQUAL "ia32")
    set(_triple i686-linux-android)
  else()
    set(_triple x86_64-linux-android)
  endif()
elseif(_sys STREQUAL "ios")
  if(CMAKE_OSX_SYSROOT MATCHES "[Ss]imulator")
    if(_arch STREQUAL "arm64")
      set(_triple aarch64-apple-ios-sim)
    else()
      set(_triple x86_64-apple-ios)
    endif()
  else()
    set(_triple aarch64-apple-ios)
  endif()
elseif(_sys STREQUAL "win32")
  if(_arch STREQUAL "arm64")
    set(_triple aarch64-pc-windows-msvc)
  else()
    set(_triple x86_64-pc-windows-msvc)
  endif()
elseif(_sys STREQUAL "wasi")
  set(_triple wasm32-wasip1)
else()
  message(FATAL_ERROR "simple-converter: no Rust target for '${_sys}/${_arch}'")
endif()

set(Rust_CARGO_TARGET "${_triple}" CACHE STRING "Rust target triple")

fetch_package("github:corrosion-rs/corrosion#${CORROSION_COMMIT}")

corrosion_import_crate(
  MANIFEST_PATH "${CMAKE_CURRENT_SOURCE_DIR}/core/Cargo.toml"
  CRATE_TYPES staticlib
  PROFILE release
)

# Corrosion 0.6.1 adds the Command Line Tools SDK as a link directory on macOS.
# The linker searches it before CMAKE_OSX_SYSROOT, so -lSystem resolves against
# the wrong SDK and fails when the two differ (corrosion-rs/corrosion#686, fixed
# upstream after 0.6.1). Drop this once the pin moves past that fix.
if(APPLE)
  set_property(TARGET simple_converter_core-static PROPERTY INTERFACE_LINK_DIRECTORIES "")
endif()
