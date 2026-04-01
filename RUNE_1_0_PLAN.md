# Rune 1.0 Master Plan

## 1. Purpose

This document defines the complete promised scope of Rune 1.0.

Rune 1.0 is not a toy release, not a bootstrap release, and not a partial language preview.

Rune 1.0 is the first complete public release of the language for its declared scope.

This plan is governed by the repository rules in `AGENTS.md`.

## 2. Rune 1.0 Release Contract

Rune 1.0 must be complete for everything declared in this document.

Nothing listed here may ship partially.

A feature is not part of Rune 1.0 unless it is complete across:

- syntax
- parsing
- semantic analysis
- type checking
- ownership and safety rules where applicable
- diagnostics
- code generation
- runtime behavior
- testing
- documentation

If any of those are missing for a promised feature, Rune 1.0 is not ready.

## 3. Rune Identity

Rune is a native compiled systems language with:

- Python-inspired indentation-sensitive syntax
- static typing
- memory safety without garbage collection
- explicit ownership and borrowing
- modern OOP through structs, methods, and traits/interfaces
- async and networking in the standard release
- native executable output

Rune should feel:

- readable like Python
- safe like Rust
- explicit like a systems language
- practical like a full modern language

Rune should not feel:

- dynamic like Python
- inheritance-heavy like old-school OOP languages
- runtime-heavy like managed VM languages
- low-level to the point of being hostile

## 4. Rune 1.0 Scope

Rune 1.0 includes:

- the Rune language core
- the Rune compiler
- native executable generation
- the Rune standard library
- async programming support
- networking support
- HTTP support
- WebSocket support
- testable and documented user-facing behavior

Rune 1.0 excludes:

- package registry service
- self-hosting compiler
- macro metaprogramming
- JIT
- GC runtime
- multiple production-quality backends beyond the primary 1.0 target

## 5. Source Form and Tooling Identity

Language name:

- `Rune`

Source file extension:

- `.rn`

Primary command-line tool:

- `rune`

Expected command structure:

- `rune build`
- `rune run`
- `rune check`
- `rune test`
- `rune fmt`
- `rune doc` later if included in 1.0 scope

## 6. Syntax Direction

Rune uses a Python-inspired surface syntax.

That means:

- significant indentation
- no braces for block structure
- readable, low-noise declarations
- `def` and `async def`
- explicit blocks through indentation and `:`

That does not mean:

- dynamic typing
- Python object semantics
- Python runtime behavior
- Python's memory model

### 6.1 Block Syntax

Rune blocks are introduced with `:` and defined by indentation.

Examples:

```text
if cond:
    do_work()
else:
    do_other_work()
```

```text
def add(a: i64, b: i64) -> i64:
    return a + b
```

### 6.2 Variable Declarations

Preferred direction:

```text
let x: i64 = 42
let name: String = "Rune"
let total = add(10, 20)
```

Rules:

- `let` introduces bindings
- explicit type annotations are supported
- local inference is allowed where the inferred type is unambiguous
- mutability syntax is to be finalized separately, but must remain explicit

### 6.3 Functions

Rune uses `def` and `async def`.

Examples:

```text
def add(a: i64, b: i64) -> i64:
    return a + b
```

```text
async def fetch() -> String raises HttpError:
    let response = await http.get("https://example.com")
    return await response.text()
```

### 6.4 Control Flow

Core control flow in Rune 1.0:

- `if`
- `elif`
- `else`
- `while`
- `for` if finalized for 1.0
- `return`
- `break`
- `continue`

Example:

```text
if x > 0:
    println("positive")
elif x < 0:
    println("negative")
else:
    println("zero")
```

### 6.5 Type and Object Syntax

Proposed direction:

```text
struct Vec2:
    x: f64
    y: f64

impl Vec2:
    def length(self: &Vec2) -> f64:
        return sqrt(self.x * self.x + self.y * self.y)
```

Construction direction:

```text
let v = Vec2(x=3.0, y=4.0)
```

### 6.6 Traits or Interfaces

Rune 1.0 should include behavior abstraction through either:

- `trait`

or:

- `interface`

The keyword must be selected before implementation work begins in that area.

Current recommendation:

- use `trait`

Reason:

- closer to systems-language semantics
- less tied to classical OOP inheritance expectations

### 6.7 Imports and Modules

Module syntax must be simple and explicit.

Example direction:

```text
import net.tcp
import http
from core.result import Result
```

This is not finalized yet, but Rune 1.0 must include a complete module/import system.

## 7. Type System Scope for Rune 1.0

Rune 1.0 includes:

- `bool`
- signed integers
- unsigned integers
- floating-point types
- `char`
- string slices or string views
- owned strings
- arrays
- slices
- structs
- enums
- references
- optional values
- result values

Rune 1.0 may include generics if and only if they are fully complete for the promised 1.0 surface.

If generics are included in Rune 1.0, they must not be partial.

## 8. Ownership and Safety Model

Rune 1.0 promises memory safety without garbage collection.

That requires:

- ownership by default
- move semantics
- borrowing
- immutable aliasing rules
- mutable exclusivity rules
- deterministic destruction
- explicit unsafe escape hatches

Rune 1.0 must not claim memory safety unless these rules are actually enforced coherently.

### 8.1 Safe and Unsafe

Rune 1.0 includes:

- safe code by default
- `unsafe` blocks for raw memory operations and low-level escape hatches

Unsafe must be explicit and narrow.

## 9. OOP Model

Rune 1.0 includes OOP, but not inheritance-centric OOP.

The 1.0 OOP model should be:

- `struct` for data
- `impl` for methods
- `trait` for shared behavior contracts
- composition-first design
- static dispatch by default
- dynamic dispatch only if fully designed and implemented

Rune should support rich object modeling without reproducing the worst complexity of classical inheritance systems.

## 10. Error Model

Rune 1.0 includes all of the following:

- `Result[T, E]`
- `raise`
- `raises`
- `try`
- `except`
- `panic`

These must coexist coherently.

### 10.1 Semantic Roles

Use:

- `Result[T, E]` for explicit typed error values
- `raise` for recoverable typed error flow
- `panic` for unrecoverable faults

### 10.2 Error Declaration

Functions that may raise recoverable errors must declare them.

Example direction:

```text
def divide(a: i64, b: i64) -> i64 raises CalcError:
    if b == 0:
        raise CalcError("division by zero")
    return a / b
```

### 10.3 Result Example

```text
def divide_checked(a: i64, b: i64) -> Result[i64, CalcError]:
    if b == 0:
        return Err(CalcError("division by zero"))
    return Ok(a / b)
```

### 10.4 Panic Example

```text
def divide_or_panic(a: i64, b: i64) -> i64:
    if b == 0:
        panic("division by zero")
    return a / b
```

### 10.5 Async Interaction

`async def` must support:

- `await`
- `Result`
- `raise`
- `raises`
- `panic`

These interactions must be specified before implementation begins.

## 11. Async Model

Rune 1.0 includes async support.

That means Rune 1.0 must define:

- async functions
- await points
- task spawning model
- cancellation strategy
- error propagation in async code
- runtime scheduling model

Example direction:

```text
async def main() -> i32 raises IoError:
    println("Enter your name:")
    let name = await input()
    println("Hello, ", name)
    return 0
```

Rune 1.0 must not ship "async syntax only."

Async must be real end-to-end.

## 12. Standard Library Architecture

Rune 1.0 standard library is part of the promised release scope.

It should be organized by responsibility.

Recommended top-level layout:

- `core`
- `alloc`
- `std`
- `async`
- `net`
- `http`
- `ws`

### 12.1 `core`

No hidden heap dependence.

Contains:

- primitive support
- `Option`
- `Result`
- panic/assert support
- traits used pervasively by the language
- slices and views where possible
- comparison and ordering support
- numeric helpers where appropriate

### 12.2 `alloc`

Heap-backed foundational types.

Contains:

- `String`
- `Vec`
- allocation interfaces
- owned dynamic containers required by higher layers

### 12.3 `std`

General operating-system-facing standard library.

Contains:

- console IO
- file IO
- paths
- process
- environment
- time
- filesystem abstractions

Rune 1.0 should include `print`, `println`, `panic`, and input support in this general layer, or expose them through language prelude decisions backed by `std`.

### 12.4 `async`

Contains:

- async runtime interfaces
- task primitives
- timers
- synchronization primitives
- channels if included

This module must be sufficient to support networked async applications in Rune 1.0.

### 12.5 `net`

Contains:

- IP address types
- socket addresses
- TCP client and server support
- UDP sockets
- connection abstractions

Rune 1.0 networking must include:

- TCP
- UDP

### 12.6 `http`

Contains:

- HTTP request types
- HTTP response types
- headers
- status codes
- async client
- async server if included in 1.0 scope

Rune 1.0 must define whether server support is mandatory in 1.0.

Current recommendation:

- include both HTTP client and server in 1.0

### 12.7 `ws`

Contains:

- WebSocket client
- WebSocket server if included in 1.0 scope
- frame/message abstractions
- upgrade handling

Current recommendation:

- include both WebSocket client and server in 1.0

## 13. Built-ins and Prelude-Level Facilities

Rune should provide a clean everyday experience.

Candidate built-ins or prelude-level facilities:

- `print`
- `println`
- `panic`
- `input`

The exact split between language built-ins and prelude imports must be decided early because it affects compiler and stdlib boundaries.

Current recommendation:

- treat them as prelude-exposed standard facilities backed by libraries, not magical ad hoc compiler intrinsics except where necessary for bootstrap/runtime entry behavior

## 14. Native Backend Scope

Rune 1.0 is a native compiled language.

Rune 1.0 includes:

- native executable generation
- assembly emission as the first backend path
- assembly to object file assembly step
- linking into a final executable

Current target recommendation:

- one primary x86-64 Windows target first

If Rune 1.0 claims multi-platform support, each claimed platform must be complete.

## 15. Testing Contract for Rune 1.0

Rune 1.0 requires:

- syntax tests
- indentation/offside-rule tests
- semantic tests
- ownership tests
- async behavior tests
- networking tests
- HTTP tests
- WebSocket tests
- compile-and-run integration tests
- diagnostics tests

No subsystem listed in Rune 1.0 scope is complete without real tests.

## 16. Documentation Contract for Rune 1.0

Rune 1.0 requires user-facing documentation for:

- language syntax
- type system
- ownership and borrowing
- error model
- async model
- module/import system
- standard library overview
- networking APIs
- HTTP APIs
- WebSocket APIs

## 17. Major Open Design Decisions

These must be decided before implementation planning becomes final:

1. final mutability syntax
2. final trait vs interface keyword
3. whether generics are in 1.0
4. whether `for` is in 1.0 and what iterable model it uses
5. whether HTTP server support is mandatory in 1.0
6. whether WebSocket server support is mandatory in 1.0
7. how exceptions and `Result` interoperate in detail
8. what async runtime model is used
9. what module/import syntax is final
10. what the prelude contains

## 18. Implementation Strategy Under Repository Policy

Rune 1.0 may be developed in stages.

That is allowed.

But every stage must produce real completed work, not placeholders.

Therefore the strategy is:

- fully design the 1.0 release scope
- fully specify subsystem behavior before implementation enters risky areas
- implement through completed vertical slices
- never claim a 1.0 feature exists until it is truly complete

This plan must obey `AGENTS.md`.

## 19. Recommended Planning Order

Before code implementation begins in earnest, refine in this order:

1. syntax
2. mutability and ownership surface syntax
3. error-handling model
4. async model
5. stdlib architecture
6. networking API design
7. HTTP API design
8. WebSocket API design
9. module and import system
10. 1.0 inclusion decisions for borderline features

## 20. Immediate Next Discussion Topics

The next discussions should focus on:

- concrete Rune syntax examples
- print, println, input, panic, raise behavior
- stdlib layering and naming
- TCP/UDP API shape
- HTTP API shape
- WebSocket API shape

## 21. Summary

Rune 1.0 is defined here as a complete release with:

- Python-inspired syntax
- native compilation
- memory safety without GC
- OOP
- async
- `Result` and `raise`
- standard library support
- networking support including TCP, UDP, HTTP, and WebSocket

The project may take time to build, but the 1.0 release must be complete for this promised scope.
