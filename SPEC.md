# Rune Cross-Platform, Embedded, And Omission Specification

## 1. Purpose

This document is the working implementation specification for Rune's:

- cross-platform compiler direction
- embedded platform strategy
- stdlib layering
- whole-program omission rules
- target-runtime architecture
- ecosystem/library integration plan
- diagnostics and completeness rules for platform work

This document is intentionally broader than release notes.

It describes:

- what Rune is trying to become
- what order we will build that in
- what counts as complete for each slice
- what we explicitly do not claim yet

This document is not a release-completeness waiver.

If this document says a feature is planned, that does not mean the feature is
implemented.

If this document says a target is planned, that does not mean Rune currently
supports that target end to end.

## 2. Core Goals

Rune should become a language that can credibly replace C for a large class of:

- desktop systems programming
- command-line tools
- native applications
- embedded firmware
- board-level hardware programming
- cross-platform device tooling

Rune should aim for:

- low-level control
- native compilation
- strong optimization
- predictable generated artifacts
- Python-like readability where reasonable
- OOP that does not sacrifice systems-level performance
- progressively broader platform reach without fake support

The long-term platform vision includes:

- Windows
- Linux
- macOS
- Arduino-class boards
- ESP32-class boards
- Raspberry Pi-class boards
- WASM / WASI

## 3. Non-Negotiable Project Rules

This specification must be read together with `AGENTS.md`.

The following rules are mandatory:

- no placeholder features
- no parser-only or semantic-only fake support
- no documented feature without backend/runtime support for declared scope
- no target marked implemented unless it builds and runs end to end for the
  declared slice
- no partial public stdlib surface that is not wired through semantics,
  codegen/runtime, and tests

For platform work specifically:

- a shared stdlib surface must only include capabilities backed by real targets
- a target-specific stdlib must only expose what the current target runtime
  actually supports
- “planned breadth” must remain in this spec until it is implemented

## 4. Foundational Rule: Omit What Is Not Used

Rune should omit unused functionality from final artifacts across all targets.

This is not a minor optimization.

This is a core compiler rule.

### 4.1 What Must Be Omitted When Unused

Where technically possible, Rune should omit:

- unreachable functions
- unreachable methods
- unreachable constructors
- unreachable structs/classes
- unreachable top-level script glue
- unused stdlib wrappers
- unused runtime helper functions
- unused target-specific helper code
- unused packaged library glue
- unused import-driven board/runtime adapters

### 4.2 Examples

Expected behavior:

- if a program never reaches `print`, then `print` support should not appear in
  the final artifact
- if a program never imports or reaches servo support, servo glue should be
  omitted
- if a Uno program never uses serial, serial helper/runtime code should not be
  dragged in by default
- if a host program never touches filesystem paths, filesystem support should
  not be retained unless something reachable requires it
- if a program imports a module but only reaches one helper function, the rest
  should be omittable

### 4.3 Scope Of The Rule

This rule applies to:

- native builds
- LLVM builds
- AVR/Arduino builds
- future ESP32 builds
- future Raspberry Pi hardware/runtime builds
- future static/shared library outputs where practical

### 4.4 How Omission Should Be Achieved

Rune should combine:

- front-end/program reachability pruning
- stdlib wrapper pruning
- runtime symbol pruning where possible
- target-specific glue pruning
- linker dead stripping
- section garbage collection
- LTO where appropriate

Compiler reachability and linker dead stripping must work together.

Neither one alone is enough.

### 4.5 Acceptance Criteria

A target/build path can claim omission support only when:

- unreachable Rune items are pruned or proven removable
- targeted tests verify that dead helpers do not survive into emitted output
- emitted precode or target glue does not retain obviously dead board/library
  code
- the build still works correctly for reachable features

## 5. Priority Order

Platform and embedded work should be implemented in this order:

1. compiler-wide omission and reachability
2. universal `time`
3. universal `network`
4. common `gpio`
5. common `serial`
6. common `pwm`
7. common `adc`
8. common `spi`
9. common `i2c`
10. common `servo`
11. target-specific escape hatches
12. embedded/library-oriented CFFI
13. ecosystem-specific libraries beyond the core transport/hardware layers

### 5.1 Why This Order

- omission affects every platform and every binary
- `time` affects nearly every target and almost every real program
- `network` affects desktop, Raspberry Pi, ESP32, WASM, and any future hosted targets
- `gpio` affects Arduino, ESP32, and Raspberry Pi
- `serial` affects both host and embedded workflows
- `pwm` / `adc` are common hardware needs across embedded targets
- `spi` / `i2c` unlock many sensor and device ecosystems
- `servo` is important but narrower than the previous layers
- target-specific modules should sit on top of the shared layers, not replace
  them
- CFFI should come after the common target/runtime model is stable enough to
  support it honestly

## 6. Current Reality

This section records the current baseline at the time of this spec.

### 6.1 Implemented Today

Verified current reality:

- native targets exist and build through the current Rune pipelines
- LLVM-backed executable paths exist
- Uno AVR target exists as a real embedded slice:
  - target: `avr-atmega328p-arduino-uno`
  - `.hex` and `.elf` outputs
  - packaged Arduino AVR core/toolchain integration
  - `llvm-cbe`-backed transient C path for Uno
- a real `gpio` stdlib surface exists today
- current Uno build path already performs Rune-item reachability pruning before
  precode/toolchain compilation

### 6.2 Not Yet Implemented

Not yet implemented as real Rune target slices:

- ESP32 target
- Raspberry Pi GPIO/runtime target
- complete embedded CFFI
- complete Arduino ecosystem parity
- complete ESP32 ecosystem parity
- complete Raspberry Pi ecosystem parity

### 6.3 Honesty Rule

These not-yet-implemented targets and surfaces must stay documented as planned
until:

- target definition exists
- build path exists
- runtime backing exists
- tests exist
- docs are updated to reflect the actual scope

## 7. Stdlib Layering Model

Rune stdlibs should be layered intentionally.

### 7.1 Universal Core Modules

These are platform-wide foundational modules:

- `time`
- `sys`
- `env`
- `io`
- `terminal`
- `fs`
- `network`
- `json`
- `audio` where supported

Rules:

- the module may exist broadly
- but functions inside it must remain honest about capability differences
- unavailable capabilities must not be silently advertised as universal

### 7.2 Shared Embedded Surface Modules

These are the cross-board hardware-facing abstractions:

- `gpio`
- `serial`
- `pwm`
- `adc`
- `spi`
- `i2c`
- `servo`

These should be the main user-facing portability layers across:

- Arduino
- ESP32
- Raspberry Pi

These should only claim support per target where they are truly backed.

### 7.3 Target-Specific Escape Hatch Modules

These expose board or platform specific behavior that does not belong in the
portable shared layers.

Current/planned examples:

- `arduino`
- future `esp32`
- future `rpi`

Rules:

- shared portable APIs go into shared modules first
- board-specific extras go into target modules
- target modules must not be used as an excuse to skip the common surface

## 8. Universal `time` Module Specification

`time` is not only an OS module.

It is a universal platform module.

It should work across:

- Windows
- Linux
- macOS
- Raspberry Pi
- Arduino
- ESP32
- WASM/WASI where host capabilities allow

### 8.1 Cross-Platform Meaning

On desktop/host targets:

- wall-clock time
- monotonic time
- sleep
- tick/elapsed helpers

On Arduino:

- `millis`
- `micros`
- `delay`
- `delayMicroseconds`

On ESP32:

- equivalent core timing functions

On Raspberry Pi:

- Linux-backed time + optional board-level timing helpers if needed

### 8.2 Target-Neutral Design Goal

Users should be able to write timing-oriented Rune code with similar shape across
desktop and embedded where that is semantically reasonable.

### 8.3 Planned `time` Direction

The long-term `time` direction should include a clean combination of:

- wall time
- monotonic time
- sleep
- tick/elapsed helpers
- capability-specific additions only where honestly backed

## 9. Universal `network` Module Specification

`network` is a universal core stdlib module, not an embedded-hardware module.

It should be treated more like `time` than like `gpio`.

### 9.1 Why `network` Is Universal

`network` affects:

- Windows
- Linux
- macOS
- Raspberry Pi
- ESP32
- Node/WASM host environments where supported
- WASI where capabilities permit

It does not naturally belong in the shared board-hardware layer.

### 9.2 Target Meaning

Desktop/native targets:

- TCP/UDP and higher-level helpers where implemented

Raspberry Pi:

- Linux-backed network behavior, same family as native Linux

ESP32:

- target-backed network slice only after the first real ESP32 target exists

WASM / WASI:

- capability-dependent runtime support

Arduino Uno-class no-network targets:

- `network` must not pretend to work there
- unsupported capability must remain explicit

### 9.3 Design Rule

The `network` module may exist as part of the universal stdlib, but each target
must only claim the subset it really supports.

That means:

- no fake network support on bare no-network targets
- no parser/semantic acceptance followed by hidden target failure for in-scope
  network features
- capability-based docs per target/runtime

### 9.4 Planned Work Order For `network`

1. keep `network` in the universal core track
2. continue improving host/native support
3. keep WASM/WASI behavior explicit and capability-based
4. add Raspberry Pi support as part of Linux-backed target work
5. add ESP32 support only once the real ESP32 target/runtime exists
6. never advertise Uno-class direct network support without a real backing stack

## 10. `gpio` Module Specification

`gpio` is the first major portable embedded abstraction.

It is high priority because it affects:

- Arduino
- ESP32
- Raspberry Pi

### 9.1 Core `gpio` Capabilities

Portable `gpio` should cover:

- digital output
- digital input
- input with pull-up
- write/high/low
- read
- toggle
- blink/pulse helpers
- pin wrappers/objects where they remain zero-cost enough

### 9.2 Boundary Rule

`gpio` must not claim ESP32 or Raspberry Pi support until those backends exist.

Current truthful state:

- `gpio` exists as a real shared surface
- current backing is the Arduino Uno target

### 9.3 Expansion Order

1. harden current Uno `gpio` surface
2. add more reusable common behavior
3. back it with ESP32
4. back it with Raspberry Pi

## 11. `serial` Module Specification

`serial` is a shared host-and-embedded transport layer.

It should support:

- host serial tools/connectors/readers
- board serial I/O
- embedded interactive tooling

### 10.1 Portable Goals

The `serial` surface should unify:

- host-side serial connection and line/byte transport
- embedded serial-backed `print` / `input` style flows
- lower-level byte control when needed

### 10.2 Target Backing

Arduino:

- core serial / UART

ESP32:

- UART / serial facilities

Raspberry Pi:

- Linux serial devices

Windows/Linux/macOS host tools:

- serial file/device access for connectors and interactive CLIs

## 12. `pwm`, `adc`, `spi`, `i2c`, `servo`

These modules are next after `time`, `gpio`, and `serial`.

### 11.1 `pwm`

Portable abstraction for:

- duty-cycle output
- duty limits
- turning channels off

### 11.2 `adc`

Portable abstraction for:

- analog input reads
- percent conversion
- voltage conversion where the reference is explicit and honest

### 11.3 `spi`

Portable abstraction for:

- bus setup
- transfer/write/read
- chip select patterns where appropriate

### 11.4 `i2c`

Portable abstraction for:

- begin/connect
- read/write
- address-based device communication

### 11.5 `servo`

Portable abstraction for:

- attach/detach or enable/disable
- angle writes where honest
- pulse writes
- calibration helpers

`servo` should remain above target-specific libraries while still allowing lower
level target escape hatches.

## 13. Arduino Specification

Rune should provide a real Arduino integration path, not a toy path.

### 12.1 Goals

Rune on Arduino should eventually support:

- packaged Arduino core integration
- packaged high-value Arduino libraries
- shared portable hardware surfaces
- lower-level Arduino escape hatches
- omission of unused board/library glue

### 12.2 Build Pipeline Direction

Current truthful Uno build direction is:

- Rune source
- semantic analysis
- optimization / pruning
- LLVM IR
- `llvm-cbe`
- transient C
- packaged Arduino core + AVR GCC/G++
- `.elf`
- `.hex`
- flash

Generated intermediate code should remain transient unless explicitly emitted by
debug/precode commands.

### 12.3 Arduino Work Order

1. continue compiler-wide omission
2. harden shared `time`, `gpio`, `serial`, `pwm`, `adc`
3. add portable `spi`
4. add portable `i2c`
5. keep `servo` as shared layer backed by Arduino support
6. expand low-level `arduino` module for board/core-specific behavior
7. port high-value Arduino libraries one at a time

### 12.4 Arduino Core Coverage Plan

High-priority Arduino coverage:

- core GPIO/timing/serial
- interrupts
- random helpers
- shift register helpers
- Servo
- SoftwareSerial
- SPI
- Wire / I2C

Medium-priority Arduino-specific expansion:

- additional timing/control helpers
- reusable board constants
- common IO patterns

### 12.5 Library Porting Rule

Only port an Arduino library into Rune when:

- the library is genuinely common/useful
- the build path packages it honestly
- only used libraries are compiled/linked
- the Rune surface is wired end to end
- tests exist

### 12.6 Current Arduino Truth Boundary

Rune does not yet have full Arduino ecosystem parity.

That remains a goal, not a present fact.

## 14. ESP32 Specification

ESP32 support should begin as a real packaged target slice.

### 13.1 First ESP32 Slice

The first truthful slice should include:

- packaged `arduino-esp32` core/toolchain assets
- a real Rune target definition
- shared `time`
- shared `gpio`
- shared `serial`
- shared `pwm`
- shared `adc`
- `sys` target/board detection

### 13.2 After The First Slice

Then add:

- board-specific ESP32 helpers
- more packaged ESP32 libraries
- Wi-Fi / networking libraries where realistically supportable
- more target-specific facilities only after they are end to end

### 13.3 What Must Not Happen

We must not:

- create a fake `esp32` stdlib full of unsupported declarations
- document ESP32 libraries before the target exists
- accept source surfaces that codegen/runtime cannot fulfill

## 15. Raspberry Pi Specification

Raspberry Pi support should start as a Linux-backed hardware/runtime slice.

### 14.1 First RPi Slice

The first truthful slice should include:

- shared `time`
- shared `gpio`
- shared `serial`
- `sys` board/platform detection

### 14.2 Later Expansion

After the first slice:

- `pwm`
- `spi`
- `i2c`
- additional `rpi`-specific helpers where really implemented

### 14.3 Important Scope Rule

Raspberry Pi support does not initially mean:

- all Raspberry Pi ecosystem libraries
- all Linux device stacks
- all board/vendor frameworks

It means a real, honest first hardware/runtime slice.

## 16. Embedded And Ecosystem CFFI Specification

Rune should eventually support proper library integration across host and
embedded targets.

### 15.1 Priority Order

1. common FFI model
2. hosted/native library integration
3. embedded compile/link integration
4. board/ecosystem library consumption

### 15.2 Embedded Meaning

For Arduino and ESP32, “CFFI” should primarily mean:

- compiling and linking C/C++ libraries into the target build
- exposing symbols through real Rune declarations

It should not begin by pretending MCU targets have Python-style runtime dynamic
loading.

### 15.3 Requirements

Embedded/library CFFI must define:

- calling convention
- ownership and lifetime rules
- ABI expectations
- static vs dynamic linking model
- target/toolchain integration behavior

### 15.4 Ecosystem Goal

Long-term goal:

- the C Arduino ecosystem
- the C/C++ ESP32 ecosystem
- board-specific low-level libraries

But each library path must be implemented in completed slices.

## 17. Whole-Program Omission Work Breakdown

### 16.1 Stage 1: Rune Item Reachability

Extend current pruning from the Uno path into all current build pipelines.

Targets/build paths:

- native
- LLVM executables
- AVR/Uno
- future ESP32
- future Raspberry Pi hardware builds

### 16.2 Stage 2: Stdlib Wrapper Reachability

Ensure unused stdlib wrappers are not retained unnecessarily.

This includes:

- function wrappers
- class wrappers
- helper methods

### 16.3 Stage 3: Runtime Helper Reachability

Ensure target runtime code is retained only when needed.

Examples:

- print runtime only when print is reachable
- serial runtime only when serial/input paths are reachable
- servo glue only when servo support is reachable

### 16.4 Stage 4: Library/Target Glue Reachability

Only compile/link packaged board libraries when they are actually used.

Examples:

- Servo only if servo symbols are reachable
- SoftwareSerial only if used
- future SPI/Wire glue only if used

### 16.5 Stage 5: Linker Cooperation

Retain and improve:

- section GC
- LTO
- dead-strip-friendly code structure

### 16.6 Acceptance Criteria

We should consider omission mature per target when:

- dead helpers are absent from emitted precode/IR where observable
- unused stdlib wrappers do not force unnecessary runtime glue
- unused board-library code is not compiled/linked by default
- targeted tests confirm all of the above

## 18. Diagnostics And Error Code Specification

Rune should classify platform/build/runtime failures clearly.

### 17.1 Error Classes

- target resolution errors
- unsupported target/runtime-scope errors
- tool-not-found errors
- tool failure errors
- codegen/runtime ABI errors
- flash/serial/board connection errors
- link-time/library-integration errors

### 17.2 Principle

Platform work should not collapse into generic “build failed” errors when the
real problem is:

- missing toolchain
- wrong target
- unsupported board slice
- flash port issue
- runtime ABI mismatch

## 19. Target Completion Criteria

A target can only be called implemented for a scope when all of these are true
for that scope:

- target resolution exists
- parser/semantics accept the surface honestly
- backend codegen exists
- runtime/board glue exists
- toolchain integration exists
- representative tests exist
- docs describe the real current scope

For embedded targets that also includes:

- artifact generation
- board/runtime entry handling
- flashing flow where claimed

## 20. Shared Surface Completion Criteria

A shared stdlib layer such as `gpio` or `serial` should only be called complete
for a target when:

- the full declared shared API surface works on that target
- unsupported pieces are not advertised as supported
- tests exist for the target-backed surface

If only part of the shared surface exists for a target, docs must say so.

## 21. Planned Phases

### Phase A: Foundation

- generalize omission
- harden `time`
- harden `network`
- tighten diagnostics and exit codes

### Phase B: Shared Embedded Base

- harden `gpio`
- harden `serial`
- add `pwm`
- add `adc`

### Phase C: Bus And Device Layers

- add `spi`
- add `i2c`
- refine `servo`

### Phase D: Arduino Ecosystem Expansion

- continue Arduino core coverage
- add `SoftwareSerial`
- add other high-value packaged libraries in real slices

### Phase E: ESP32 First Slice

- package toolchain/core
- back shared layers
- add tests/docs

### Phase F: Raspberry Pi First Slice

- Linux-backed GPIO/serial/time
- add tests/docs

### Phase G: Embedded Library/CFFI Expansion

- compile/link-oriented embedded FFI
- ecosystem library integration

## 22. Near-Term Concrete Work Queue

The next concrete work order after this spec is:

1. generalize current omission machinery beyond the Uno path
2. audit and tighten the current `time` module as a universal surface
3. audit and tighten the current `network` module as a universal surface with explicit per-target capability rules
4. keep strengthening `gpio` as the common embedded base
5. move more Arduino functionality under shared surfaces where honest
6. add `spi`
7. add `i2c`
8. port `SoftwareSerial` as a real Arduino library slice
9. begin the first truthful ESP32 target slice
10. begin the first truthful Raspberry Pi target slice

## 23. Documentation Rules

This spec may describe the full intended direction.

But user-facing docs must continue to separate:

- implemented
- planned
- current target scope
- future expansion

This file should be updated as work order changes, but it must never be used to
launder unimplemented work into release claims.

## 24. Explicit Non-Claims

The following are goals, not current facts:

- full Arduino core/library parity
- full ESP32 core/library parity
- full Raspberry Pi library parity
- complete embedded CFFI already existing
- every shared embedded module already being backed by all targets

These remain future work until implemented end to end.
