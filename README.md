# Rune

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

Rune is ambitious, but the repository is kept honest. The language vision is larger than the current fully-complete release surface, and the docs call that out explicitly.

## Language Direction

Rune is aiming at:

- Python-like readability
- native compilation
- optional dynamic typing
- stronger static typing where declared
- low-level systems access
- OOP and richer type-system work over time
- cross-target native builds
- WASM and WASI support

## What Works Now

### Frontend and Core Compiler

- `rune lex`
- `rune parse`
- `rune check`
- `rune emit-ir`
- `rune emit-asm`
- `rune emit-llvm-ir`
- `rune emit-llvm-asm`
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
- `system`
- `terminal`
- `time`
- `audio`

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

Emit LLVM assembly:

```powershell
cargo run -- emit-llvm-asm calculator.rn --target x86_64-unknown-linux-gnu
```

Build WASI and run it through packaged Wasmtime:

```powershell
cargo run -- build calculator.rn --target wasm32-wasip1 -o calculator_wasi.wasm
cargo run -- run-wasm calculator_wasi.wasm --host wasmtime
```

## Main Commands

```text
rune lex file.rn
rune parse file.rn
rune check file.rn
rune emit-ir file.rn
rune emit-asm file.rn
rune emit-llvm-ir file.rn
rune emit-llvm-asm file.rn [--target <triple>]
rune emit-c-header file.rn -o file.h
rune build file.rn
rune build-llvm file.rn
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

## Release Automation

The repository now includes [`.github/workflows/release.yml`](.github/workflows/release.yml).

On pushes to the `release` branch, it builds host-native Rune bundles for the configured GitHub Actions matrix, packages them as `rune-bundle-*` assets, and publishes or updates the `release-branch-latest` GitHub Release.

## Development Rules

This repo is intentionally strict:

- no empty scaffolding
- no placeholder compiler passes
- no parser-only fake features
- no “implemented” claims without semantics, diagnostics, codegen, runtime behavior, and tests

Those rules are defined in [AGENTS.md](AGENTS.md).
