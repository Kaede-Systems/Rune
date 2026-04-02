# Rune

![Rune logo](assets/branding/rune-logo-lockup.png)

Rune is a native compiled programming language with Python-inspired syntax, a real Rust compiler, an internal IR, an LLVM-backed target path, a native backend, packaged tooling, and a growing standard library.

This repository is the actual compiler and toolchain workspace. It follows the repository rules in [AGENTS.md](AGENTS.md): no scaffolds, no placeholder features, and no claiming support beyond what is implemented end to end.

## Current Status

Rune today has working vertical slices for:

- indentation-sensitive `.rn` source files
- lexer, parser, semantic checker, warnings, diagnostics, optimizer, and IR
- native executable builds on the current Windows path
- LLVM IR emission, LLVM assembly emission, and LLVM-backed builds for supported targets
- dynamic values in the native backend and the LLVM backend
- imports and stdlib modules
- C FFI in both directions for the currently supported ABI slice
- packaged tooling for LLVM, LLD, Wasmtime, installers, and branding assets
- vendored `llvm-cbe` source plus packaged host `llvm-cbe` binaries in release bundles

Rune is ambitious, but the repository is kept honest. The language vision is larger than the current fully-complete release surface, and the docs call that out explicitly.

## Language Direction

Rune is aiming at:

- Python-like readability
- native compilation
- optional dynamic typing
- stronger static typing where declared
- low-level systems access
- OOP with working concrete classes/methods today, and richer polymorphism over time
- cross-target native builds
- WASM and WASI support
- freestanding embedded object/static-library output for supported LLVM targets
- packaged Arduino Uno AVR builds for the current embedded slice using the packaged Arduino AVR core and AVR GCC/G++

## What Works Now

### Frontend and Core Compiler

- `rune lex`
- `rune parse`
- `rune check`
- `rune emit-ir`
- `rune emit-asm` (LLVM-backed assembly emission)
- `rune emit-llvm-ir`
- `rune emit-avr-precode` for real Uno pre-ELF code inspection
- `rune debug`

### Backends

- handwritten native backend for the currently supported Windows-native slice
- LLVM-backed object generation
- LLVM-backed assembly generation
- LLVM-backed dynamic-value support
- WASM module output
- WASI module output with packaged Wasmtime execution

### Standard Library Surface

Current stdlib modules live under [stdlib](stdlib):

- `env`
- `fs`
- `io`
- `network`
- `serial`
- `system`
- `terminal`
- `time`
- `audio`
- `arduino`

Current class-style stdlib wrappers include:

- `serial.SerialPort`
- `network.TcpClient`
- `network.UdpEndpoint`

### FFI

- Rune calling C through `extern def`
- C calling Rune through generated shared/static libraries
- automatic C header generation for supported library exports
- automatic C-source compilation during Rune builds through `--link-c-source`

## Quick Start

Build the compiler:

```powershell
cargo build
```

Check a Rune file:

```powershell
cargo run -- check calculator.rn
```

Build a native executable:

```powershell
cargo run -- build calculator.rn -o calculator.exe
```

Emit Rune IR:

```powershell
cargo run -- emit-ir calculator.rn
```

Emit LLVM IR:

```powershell
cargo run -- emit-llvm-ir calculator.rn
```

Emit target assembly:

```powershell
cargo run -- emit-asm calculator.rn --target x86_64-unknown-linux-gnu
```

Inspect the real Arduino Uno pre-ELF generated code:

```powershell
cargo run -- emit-avr-precode buzzer_serial_control_arduino.rn
```

Build WASI and run it through packaged Wasmtime:

```powershell
cargo run -- build calculator.rn --target wasm32-wasip1 -o calculator_wasi.wasm
cargo run -- run-wasm calculator_wasi.wasm --host wasmtime
```

Build Arduino Uno firmware (`.hex`) through the packaged Arduino AVR core and AVR toolchain:

```powershell
cargo run -- build hello_arduino.rn --target avr-atmega328p-arduino-uno -o hello_arduino.hex
```

Build and flash to a serial port:

```powershell
cargo run -- build hello_arduino.rn --target avr-atmega328p-arduino-uno --flash --port COM5 -o hello_arduino.hex
```

Build and flash the serial calculator example:

```powershell
cargo run -- build serial_calculator_arduino.rn --target avr-atmega328p-arduino-uno --flash --port COM5 -o serial_calculator_arduino.hex
```

Build and flash the current Arduino quiz/buzzer examples:

```powershell
cargo run -- build serial_math_quiz_arduino.rn --target avr-atmega328p-arduino-uno --flash --port COM5 -o serial_math_quiz_arduino.hex
cargo run -- build buzzer_serial_control_arduino.rn --target avr-atmega328p-arduino-uno --flash --port COM5 -o buzzer_serial_control_arduino.hex
```

Verify the AVR OOP/string runtime example on real hardware:

```powershell
cargo run -- build avr_oop_string_test.rn --target avr-atmega328p-arduino-uno --flash --port COM5 -o avr_oop_string_test.hex
```

## Main Commands

```text
rune lex file.rn
rune parse file.rn
rune check file.rn
rune emit-ir file.rn
rune emit-asm file.rn
rune emit-llvm-ir file.rn
rune emit-avr-precode file.rn
rune emit-c-header file.rn -o file.h
rune build file.rn
rune build file.rn --object --target thumbv6m-none-eabi -o firmware.o
rune build file.rn --target <triple> -o output
rune build file.rn --lib -o library
rune build file.rn --static-lib -o library
rune build file.rn --link-c-source helper.c -o app
rune run-wasm module.wasm --host node|wasmtime
rune debug file.rn
rune decompile binary [--target <triple>]
rune targets
rune toolchain
```

## Toolchain

Rune now uses packaged LLVM/LLD tooling. Zig is not part of the live build path anymore.

For embedded targets, Rune currently supports freestanding object/static-library output on the packaged LLVM targets that really exist in this repo today:

- `thumbv6m-none-eabi`
- `thumbv7em-none-eabihf`
- `riscv32-unknown-elf`

Arduino Uno is implemented through the packaged Arduino AVR core plus AVR GCC/G++/objcopy/avrdude toolchain path. Xtensa ESP32 is not claimed as implemented yet.

When the packaged LLVM C backend is available, the Uno path now goes through:

- Rune -> LLVM IR
- `llvm-cbe`
- transient C
- packaged AVR GCC/G++
- `.elf`
- `.hex`

Rune release bundles and installers now treat `llvm-cbe` as part of the packaged toolchain:

- the `llvm-cbe` source tree is vendored in this repository under `tools/llvm-cbe`
- CI builds a host `llvm-cbe` binary for each release bundle
- installers verify that `llvm-cbe` exists and can build it locally against the packaged LLVM bundle if needed

## Examples

Current repo examples include:

- [hello_arduino.rn](hello_arduino.rn): minimal Arduino Uno serial hello-world
- [serial_math_quiz_arduino.rn](serial_math_quiz_arduino.rn): serial-driven positive math quiz with LED feedback
- [serial_connector_arduino.rn](serial_connector_arduino.rn): host-side serial connector for the Uno quiz
- [buzzer_arduino.rn](buzzer_arduino.rn): basic buzzer example for pins `8` and `7`
- [buzzer_serial_control_arduino.rn](buzzer_serial_control_arduino.rn): interactive buzzer control over serial
- [ultrasonic_distance_arduino.rn](ultrasonic_distance_arduino.rn): HC-SR04 distance reader example
- [avr_oop_string_test.rn](avr_oop_string_test.rn): AVR class/method/string runtime smoke test
- [calculator.rn](calculator.rn): native desktop calculator example
- [wasm_demo.rn](wasm_demo.rn): simple WASM-oriented example

The current toolchain state is documented in:

- [Docs/NO_ZIG_TOOLCHAIN_PLAN.md](Docs/NO_ZIG_TOOLCHAIN_PLAN.md)
- [Docs/TARGET_SUPPORT_MATRIX.md](Docs/TARGET_SUPPORT_MATRIX.md)
- [Docs/WASM_PLAN.md](Docs/WASM_PLAN.md)
- [Docs/RUNTIME_ABI.md](Docs/RUNTIME_ABI.md)

## Documentation

Start with:

- [Docs/README.md](Docs/README.md)
- [Docs/SYNTAX.md](Docs/SYNTAX.md)
- [Docs/GRAMMAR.md](Docs/GRAMMAR.md)
- [Docs/STDLIBS.md](Docs/STDLIBS.md)

Planning docs:

- [RUNE_1_0_PLAN.md](RUNE_1_0_PLAN.md)
- [TECH_STACK_AND_PLAN.md](TECH_STACK_AND_PLAN.md)

## Repository Layout

- [src](src): compiler, backends, toolchain, runtime build integration
- [tests](tests): parser, semantic, IR, backend, runtime, CLI, and toolchain tests
- [stdlib](stdlib): Rune standard-library modules that are implemented today
- [Docs](Docs): syntax, grammar, ABI, WASM, targets, and toolchain docs
- [installers](installers): Windows and Unix installer scripts
- [assets/branding](assets/branding): Rune icon and packaging assets
- [tools](tools): packaged toolchain assets used by Rune

## Installation

Installer scripts:

- [installers/install-windows.ps1](installers/install-windows.ps1)
- [installers/install-unix.sh](installers/install-unix.sh)
- [installers/README.md](installers/README.md)

The installers are release-oriented now: if you do not pass a local binary path, they download the matching Rune release bundle for the current OS/arch from GitHub Releases and install it.

Version commands:

```text
rune version
rune --version
```

## Release Automation

The repository now includes [`.github/workflows/release.yml`](.github/workflows/release.yml).

Branch flow:

- `main` is the active development branch
- `release` is the release-candidate and publishing branch

When `main` is ready, it should be merged into `release`.

On pushes to the `release` branch, CI now:

- builds host-native Rune bundles for the configured matrix
- publishes immutable versioned assets like `rune-v0.2.0-linux-x64.tar.gz`
- publishes moving latest-channel assets like `rune-latest-linux-x64.tar.gz`
- updates the `release-branch-latest` GitHub Release

Versioning details are documented in [Docs/VERSIONING.md](Docs/VERSIONING.md).

## Arduino Uno Notes

The current Arduino Uno slice now supports:

- `main()` firmware entry
- or Arduino-style `setup()` / `loop()` entrypoints
- packaged `arduino` stdlib resolution from `stdlib/arduino.rn`
- serial I/O through the normal Rune surface: `print`, `println`, and `input`
- lower-level serial control through `uart_*` helpers when byte-oriented hardware control is needed
- pin/timing helpers like `pin_mode`, `digital_write`, `analog_write`, `delay_ms`, `millis`, and board constants
- real `.hex` generation and flashing with packaged AVR tools

Example:

```rune
from arduino import (
    led_builtin,
    mode_output,
    pin_mode,
)

def setup() -> unit:
    pin_mode(led_builtin(), mode_output())
    println("Rune on Uno!")

def loop() -> unit:
    return
```

## Development Rules

This repo is intentionally strict:

- no empty scaffolding
- no placeholder compiler passes
- no parser-only fake features
- no “implemented” claims without semantics, diagnostics, codegen, runtime behavior, and tests

Those rules are defined in [AGENTS.md](AGENTS.md).
