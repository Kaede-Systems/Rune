# CLAUDE.md

This file provides guidance to code assistants working with code in this repository.

## What This Is

**Rune** is a native compiled programming language (`.rn` files) with Python-inspired indentation-sensitive syntax. The compiler is written in Rust (crate name `rune`, binary named `rune`). This repo is the full compiler and toolchain workspace.

## Build and Run

```powershell
# Build the compiler
cargo build

# Run the compiler (any subcommand)
cargo run -- <subcommand> [args]

# Examples
cargo run -- check calculator.rn
cargo run -- build calculator.rn -o calculator.exe
cargo run -- emit-ir calculator.rn
cargo run -- emit-llvm-ir calculator.rn
cargo run -- emit-asm calculator.rn --target x86_64-unknown-linux-gnu
cargo run -- emit-avr-precode buzzer_serial_control_arduino.rn
cargo run -- build hello_arduino.rn --target avr-atmega328p-arduino-uno -o hello_arduino.hex
cargo run -- build calculator.rn --target wasm32-wasip1 -o calculator_wasi.wasm
cargo run -- run-wasm calculator_wasi.wasm --host wasmtime
```

## Tests

```powershell
# Run all tests
cargo test

# Run a specific test file (e.g. parser tests)
cargo test --test parser_tests

# Run a single test by name
cargo test --test parser_tests test_name_here

# Run tests matching a pattern
cargo test --test semantic_tests -- keyword
```

Test files live under `tests/` — one file per compiler stage or CLI subsystem.

## Architecture

All compiler source lives in a single flat crate at `src/`. The modules are:

**Frontend / Language Model**
- `lexer.rs` — tokenizer with INDENT/DEDENT handling for indentation-sensitive syntax
- `parser.rs` — recursive descent parser → AST
- `semantic.rs` — name resolution and type checking
- `warnings.rs` — warning collection pass
- `ir.rs` — lowers AST to Rune IR (the internal representation used by native codegen)
- `optimize.rs` — IR optimizer and dead-function pruner (`prune_program_for_executable`)
- `module_loader.rs` — resolves and loads `.rn` source files and stdlib modules
- `builtin_modules.rs` — native Rust implementations of built-in stdlib modules

**Backends**
- `codegen.rs` — handwritten native backend (Windows x86-64)
- `llvm_ir.rs` — emits LLVM IR text from Rune IR
- `llvm_backend.rs` — drives packaged LLVM/LLD tools for LLVM-backed builds (object files, assembly, linked executables, WASM)
- `avr_cbe_opt.rs` — AVR/Arduino path: Rune IR → LLVM IR → llvm-cbe C → AVR GCC → .elf/.hex
- `toolchain.rs` — discovers and verifies packaged LLVM, LLD, AVR GCC, avrdude, Wasmtime tools

**Build Driver**
- `build.rs` (Rust build script) — sets up LLVM linking
- `src/main.rs` — CLI entry point; parses subcommands and dispatches to library functions
- `lib.rs` — re-exports all modules

**Standard Library**
- `stdlib/` — source-backed `.rn` stdlib modules not yet promoted to built-in status
- Built-in modules (`env`, `io`, `time`, `clock`, `gpio`, `serial`, `network`, etc.) are implemented natively in `builtin_modules.rs`

**Packaging / Tools**
- `tools/` — vendored packaged toolchain: LLVM xpack, Wasmtime, llvm-cbe source and binaries
- `installers/` — Windows and Unix installer scripts
- `Docs/` — syntax, grammar, ABI, WASM, targets, versioning docs

## Pipeline Summary

**Host native (Windows):** `.rn` → Lexer → Parser → Semantic → IR → Optimize → Native Codegen → `.exe`

**LLVM-backed:** `.rn` → Lexer → Parser → Semantic → IR → LLVM IR → packaged `clang`/`lld` → target binary

**Arduino AVR:** `.rn` → IR → LLVM IR → `llvm-cbe` → transient C → packaged AVR GCC/G++ + Arduino core → `.elf` → `.hex`

**WASM/WASI:** LLVM-backed → packaged `wasm-ld` → `.wasm`; run via packaged Wasmtime

## Key Language Notes

- Rune source files use `.rn` extension with significant indentation (no braces)
- Import syntax: `from module import name` or `import module` (aliases not yet implemented)
- Arduino entry points: `main()` or Arduino-style `setup()` / `loop()`
- `dynamic` values are supported in both the native and LLVM backends

---

## Development Policies (from AGENTS.md)

### Core Principle

This project is building a real language and compiler, not a prototype made of placeholders.

Everything added to the codebase must earn its place by being real, functional, and internally honest.

### Development Policy

Development may happen in stages over many days. That is allowed.

What is not allowed:
- fake progress
- hollow architecture
- placeholder implementations
- advertising unfinished features as finished

We may build incrementally, but every implemented part must be real.

### No-Scaffolds Policy

Scaffolding is forbidden when it creates the appearance of progress without delivering working behavior.

Do not add:
- empty modules or empty crates
- placeholder files or stub compiler passes
- fake AST/HIR/MIR layers with no real role
- unimplemented APIs added "for later"
- dummy runtime components
- syntax accepted by the parser but unsupported semantically
- semantic features accepted by analysis but unsupported in codegen
- codegen hooks that do nothing

Allowed: design documents, specifications, implementation plans, completed vertical slices.

**Rule:** If a component is added to the codebase, it must do real work now. If it is not ready to do real work, it stays out of the codebase and remains only in docs or planning.

### No-Placeholder Policy

Do not use:
- `todo!()`
- `unimplemented!()`
- panic-based fake behavior standing in for real logic
- hardcoded temporary return values pretending to be real results
- mock language behavior inside the real compiler pipeline
- "accept now, implement later" feature branches in the language

### Honesty Policy

A feature exists only when it is implemented end-to-end for its declared scope.

A feature is not considered implemented merely because the syntax parses, the AST node exists, the type checker mentions it, the backend has a named file for it, or a document says it is planned.

If any required stage is missing, the feature is not complete.

### Definition of Implemented

A language feature counts as implemented only when all applicable parts are complete:
- syntax and parsing
- AST or equivalent representation
- name resolution and semantic analysis
- type checking
- diagnostics
- code generation
- runtime behavior if required
- tests

### Backend Parity Policy

If a feature is part of the current language or stdlib scope, it must compile through every backend advertised for that scope. Error messages like "X is not supported by the current backend" are only acceptable while a feature is still outside the declared scope.

### Release Completeness Policy

Any release must be complete for its declared scope:
- no half-finished released features
- no partially supported syntax in a release
- no release notes claiming support beyond what actually works
- no silent gaps between parser, semantics, and backend

Every release must define a scope. Everything inside that scope must be complete. Everything outside that scope must be explicitly excluded.

### Branch Discipline Policy

- ongoing implementation work is committed to `main`
- `release` is not the default development branch
- `release` should only receive changes when a feature batch is actually ready for release
- the normal path is `main` → review/audit → PR/merge into `release`
- do not continue long-running feature work directly on `release`

### Vertical Slice Policy

Preferred pattern:
1. choose a real language capability
2. implement it fully through the compiler pipeline
3. test it
4. only then move to the next capability

Avoid building many disconnected layers that look complete but cannot compile real programs.

### Architecture Discipline Policy

Architecture is allowed only when it is justified by current needs or by a very near-term implementation requirement. Do not create files, modules, crates, interfaces, or abstractions solely because a mature compiler might eventually need them.

### Stdlib Architecture Policy

- built-in stdlibs must be implemented as real Rust-side modules, runtime bindings, or equivalent native compiler structures
- once a stdlib is claimed to be built-in, it must not be implemented as an embedded `.rn` source blob hidden inside Rust code
- stdlib APIs must be wired end-to-end through parsing, semantics, type checking, IR, codegen, runtime, diagnostics, and tests for every backend in the claimed scope

### Testing Policy

Tests are part of completion, not an optional cleanup step.

Every implemented feature should have appropriate tests including: valid examples, invalid examples, diagnostic expectations, runtime behavior, codegen-sensitive cases.

### Documentation Policy

Docs must clearly separate: implemented, planned, rejected, unresolved. Documentation must never blur the line between current reality and future intent.

### Completion Policy

When we start implementing a feature, the goal is to finish it for the declared scope before moving on. Do not leave trails of partial implementation across the codebase.

### Enforcement Rule

When deciding whether something belongs in the repository, ask:
1. Does it do real work now?
2. Is it complete for the scope we are claiming?
3. Would a new contributor mistake this for a finished feature when it is not?

If the answer to question 1 or 2 is no, it should not be merged. If the answer to question 3 is yes, it must be redesigned, completed, or removed.
