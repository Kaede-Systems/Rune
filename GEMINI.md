# Gemini Context: Rune Language Project

This project is the **Rune** language compiler and toolchain. Rune is a native compiled programming language with Python-inspired syntax, featuring a Rust-based compiler, an internal IR, and multiple backends including LLVM, AVR (Arduino Uno), and WASM/WASI.

## Project Overview

- **Implementation Language:** Rust (stable edition 2024).
- **Target Language:** Rune (`.rn` files).
- **Syntax Philosophy:** Python-like readability with significant indentation, but statically typed and natively compiled.
- **Core Architecture:** Lexer -> Parser -> AST -> Semantic Analysis -> IR -> LLVM IR/CBE -> Native/AVR/WASM.
- **Key Features:**
    - Native execution (Windows, Linux, macOS).
    - Embedded support (Arduino Uno via AVR).
    - WebAssembly support (WASI/Wasmtime).
    - C FFI (calling C from Rune and vice-versa).
    - Standard library for I/O, networking, GPIO, and more.

## Development Mandates (from AGENTS.md)

- **No Placeholders:** `todo!()`, `unimplemented!()`, and empty stubs are forbidden in the codebase.
- **Honesty:** A feature is only "implemented" if it works end-to-end (syntax, semantics, codegen, runtime, and tests).
- **Vertical Slices:** Features should be implemented fully through the pipeline rather than layer-by-layer across all features.
- **No Scaffolding:** Do not add empty modules or files "for later."

## Key Commands

### Compiler Development
- **Build Compiler:** `cargo build`
- **Run Tests:** `cargo test`
- **Check Compiler Style:** `cargo clippy`

### Using Rune
- **Check a Rune File:** `cargo run -- check <file.rn>`
- **Build Native Executable:** `cargo run -- build <file.rn> -o <output.exe>`
- **Build for Arduino Uno:** `cargo run -- build <file.rn> --target avr-atmega328p-arduino-uno --flash --port <COM_PORT>`
- **Build for WASI:** `cargo run -- build <file.rn> --target wasm32-wasip1 -o <file.wasm>`
- **Run WASM:** `cargo run -- run-wasm <file.wasm> --host wasmtime`
- **Emit IR/LLVM:** `cargo run -- emit-ir <file.rn>` or `cargo run -- emit-llvm-ir <file.rn>`

## Repository Structure

- `src/`: Rust source code for the compiler (lexer, parser, semantic, codegen, etc.).
- `stdlib/`: Rune standard library modules (`io.rn`, `gpio.rn`, `network.rn`, etc.).
- `tests/`: Comprehensive test suite for all compiler stages and runtimes.
- `tools/`: Packaged toolchain assets (LLVM-AVR, Wasmtime, Arduino cores).
- `Docs/`: Detailed specifications (Syntax, Grammar, ABI, Plans).
- `examples/`: Sample Rune programs for various targets.

## Coding Standards

- **Rust:** Follow idiomatic Rust patterns. Use the provided diagnostics framework in `src/diagnostics.rs` for compiler errors.
- **Rune:** Adhere to the syntax defined in `Docs/SYNTAX.md` and `Docs/GRAMMAR.md`.
- **Honesty Rule:** When implementing a feature, ensure it is wired through to the backend and verified with tests before considering it complete.
