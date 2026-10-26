cmake_minimum_required(VERSION 4.0)

find_package(cmake-bare REQUIRED PATHS node_modules/cmake-bare)
find_package(cmake-fetch REQUIRED PATHS node_modules/cmake-fetch)

project(simple_converter C)

include(cmake/rust.cmake)

bare_target(target)

if(target STREQUAL "wasi-wasm32")
  add_bare_module(simple_converter)
else()
  add_bare_module(simple_converter NAME bare-x-to-pdf-${target})
endif()

target_sources(
  ${simple_converter}
  PRIVATE
    binding.c
)

target_include_directories(
  ${simple_converter}
  PRIVATE
    core/include
)

target_link_libraries(
  ${simple_converter}
  PRIVATE
    simple_converter_core
)

# Bound the linear memory a hostile document can claim, so it traps instead of
# growing to the 4 GiB limit of wasm32, and give the recursive readers and
# Typst the 8 MiB stack they get on native threads rather than the 64 KiB
# default of wasm-ld. The default cap leaves room for the 256 MB images and
# 128 MB parts that core/src/limits.rs lets through.
set(SIMPLE_CONVERTER_WASM_MAX_MEMORY 1073741824 CACHE STRING "Linear memory limit of the WebAssembly addon in bytes, a multiple of 65536")

if(target MATCHES "^wasi-")
  target_link_options(
    ${simple_converter}_module
    PRIVATE
      -Wl,--max-memory=${SIMPLE_CONVERTER_WASM_MAX_MEMORY}
      -Wl,-z,stack-size=8388608
  )
endif()

if(CMAKE_STRIP AND NOT WIN32)
  add_custom_command(
    TARGET ${simple_converter}_module
    POST_BUILD
    COMMAND ${CMAKE_STRIP} $<IF:$<PLATFORM_ID:Darwin,iOS>,-x,$<IF:$<PLATFORM_ID:WASI>,--strip-all,--strip-unneeded>> $<TARGET_FILE:${simple_converter}_module>
  )
endif()
