# No-Zig Toolchain Plan

## Goal

Remove Rune's build-time dependency on Zig.

The replacement must still allow Rune to:

- emit native executables
- emit shared libraries
- emit static libraries
- support Windows, Linux, and macOS targets
- cross-compile from one host to another target when the packaged toolchain for that target exists

## Current Reality

Today Rune already ships a packaged LLVM distribution in `tools/llvm21/`.

The compiler already uses those LLVM tools for:

- LLVM IR verification
- object-file generation with `llc`
- binary decompilation with `llvm-objdump`

The remaining Zig dependency is specifically in the final link/archive layer inside [src/build.rs](/C:/Users/kaededevkentohinode/KUROX/src/build.rs).

That means the real problem is not "compiler backend generation". The real problem is "target-aware final link packaging".

## Bundled Tools We Already Have

The packaged LLVM bundle already contains:

- `clang`
- `clang++`
- `llc`
- `opt`
- `ld.lld`
- `ld64.lld`
- `lld-link`
- `llvm-ar`
- `llvm-ranlib`
- `llvm-lib`
- `llvm-objdump`
- `llvm-readobj`

This is enough to start replacing Zig for:

- object processing
- archive creation
- some direct link steps
- wasm module linking
- inspection and debugging

## What Zig Is Doing Today

Zig is currently acting as a cross-target linker driver.

In practice that means it is covering several responsibilities at once:

- target triple normalization
- invoking the right linker mode
- locating CRT/runtime objects
- locating libc and related target libraries
- providing practical cross-platform defaults

So "remove Zig" does not mean "delete a binary". It means Rune must own those responsibilities directly.

## Required Replacement Layers

Rune needs a real packaged toolchain layer with four parts:

1. Object generation
- already real through LLVM IR + `llc`

2. Archive generation
- `llvm-ar` for Unix archives
- `llvm-lib` or `llvm-ar` for Windows archives

3. Final linking
- `lld-link` for PE/COFF
- `ld.lld` for ELF
- `ld64.lld` for Mach-O

4. Target runtime assets
- CRT startup objects
- compiler runtime pieces
- libc/sysroot headers and libraries
- platform SDK assets where required

## Important Constraint

macOS support is the hardest target.

Even if Rune can emit valid Mach-O objects and call `ld64.lld`, practical macOS cross-linking still depends on packaged Apple-target runtime/SDK assets. That part must be handled explicitly and honestly.

So Rune can remove Zig before it solves every macOS packaging wrinkle, but it cannot claim "fully self-contained macOS cross-linking" until those assets are truly present and tested.

## Implementation Order

### Phase 1: Shared Toolchain Layer

Complete the `src/toolchain.rs` consolidation so all packaged tool lookup is centralized.

This phase is about correctness and maintainability:

- one packaged LLVM lookup path
- one packaged archiver/linker lookup path
- one target-to-tool mapping table

### Phase 2: Static Library Output

Add real static-library generation first.

This is the cleanest no-Zig slice because it does not require a full final executable link.

Output targets:

- Windows: `.lib`
- Linux: `.a`
- macOS: `.a`

Implementation:

- Rune object via LLVM backend
- runtime object if required for exported runtime helpers
- archive via `llvm-ar` or `llvm-lib`

Status:

- implemented for archive generation through packaged LLVM tooling
- output mode exists through `rune build --static-lib`
- executable/shared-library final linking still has remaining Zig-backed paths

### Phase 3: ELF and PE Linking Without Zig

Replace Zig-backed link steps for Linux and Windows first.

Windows:

- object emission via LLVM backend
- runtime object via packaged compiler flow
- final link via `lld-link`

Linux:

- object emission via LLVM backend
- runtime object via packaged compiler flow
- final link via `ld.lld` or `clang --target ... -fuse-ld=lld`
- packaged sysroot/runtime assets

### Phase 4: Mach-O Linking Without Zig

Move macOS to packaged LLVM/lld tooling once the required SDK/runtime assets are truly packaged and tested.

### Phase 5: Explicit Link Modes

Add real CLI-level link-mode control:

- default dynamic/runtime-linked executable mode
- static executable mode where the target runtime makes that practical
- shared library mode
- static library mode

This should be reflected directly in `rune build`, not hidden in internal code paths.

## CLI Shape

Target CLI additions once implemented:

```powershell
rune build app.rn --static-lib -o librune_app.a
rune build app.rn --lib -o librune_app.so
rune build app.rn --link static -o app_static
rune build app.rn --link dynamic -o app_dynamic
```

These flags should not be exposed until each mode is truly real for the claimed targets.

## Scope Honesty

This plan does not claim that Rune is already Zig-free.

Current true state:

- packaged LLVM tools exist
- object generation exists
- Zig is still used in some final link steps

Target state:

- Rune links through its own packaged LLVM/lld-based toolchain
- Zig is no longer required or shipped

## WASM Status

Rune now has a real packaged-LLVM wasm slice:

- `wasm32-unknown-unknown` target
- linking through packaged `wasm-ld`
- generated JS host loader sidecar
- verified Node runtime behavior for print, input, and panic

This is a real wasm runtime slice, but it is not yet "full feature parity across all Rune features on wasm."

## Success Criteria

The no-Zig transition is complete only when:

- `rune build` no longer shells out to Zig
- `rune build --lib` no longer shells out to Zig
- static library output exists and is tested
- Windows/Linux/macOS target handling is explicit and documented
- the shipped toolchain assets required for each target are packaged with Rune
- tests verify the produced binaries and libraries for the declared supported targets
