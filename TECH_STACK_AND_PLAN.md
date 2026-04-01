# Native Safe OOP Language: Tech Stack and Development Plan

## 1. Project Goal

Build a compiled systems programming language with:

- Native executable output like C/C++
- Memory safety without garbage collection
- Modern OOP through composition, methods, traits, and interfaces
- Strong static typing
- Predictable performance
- Zero-cost abstractions where possible
- A path to self-hosting later

This is not a C clone. It is a new native language in the space between C++, Rust, and Swift, with a compiler that emits native machine-oriented output through assembly/object generation and linking.

## 2. Product Vision

The language should feel:

- Lower-level and predictable like C
- Safer like Rust
- More approachable than Rust for common application code
- More modern and less dangerous than C++
- Capable of systems programming and application programming
- More readable and fluid at the syntax level through a Python-inspired surface language

The first implementation should prioritize:

- Correctness over optimization
- Simple rules over maximum feature count
- Explicit ownership and lifetimes over hidden behavior
- Static dispatch first, dynamic dispatch second
- A clean architecture that can scale into a production compiler
- A Python-like syntax that stays clean without becoming dynamically typed or runtime-heavy

## 3. Recommended Implementation Language

Build the compiler in Rust.

Why Rust:

- Excellent for compiler data structures like AST, IR, and symbol tables
- Strong enums and pattern matching for parsers and semantic analysis
- Safer memory model while building a memory-safe language
- Good ecosystem for CLI tools, testing, and diagnostics
- Easier to maintain than a large compiler in C

Rejected choices:

- C: possible, but slower to develop and harder to maintain for a complex safe language
- C++: powerful, but adds avoidable complexity
- Python: useful for prototypes, but not ideal for a serious native compiler
- Zig: interesting, but adds risk and ecosystem uncertainty for the compiler implementation itself

## 4. Output Strategy

The compiler should not be designed as direct AST-to-assembly only.

The production pipeline should be:

`Source -> Lexer -> Parser -> AST -> Semantic Analysis -> Ownership/Borrow Analysis -> MIR -> Lowering -> Backend IR -> Assembly/Object -> Linker -> Executable`

Recommended executable path for v1:

- Frontend and middle-end in Rust
- Generate x86-64 assembly first
- Assemble to object files
- Link to native executables

Future path:

- Add direct object emission
- Add more backends
- Add optimization passes
- Add LLVM or custom backend only if needed later

## 5. Core Tech Stack

### 5.1 Compiler Language

- Rust stable

### 5.2 Cargo Workspace Layout

- `compiler/cli`
- `compiler/lexer`
- `compiler/parser`
- `compiler/ast`
- `compiler/hir`
- `compiler/mir`
- `compiler/semantic`
- `compiler/borrowck`
- `compiler/codegen_x64`
- `compiler/driver`
- `compiler/diagnostics`
- `compiler/span`
- `compiler/symbols`
- `compiler/formatter` later
- `compiler/lsp` later
- `runtime/core`
- `runtime/startup`
- `stdlib`
- `tests`
- `examples`
- `docs`

### 5.3 Parsing

Recommended approach:

- Hand-written lexer
- Hand-written recursive descent parser or Pratt parser for expressions
- Explicit indentation and dedentation token generation in the lexer

Why:

- Better control over diagnostics
- Easier language evolution
- Better fit for custom syntax and ownership-related grammar later
- Necessary for a Python-based surface syntax with meaningful indentation

Avoid parser generators for v1 unless they solve a specific pain point.

### 5.4 Internal Representations

Use multiple lowering stages:

- AST: direct parsed syntax tree
- HIR: desugared, name-resolved high-level representation
- MIR: typed control-flow-oriented representation for safety analysis and codegen prep
- Backend IR: x86-64-oriented lower representation with stack slots, registers, and calling convention details

This is important. A safe language with OOP and ownership is too complex for a single-tree compiler architecture.

### 5.5 Diagnostics

Use:

- `miette` or a custom diagnostic renderer
- source spans for every token and node
- structured diagnostics with:
  - error code
  - primary span
  - secondary spans
  - help text
  - note text

Compiler UX is a core feature, not a nice-to-have.

### 5.6 Target Platform for v1

Start with one target only:

- `x86_64-pc-windows-msvc` if Windows-native toolchain is preferred

Alternative:

- `x86_64-pc-windows-gnu` if using GNU assembler/linker tooling feels easier

Recommendation for early simplicity:

- Windows x86-64
- NASM or GNU assembler syntax
- Link using a system linker driver

Later targets:

- Linux x86-64
- ARM64

### 5.7 Codegen Backend

Versioned plan:

- v1 backend: emit readable x86-64 assembly
- v2 backend: emit object files directly or use a lower-level codegen library
- v3 backend: add optimizations and more targets

Recommended initial backend responsibilities:

- stack frame layout
- register assignment with a simple strategy
- function prologue/epilogue
- integer arithmetic
- branching
- function calls
- struct layout
- method calls

### 5.8 Linker Strategy

Use external system tools first.

Windows options:

- `clang` as linker driver
- `gcc` as linker driver if using MinGW
- `link.exe` later if targeting MSVC directly

Recommendation:

- Emit assembly
- Assemble using an external assembler
- Link using `clang` or `gcc`

This reduces complexity while the frontend and safety model are still evolving.

## 6. Language Runtime Strategy

This language should have a minimal runtime, not a garbage-collected VM.

Runtime components:

- startup entry glue
- panic handling strategy
- optional bounds checks and trap helpers
- optional allocation abstractions
- deterministic destruction support

Runtime goals:

- tiny
- transparent
- mostly optional
- no hidden garbage collector

## 7. Memory Safety Model

This is the most important language design decision.

Recommended model:

- Ownership by default
- Move semantics
- Borrowed references
- Mutable borrow exclusivity
- Immutable aliasing allowed
- No null references by default
- Explicit optional type for absence
- Deterministic destruction
- Escape analysis and region rules later if needed

Version 1 safety boundaries:

- Safe stack values
- Safe references
- No arbitrary pointer arithmetic in safe code
- Unsafe blocks for raw memory escape hatches

Do not attempt a fully Rust-level borrow checker in week one.

Stage the design:

1. Owned values only
2. Stack borrows
3. Mutable and immutable reference rules
4. Heap-owned values
5. Traits and generic interactions with ownership
6. Non-lexical lifetime improvements later

## 8. OOP Model

Use modern systems-language OOP, not classic inheritance-heavy OOP.

Recommended features:

- `struct` for data
- `impl` blocks for methods
- `trait` for behavior contracts
- static dispatch by default
- dynamic dispatch through trait objects later
- composition over inheritance

Avoid in v1:

- deep class inheritance
- implicit virtual dispatch everywhere
- hidden heap allocation

This language should support OOP, but in a safe and explicit way.

## 9. Type System

### 9.1 Early Types

- `bool`
- signed integers: `i8 i16 i32 i64 isize`
- unsigned integers: `u8 u16 u32 u64 usize`
- `f32 f64` later
- `char`
- `str` slice/string view
- `unit` or `void`

### 9.2 Composite Types

- arrays
- slices
- tuples later
- structs
- enums
- references
- optional type
- result type

### 9.3 Advanced Types Later

- generics
- associated types
- trait bounds
- lifetimes in surface syntax only if necessary
- const generics much later

## 10. Error Handling Model

Do not use exceptions.

Use:

- `Result<T, E>` style recoverable errors
- `panic` for unrecoverable faults
- explicit propagation operators later

This aligns with native performance and predictability.

## 11. Syntax Direction

Recommended syntax direction:

- Python-inspired surface syntax
- Significant indentation instead of braces
- Explicit type annotations where they improve clarity and safety
- Type inference for locals only where rules remain simple and unsurprising
- `struct`, `enum`, `trait`, `impl`, `fn` or equivalent keywords adapted to the final syntax style
- No dependence on Python runtime semantics, dynamic typing, or garbage collection

Example direction:

```text
struct Vec2:
    x: f64
    y: f64

impl Vec2:
    fn length(self: &Vec2) -> f64:
        return sqrt(self.x * self.x + self.y * self.y)

fn main() -> i32:
    let v: Vec2 = Vec2(x=3.0, y=4.0)
    print(v.length())
    return 0
```

The exact syntax can change later, but the direction should remain:

- indentation-sensitive
- visually lightweight
- statically typed
- explicit enough for systems programming

### 11.1 Python-Based Syntax Policy

The language should borrow syntax feel from Python, not Python's runtime model.

Keep:

- indentation-based blocks
- readable statement layout
- lightweight function and type declarations if possible
- low punctuation density where clarity is preserved

Do not copy:

- dynamic typing
- reference semantics by default
- implicit object model behavior
- garbage collection assumptions
- Python execution model

This language should read more like Python while compiling and behaving more like a native systems language.

## 12. Unsafe Story

A serious systems language needs an unsafe escape hatch.

Recommended model:

- safe by default
- `unsafe` blocks for raw pointer operations, FFI, manual allocation, and unchecked assumptions
- explicit marking of unsafe functions and traits later

This allows:

- systems programming
- OS work
- interop
- performance-sensitive code

without compromising the safe subset.

## 13. FFI Strategy

FFI is critical for ecosystem growth.

Version 1 goals:

- call C functions
- export plain C ABI functions
- support basic structs and integers

Later:

- header generation
- binding generation
- ABI verification tooling

This is one of the fastest ways to make the language practically useful.

## 14. Standard Library Strategy

Start small.

### 14.1 Core Library

- primitive types
- `Option`
- `Result`
- slices
- strings
- basic formatting later
- iterators later

### 14.2 Allocation Story

Recommended approach:

- no mandatory GC
- explicit allocator interfaces later
- default global allocator available
- ownership-based containers

### 14.3 Collections Later

- `Vec`
- `String`
- hash map later

## 15. Build Tools

Compiler executable:

- `kurox` for the compiler driver

Subcommands:

- `kurox build`
- `kurox run`
- `kurox check`
- `kurox fmt` later
- `kurox test` later

Package manager:

- Do not build one immediately
- Start with single-package builds
- Add package/workspace tooling only after the compiler core is stable

## 16. IDE and Tooling Roadmap

After the compiler stabilizes, add:

- formatter
- language server
- syntax highlighting
- doc generator
- test runner integration

Do not start here. These are later-force multipliers, not day-one priorities.

## 17. Testing Strategy

Testing must be built into the project from the beginning.

### 17.1 Compiler Tests

- lexer golden tests
- parser golden tests
- semantic analysis tests
- borrow checker rule tests
- codegen snapshot tests
- indentation tokenization tests
- block structure and offside-rule diagnostics tests

### 17.2 Integration Tests

- compile valid programs
- ensure invalid programs fail with expected diagnostics
- run produced binaries and verify output

### 17.3 Conformance Tests

Build a curated language test suite organized by feature area.

## 18. Documentation Strategy

Write docs in parallel with implementation.

Recommended docs:

- language overview
- syntax reference
- type system rules
- ownership and borrowing guide
- unsafe guide
- FFI guide
- backend and ABI notes
- compiler architecture notes

## 19. Build and Dev Tooling

Recommended stack:

- Rust stable toolchain
- `cargo`
- `clippy`
- `rustfmt`
- snapshot testing crate for compiler outputs
- CI through GitHub Actions later

External native tools:

- assembler: NASM or GNU assembler
- linker driver: clang or gcc

Optional later:

- disassembler for debugging backend output
- perf tooling

## 20. Recommended Initial Repository Layout

```text
KUROX/
  compiler/
    ast/
    lexer/
    parser/
    hir/
    mir/
    semantic/
    borrowck/
    diagnostics/
    span/
    symbols/
    codegen_x64/
    driver/
    cli/
  runtime/
    core/
    startup/
  stdlib/
  tests/
    lexer/
    parser/
    semantic/
    borrowck/
    codegen/
    integration/
  examples/
  docs/
  TECH_STACK_AND_PLAN.md
```

## 21. Architecture Decision Summary

These are the recommended core decisions for the first serious version.

- Compiler implementation language: Rust
- Compilation model: multi-stage compiler, not direct AST-to-ASM only
- Surface syntax: Python-inspired and indentation-sensitive
- Safety model: ownership, borrowing, moves, deterministic destruction
- OOP model: structs, impls, traits, composition-first
- Runtime: minimal, no GC
- Backend: x86-64 assembly first
- Linking: external linker driver
- Error handling: Result + panic, no exceptions
- Package tooling: later
- Self-hosting: long-term goal, not a phase-1 requirement

## 22. Development Phases

### Phase 0: Language Design Foundation

Deliverables:

- language philosophy document
- syntax sketch
- indentation and offside-rule definition
- type system sketch
- ownership model sketch
- safety boundary definition
- v1 feature cut

Exit criteria:

- stable feature boundaries
- no contradictions in safety and OOP goals

### Phase 1: Bootstrap Compiler Skeleton

Deliverables:

- Cargo workspace
- CLI driver
- source file loader
- diagnostics framework
- spans and symbol interners

Exit criteria:

- can parse files and print structured diagnostics

### Phase 2: Lexer and Parser

Deliverables:

- token model
- lexer
- indent/dedent handling
- parser
- AST
- parser tests

Exit criteria:

- full parsing of a minimal language subset

### Phase 3: Semantic Analysis

Deliverables:

- name resolution
- type checking
- scope handling
- function validation
- struct and method validation

Exit criteria:

- semantic errors are detected and reported cleanly

### Phase 4: HIR and MIR

Deliverables:

- lowered representations
- explicit control flow
- type-annotated MIR

Exit criteria:

- safe foundation for borrow checking and codegen

### Phase 5: Ownership and Borrow Checking

Deliverables:

- move tracking
- borrow tracking
- mutability rules
- destruction points

Exit criteria:

- invalid aliasing and use-after-move cases rejected

### Phase 6: Native Backend

Deliverables:

- stack layout
- function calls
- arithmetic and branching
- local variables
- return values
- basic struct support
- assembly emission

Exit criteria:

- compile small programs into working native executables

### Phase 7: Runtime and Standard Library

Deliverables:

- startup glue
- core types
- Option and Result
- minimal string and slice support

Exit criteria:

- useful small programs can be written

### Phase 8: OOP Expansion

Deliverables:

- impl methods
- traits
- trait bounds later
- dynamic dispatch later

Exit criteria:

- object-oriented modeling feels natural without inheritance abuse

### Phase 9: Practical Language Features

Deliverables:

- heap allocation
- arrays and slices
- modules
- imports
- FFI

Exit criteria:

- real application code becomes possible

### Phase 10: Optimization and Tooling

Deliverables:

- optimization passes
- formatter
- language server
- test command

Exit criteria:

- language becomes pleasant to use, not just technically functional

## 23. Milestone Plan

### Milestone A: Native Hello World

Support:

- `fn`
- integer literals
- return
- external print or OS output helper

Goal:

- compile a trivial program to a working executable

### Milestone B: Tiny Safe Core

Support:

- locals
- arithmetic
- `if`
- `while`
- blocks
- typed functions

Goal:

- basic procedural programs compile and run

### Milestone C: Structured Types

Support:

- structs
- methods
- stack ownership

Goal:

- first OOP-feeling programs work

### Milestone D: Ownership Rules

Support:

- moves
- borrows
- mutability rules

Goal:

- first real safety story exists

### Milestone E: Standard Library Seed

Support:

- Option
- Result
- strings or slices
- basic collections

Goal:

- language becomes usable beyond demos

## 24. Risks and Hard Problems

These are the hardest parts:

- designing a safe but understandable ownership model
- balancing OOP ergonomics with zero-cost abstractions
- codegen correctness and ABI details
- borrow checker complexity
- trait system complexity
- making Python-like syntax work cleanly for a statically typed native systems language
- designing indentation rules that stay ergonomic, unambiguous, and compiler-friendly

Big warning:

Do not try to finalize generics, trait objects, advanced borrow checking, package management, and full stdlib before the core ownership and codegen pipeline works.

## 25. Suggested v1 Feature Cut

The first serious version should include:

- functions
- locals
- primitive types
- structs
- methods
- `if`, `while`, `return`
- indentation-sensitive syntax
- stack ownership
- immutable and mutable borrows
- deterministic destruction
- minimal diagnostics
- x86-64 native executable output

The first serious version should exclude:

- advanced generics
- macros
- inheritance hierarchy features
- async
- package registry
- full optimization pipeline
- full self-hosting

## 26. Long-Term Vision

Later versions can add:

- generics
- trait objects
- optimizer
- multiple targets
- package manager
- language server
- self-hosting compiler

Self-hosting should be treated as a prestige milestone, not an early requirement.

## 27. Immediate Next Planning Tasks

Before coding the compiler, we should refine these in order:

1. Name and brand direction for the language
2. Core language philosophy
3. Python-based syntax draft
4. Indentation and offside-rule specification
5. Exact v1 feature cut
6. Ownership model draft
7. OOP model draft
8. Backend target and toolchain choice

## 28. Final Recommendation

The best complete stack for this project is:

- Compiler written in Rust
- Hand-written lexer and parser with explicit indent/dedent tokenization
- AST -> HIR -> MIR -> x86-64 backend pipeline
- Python-inspired indentation-sensitive syntax
- Ownership and borrowing for memory safety
- Structs, impls, and traits for OOP
- Minimal runtime with no GC
- Assembly output first, object generation later
- External assembler and linker during bootstrap
- Strong diagnostics and testing from day one

This stack gives the project the best balance of ambition, realism, extensibility, and native performance.
