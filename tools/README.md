# Tool Bundles

This directory is where Rune expects packaged toolchain assets to exist locally.

Examples:

- `tools/llvm21/`
- `tools/wasmtime/`

Those bundles are intentionally not tracked in source control because they are far too large for a normal GitHub source repository and exceed standard GitHub file-size limits.

Source repo policy:

- source code, tests, docs, installers, manifests, and runtime integration live in Git
- packaged LLVM/LLD/Wasmtime bundles are treated as release/distribution assets

Local development can still place those bundles here, and Rune will use them.
