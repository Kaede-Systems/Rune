# Rune Target Support Matrix

This document separates implemented support from planned support.

## Native Targets

### Implemented

| Target | Status | Notes |
|---|---|---|
| `x86_64-pc-windows-gnu` | Implemented | Real executable output |
| `x86_64-pc-windows-msvc` | Implemented target entry | Toolchain path exists; practical runtime/link setup still depends on available assets |
| `aarch64-pc-windows-gnu` | Implemented | Real executable output path exists |
| `x86_64-unknown-linux-gnu` | Implemented | Real cross-build output verified |
| `aarch64-unknown-linux-gnu` | Implemented | Real target output path exists |
| `x86_64-apple-darwin` | Implemented target output | Real target output path exists |
| `aarch64-apple-darwin` | Implemented target output | Covers Apple Silicon family: M1, M2, M3, M4 |

### Notes

- Apple Silicon support maps to `aarch64-apple-darwin`
- Linux ARM64 support maps to `aarch64-unknown-linux-gnu`

## Embedded Targets

### Implemented

| Target | Status | Notes |
|---|---|---|
| `thumbv6m-none-eabi` | Implemented for freestanding object/static-lib output | Suitable for Cortex-M0/M0+ style targets |
| `thumbv7em-none-eabihf` | Implemented for freestanding object/static-lib output | Suitable for Cortex-M4/M7 style targets |
| `riscv32-unknown-elf` | Implemented for freestanding object/static-lib output | Covers the current packaged `riscv32` LLVM backend slice |
| `avr-atmega328p-arduino-uno` | Implemented for current Arduino Uno embedded slice | Produces `.hex` and sibling `.elf` through the packaged Arduino AVR core plus packaged `avr-gcc`/`avr-g++`/`objcopy`; when `llvm-cbe` is available the Uno path is `Rune -> LLVM IR -> llvm-cbe -> transient C -> AVR GCC/G++`; current scope includes top-level scripts or `main` / `setup()` / `loop()`, locals, control flow, serial output/input, UART helpers, concrete class/object methods, board constants, and pin/timing operations |

### Not Yet Implemented

| Target/Family | Status | Notes |
|---|---|---|
| Arduino Uno / AVR through direct LLVM AVR backend | Not implemented | The packaged LLVM toolchain in this repo does not currently ship an AVR backend, so Uno uses the packaged Arduino AVR core plus `llvm-cbe` + AVR GCC/G++ instead |
| Xtensa ESP32 | Not implemented | The packaged LLVM toolchain in this repo does not currently ship an Xtensa backend |

### Notes

- Embedded support currently means freestanding `--object` and `--static-lib` output for LLVM-backed embedded targets
- Arduino Uno is the current packaged-AVR exception: `rune build file.rn --target avr-atmega328p-arduino-uno` produces a real `.hex`; with packaged `llvm-cbe` available it compiles `Rune -> LLVM IR -> llvm-cbe -> transient C -> ArduinoCore-avr + AVR GCC/G++`, and `--flash --port COMx` flashes it through packaged `avrdude`
- The Arduino Uno surface now resolves `from arduino import ...` from the packaged stdlib root and supports parenthesized multiline imports
- Top-level/script-style Rune programs also work on Uno, so board examples can use normal top-level initialization plus `while true:` loops without requiring explicit `setup()` / `loop()`
- Raspberry Pi support stays under native Linux ARM64 where applicable, not under the freestanding embedded slice

## WASM Targets

### Implemented

| Target | Status | Notes |
|---|---|---|
| `wasm32-unknown-unknown` | Implemented for current LLVM-supported slice | Produces `.wasm` plus generated JS host loader |
| `wasm32-wasip1` | Implemented for current WASI slice | Produces direct WASI command module runnable with packaged Wasmtime |

### Planned

| Target | Status | Notes |
|---|---|---|
| Browser runtime for `wasm32-unknown-unknown` | Planned | Current verified host remains Node.js |

## Library Output

### Implemented

| Output Kind | Status | Notes |
|---|---|---|
| Shared library (`.dll`) | Implemented | Windows path exists |
| Shared library (`.so`) | Implemented | Linux cross-target path exists |
| Shared library (`.dylib`) | Implemented | macOS cross-target path exists |
| Static library (`.lib`) | Implemented | Packaged LLVM archiver path |
| Static library (`.a`) | Implemented | Packaged LLVM archiver path |

## Runtime/Host Status

### Native

| Capability | Status |
|---|---|
| Output | Implemented |
| Input | Implemented |
| Panic | Implemented |
| Time | Implemented |
| System | Implemented |
| Env | Implemented |
| Selected TCP probe functions | Implemented |

### Node-hosted WASM

| Capability | Status |
|---|---|
| Output | Implemented |
| Input | Implemented |
| Panic | Implemented |
| Time | Implemented |
| System | Implemented |
| Env | Implemented |
| TCP probe functions | Implemented |
| Filesystem | Implemented |
| Terminal control | Implemented |
| Bell/audio | Implemented |

### Wasmtime-hosted WASI

| Capability | Status |
|---|---|
| Output | Implemented |
| Input | Implemented |
| Panic | Implemented |
| Time | Implemented |
| System | Implemented |
| Env | Implemented |
| TCP probe functions | Implemented as safe `false` fallback in current WASI runtime |
| Filesystem | Implemented with preopened guest directory mapping |
| Terminal control | Implemented |
| Bell/audio | Implemented |

### Browser-hosted WASM

| Capability | Status |
|---|---|
| Output | Planned |
| Input | Planned |
| Panic | Planned |
| Time | Planned |
| HTTP/fetch | Planned |
| WebSocket | Planned |
| Raw TCP/UDP | Not applicable as direct browser capability |

### WASI

| Capability | Status |
|---|---|
| Direct WASI target/runtime | Implemented for current slice |
| I/O | Implemented |
| Time | Implemented |
| Env | Implemented |
| Networking | Runtime/capability-dependent |

## No-Zig Toolchain Status

### Implemented

| Area | Status |
|---|---|
| Packaged LLVM tools | Implemented |
| Packaged linker discovery | Implemented |
| Packaged static archive generation | Implemented |
| `wasm-ld` path | Implemented |

### In Progress

| Area | Status |
|---|---|
| Remove Zig from all executable/shared-library link paths | In progress |
| Package full target runtime/sysroot assets | In progress |

## Important Honesty Rules

- A target is only "implemented" for the scope that actually runs end to end today.
- Browser WASM and WASI must not be conflated with Node-hosted WASM.
- Apple Silicon support means `aarch64-apple-darwin`, not separate per-chip compiler backends.
- Linux ARM support means `aarch64-unknown-linux-gnu`, not a marketing label.
