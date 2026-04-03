# LLVM AVR Toolchain Experiment

This repository contains:

- a packaged AVR-capable LLVM tool directory under [`tools/llvm-avr/`](../tools/llvm-avr)
- the source/build experiment that produced it under [`tools/llvm-project-avr/`](../tools/llvm-project-avr)

The goal is to produce a Rune-usable LLVM build that includes the AVR backend, so Rune can keep using LLVM for AVR-oriented code generation rather than relying on an external host install.

## Current Status

Verified:

- packaged AVR-capable LLVM tools exist under `tools/llvm-avr/windows-x64/bin`
- `llc.exe` exists in `tools/llvm-project-avr/build/Release/bin`
- `clang.exe` exists in `tools/llvm-project-avr/build/Release/bin`
- `opt.exe` exists in `tools/llvm-project-avr/build/Release/bin`
- `llc --version` reports `avr`
- `llc --print-targets` reports `avr`
- `clang --print-targets` reports `avr`
- `clang --target=avr -mmcu=atmega328p -S` produces AVR assembly output
- Rune now discovers this AVR-capable LLVM toolchain automatically for:
  - `rune emit-asm --target avr-atmega328p-arduino-uno`
  - LLVM-backed AVR object emission for `rune build --object --target avr-atmega328p-arduino-uno`
- `rune toolchain` now reports the packaged AVR tools as `avr llc` and `avr clang`

Not fully completed in this environment:

- a single all-target LLVM build was too large to finish within the interactive time budget
- the broader build tree still includes in-progress Visual Studio targets beyond the core tools above
- the Arduino Uno firmware build still uses Rune's working AVR-specific firmware path; this LLVM AVR toolchain is currently used for AVR asm/object emission, not yet for the full flashed firmware pipeline

## Exact Commands

Clone:

```powershell
git clone --depth 1 --filter=blob:none --branch llvmorg-21.1.7 https://github.com/llvm/llvm-project.git tools\llvm-project-avr
```

Configure:

```powershell
cmake -S tools\llvm-project-avr\llvm -B tools\llvm-project-avr\build -G "Visual Studio 17 2022" -A x64 "-DLLVM_ENABLE_PROJECTS=clang" "-DLLVM_TARGETS_TO_BUILD=AVR;X86" -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_BUILD_TESTS=OFF -DLLVM_BUILD_DOCS=OFF -DLLVM_ENABLE_ASSERTIONS=OFF -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL
```

Build:

```powershell
cmake --build tools\llvm-project-avr\build --config Release --target llc clang opt -- /m
```

Proof checks:

```powershell
tools\llvm-project-avr\build\Release\bin\llc.exe --version
tools\llvm-project-avr\build\Release\bin\llc.exe --print-targets
tools\llvm-project-avr\build\Release\bin\clang.exe --print-targets
tools\llvm-project-avr\build\Release\bin\clang.exe --target=avr -mmcu=atmega328p -S .\tools\llvm-project-avr\test-avr.c -o .\tools\llvm-project-avr\test-avr.s
```

## Notes

- The build is isolated to `tools/llvm-project-avr/`.
- This is an experimental toolchain lane, not part of the released Rune compiler surface yet.
- The repository's working compiler/runtime code was not modified for this experiment.
