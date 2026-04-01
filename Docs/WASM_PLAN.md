# Rune WASM Plan

## Goal

Make Rune a serious WebAssembly target with clearly defined host/runtime behavior.

## Current Implemented State

Rune currently supports a real `wasm32-unknown-unknown` slice for the LLVM-supported subset.

Current output:

- `wasm32-unknown-unknown` -> `.wasm` + generated `.js` loader/runtime sidecar
- `wasm32-wasip1` -> direct `.wasm` WASI command module

Current verified runtime:

- Node.js
- packaged Wasmtime runtime for `wasm32-wasip1`

Current verified behavior:

- `print` / `println`
- `eprint` / `eprintln`
- `flush` / `eflush`
- `input`
- `panic`
- `time`
- `system`
- `env`
- TCP connectivity probes through host imports
- filesystem read/write for Node-hosted JS runtime and direct WASI runtime
- terminal control for Node-hosted JS runtime and direct WASI runtime
- bell/audio notification for Node-hosted JS runtime and direct WASI runtime

## Important Constraint

WASM is not one environment.

Rune must distinguish:

- JS-hosted browser WASM
- JS-hosted Node WASM
- WASI

These are different runtime environments with different capabilities.

## Scope Separation

### Node-hosted WASM

This is the current verified Rune WASM runtime.

Capabilities can include:

- console I/O
- stdin-like input
- environment variables
- process helpers
- host-backed networking
- host-backed filesystem access
- host-backed terminal manipulation
- host-backed bell/audio

### Browser-hosted WASM

This is planned.

Capabilities should include:

- browser console output
- host input bridges
- browser-safe timing
- `fetch`
- WebSocket

Browser-hosted Rune WASM must not claim:

- raw TCP
- raw UDP
- unrestricted process/env APIs

### WASI

This now exists for the current supported slice through `wasm32-wasip1`.

Capabilities should be defined by what the runtime actually exposes, not by native assumptions.

For Rune's current direct Wasmtime path, relative guest filesystem access is expected to go through a preopened guest `.` mapping.

## Current Compiler Model

Rune's current wasm path uses:

- Rune frontend
- Rune IR
- LLVM IR generation
- `llc`
- `wasm-ld`
- generated JS host sidecar for `wasm32-unknown-unknown`
- Rust WASI wrapper/runtime + packaged Wasmtime runtime for `wasm32-wasip1`

This is the correct direction for the current compiler architecture.

## Runtime Contract

WASM Rune programs call imported `rune_rt_*` symbols.

The current JS sidecar and the direct WASI runtime wrapper satisfy those imports for their respective targets.

This should continue to be the foundation for WASM support.

## Near-Term WASM Phases

### Phase 1: Node + direct WASI/Wasmtime runtimes

Status: implemented for the current supported slice.

### Phase 2: Browser Runtime

Target:

- generated browser-safe JS loader
- browser console and input support
- browser-safe time helpers
- host-backed HTTP/fetch support
- host-backed WebSocket support

### Phase 3: WASI Runtime Expansion

Target:

- broader WASI stdlib coverage
- capability-aware environment/system support
- clearer networking/runtime capability mapping

### Phase 4: Extended WASM Stdlib

Target:

- documented per-host stdlib capability tables
- browser/Node/WASI runtime-specific feature mapping

## Networking Plan

### Node

Node-hosted Rune WASM may expose host-backed:

- TCP connect
- TCP server
- UDP
- HTTP client
- WebSocket

This should remain host-mediated.

### Browser

Browser-hosted Rune WASM should expose:

- HTTP via host `fetch`
- WebSocket via host JS APIs

It should not expose fake raw sockets.

### WASI

WASI networking should only be claimed when the selected runtime provides the needed capabilities.

## Full WASM Scope Definition

Rune WASM should be considered complete for a declared scope only when:

- the compiler emits valid `.wasm`
- the host ABI is documented
- the host runtime exists for the declared environment
- stdlib behavior for that environment is documented
- tests verify end-to-end execution

## Current Honest Boundaries

Implemented today:

- Node-hosted Rune WASM runtime for `wasm32-unknown-unknown`
- direct Wasmtime-hosted Rune WASM runtime for `wasm32-wasip1`

Not yet complete:

- browser host runtime
- full async/runtime parity
- full stdlib parity
- full dynamic/runtime feature parity

## Recommended Next WASM Implementation Order

1. Browser loader/runtime
2. browser-safe HTTP/fetch support
3. browser-safe WebSocket support
4. expand direct WASI runtime coverage
5. target-specific stdlib capability documentation
