# Tool Bundles

This directory is where Rune expects packaged toolchain assets to exist locally.

Examples:

- `tools/llvm21/`
- `tools/llvm-avr/` for packaged host LLVM tools with the AVR backend enabled
- `tools/llvm-cbe/` for the vendored LLVM C backend source tree and local built binaries during development
- `tools/wasmtime/`
- `tools/arduino-avr/`

Those bundles are intentionally not tracked in source control because they are far too large for a normal GitHub source repository and exceed standard GitHub file-size limits.

Source repo policy:

- source code, tests, docs, installers, manifests, and runtime integration live in Git
- packaged LLVM/LLD/Wasmtime/Arduino-AVR bundles are treated as release/distribution assets
- release bundles also package host `llvm-avr` binaries under `tools/llvm-avr/<host>/bin`
- the vendored `llvm-cbe` source tree lives in Git under `tools/llvm-cbe/`
- host `llvm-cbe` binaries are packaged into release bundles under `tools/llvm-cbe/<host>/bin`

Local development can still place those bundles here, and Rune will use them.
