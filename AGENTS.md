# AGENTS.md

## Purpose

This file defines the operating policies for all work in this repository.

These policies are binding for planning, design, implementation, review, and release decisions.

## Core Principle

This project is building a real language and compiler, not a prototype made of placeholders.

Everything we add to the codebase must earn its place by being real, functional, and internally honest.

## Development Policy

Development may happen in stages over many days.

That is allowed.

What is not allowed:

- fake progress
- hollow architecture
- placeholder implementations
- advertising unfinished features as finished

We may build incrementally, but every implemented part must be real.

## No-Scaffolds Policy

Scaffolding is forbidden when it creates the appearance of progress without delivering working behavior.

Do not add:

- empty modules
- empty crates
- placeholder files
- stub compiler passes
- fake AST/HIR/MIR layers with no real role
- unimplemented APIs added "for later"
- dummy runtime components
- syntax accepted by the parser but unsupported semantically
- semantic features accepted by analysis but unsupported in codegen
- codegen hooks that do nothing

Allowed:

- design documents
- specifications
- implementation plans
- completed vertical slices

Rule:

If a component is added to the codebase, it must do real work now.

If it is not ready to do real work, it stays out of the codebase and remains only in docs or planning.

## No-Placeholder Policy

Placeholders are forbidden in production code and in active implementation paths.

Do not use:

- `todo!()`
- `unimplemented!()`
- panic-based fake behavior standing in for real logic
- hardcoded temporary return values pretending to be real results
- mock language behavior inside the real compiler pipeline
- "accept now, implement later" feature branches in the language

Exception:

Temporary local experimentation that is not committed and not treated as project progress may exist during exploration, but it must not be presented as implemented work.

## Honesty Policy

A feature exists only when it is implemented end-to-end for its declared scope.

A feature is not considered implemented merely because:

- the syntax parses
- the AST node exists
- the type checker mentions it
- the backend has a named file for it
- a document says it is planned

If any required stage is missing, the feature is not complete.

## Definition of Implemented

A language feature counts as implemented only when all applicable parts are complete:

- syntax and parsing
- AST or equivalent representation
- name resolution
- semantic analysis
- type checking
- ownership and safety rules where applicable
- diagnostics
- code generation
- runtime behavior if required
- tests

If a feature touches ABI, layout, destruction, borrowing, dynamic dispatch, or FFI, those parts must also be finished before the feature can be called implemented.

## Backend Parity Policy

Backend-specific "not supported" gaps are not an acceptable resting state for in-scope language features.

Rules:

- if a feature is part of the current language or stdlib scope, it must compile through every backend we advertise for that scope
- we must not rely on parser/semantic acceptance followed by backend rejection as the normal feature boundary
- error messages like "X is not supported by the current backend" are only acceptable while a feature is still outside the declared scope
- once a feature is documented or presented as part of the current Rune surface, backend implementation becomes mandatory
- if backend parity is not ready, the feature must be removed from the claimed scope or finished properly

The intended outcome is simple:

- supported features compile
- unsupported features stay out of scope
- we do not ship language/library surfaces that die late in codegen

## Release Completeness Policy

The language may be developed gradually, but any release must be complete for its declared scope.

This means:

- no half-finished released features
- no partially supported syntax in a release
- no release notes claiming support beyond what actually works
- no silent gaps between parser, semantics, and backend

Every release must define a scope.

Everything inside that scope must be complete.

Everything outside that scope must be explicitly excluded.

## Definition of Release-Complete

A release is complete only when every feature promised in that release:

- works end-to-end
- has coherent diagnostics
- behaves consistently with the language rules
- passes tests appropriate to its risk and complexity
- is documented accurately enough to use

If a feature is unstable, experimental, partial, or missing key safety rules, it must not be part of the release scope.

## Branch Discipline Policy

Normal feature development must happen on `main`.

Rules:

- ongoing implementation work is committed to `main`
- `release` is not the default development branch
- `release` should only receive changes when a feature batch is actually ready for release
- the normal path is `main` -> review/audit -> PR/merge into `release`
- do not continue long-running feature work directly on `release`
- if the repository is currently on `release` during ordinary development, move back to `main` before continuing unless there is a very specific release task in progress

The intended outcome is:

- `main` stays the truthful active integration branch
- `release` stays narrow and intentional
- branch usage does not blur the line between development state and release state

## Scope Discipline Policy

We design broadly, but we release narrowly and honestly.

Rules:

- the long-term language vision may be larger than the next release
- the implementation may progress feature by feature
- the release scope must be explicit
- scope may not be inflated to create the appearance of momentum

The correct pattern is:

- full vision in docs
- strict release boundaries in planning
- complete implementation for anything promised

## Vertical Slice Policy

Implementation should proceed through finished vertical slices rather than fake horizontal completion.

Preferred pattern:

1. choose a real language capability
2. implement it fully through the compiler pipeline
3. test it
4. only then move to the next capability

Avoid:

- building many disconnected layers that look complete but cannot compile real programs
- adding representation layers before they are needed
- creating backend abstractions before concrete codegen exists

## Architecture Discipline Policy

Architecture is allowed only when it is justified by current needs or by a very near-term implementation requirement.

Do not create files, modules, crates, interfaces, or abstractions solely because a mature compiler might eventually need them.

Architecture must follow implementation truth.

It must not run ahead of the actual compiler.

## Stdlib Architecture Policy

The standard library is part of the real language implementation.

It must be built with the same honesty and completeness requirements as the parser, semantics, code generation, and runtime.

Rules:

- built-in stdlibs must be implemented as real Rust-side modules, runtime bindings, compiler bindings, or equivalent native compiler structures
- once a stdlib is claimed to be built-in, it must not be implemented as an embedded `.rn` source blob hidden inside Rust code
- once a stdlib is claimed to be part of the built-in module registry, its exported surface must be defined natively in the compiler/runtime implementation
- disk-loaded `.rn` stdlib files are allowed only for modules that are still explicitly outside the built-in scope
- wrapper-only stdlib work does not count as progress if the underlying runtime/compiler capability is missing
- stdlib APIs must be wired end-to-end through parsing, semantics, type checking, IR, codegen, runtime, diagnostics, and tests for every backend in the claimed scope

The intended outcome is:

- built-in stdlibs are real parts of the compiler/runtime
- stdlib loading architecture is honest
- we do not fake native stdlib implementation by smuggling Rune source strings through the loader

## Stdlib Promotion Policy

A stdlib module may be promoted from a source module to a built-in module only when the promotion is complete for the claimed scope.

Promotion requires:

- the module surface is defined natively in Rust/compiler structures
- the module behavior is backed by real runtime/compiler functionality
- backend parity is satisfied for the advertised scope
- tests cover the promoted behavior
- docs state the real implemented scope

Do not promote a stdlib module halfway.

Do not leave it in a mixed state where the repository presentation implies a native built-in module while the implementation still depends on hidden embedded source text.

If promotion is not complete, keep the module in its honest current form until the full built-in version is ready.

## Runtime Language Policy

Core runtime and stdlib behavior must not depend on ad hoc secondary language hosts when the feature is claimed as part of Rune's real implementation.

Rules:

- do not implement core stdlib modules such as `network`, `fs`, `terminal`, `env`, `sys`, `time`, `gpio`, or `serial` by delegating their real behavior to Node.js
- do not use JavaScript or Node-based fallback hosts to claim platform support for a feature that is supposed to be implemented natively in Rune's compiler/runtime
- when a platform feature is implemented, it must be implemented through Rust/runtime/compiler code or through honest target-native libraries and toolchains
- if a target cannot support a feature honestly yet, keep it out of the claimed scope instead of routing it through a convenient foreign host

The intended outcome is:

- Rune stdlib/runtime support stays native and honest
- target claims are based on real implementation, not host-language substitution

## Feature Admission Policy

A feature may enter active implementation only when:

- its semantics are defined clearly enough to finish
- its interaction with ownership, typing, and codegen is understood
- it can be tested meaningfully

If the semantics are still vague, the feature remains in design, not code.

## Parser Policy

The parser must not accept language constructs that the compiler cannot correctly validate and compile for the current declared scope.

No "future syntax" in the real compiler.

If syntax is accepted, it must correspond to a real supported feature.

## Safety Policy

Safety claims must be earned, not implied.

Do not call a feature memory-safe unless its ownership, borrowing, aliasing, destruction, and failure behavior are actually enforced.

Unsafe escape hatches must be explicit.

If a safety rule is not implemented, we must say so plainly and keep the feature out of any safe release claim.

## Testing Policy

Tests are part of completion, not an optional cleanup step.

Every implemented feature should have appropriate tests, including where relevant:

- valid examples
- invalid examples
- diagnostic expectations
- runtime behavior
- codegen-sensitive cases

No feature is complete just because it worked once manually.

## Documentation Policy

Docs may describe:

- long-term vision
- future features
- design alternatives
- deferred ideas

But docs must clearly separate:

- implemented
- planned
- rejected
- unresolved

Documentation must never blur the line between current reality and future intent.

## Refactoring Policy

Refactoring is allowed when it improves correctness, clarity, or maintainability.

But refactoring must not introduce abstract placeholders or speculative architecture.

A refactor must leave the codebase more truthful, not more ceremonial.

## Completion Policy

When we start implementing a feature, the goal is to finish it for the declared scope before moving on.

Do not leave trails of partial implementation across the codebase.

If a feature cannot be completed yet, stop, move it back into design, and remove any misleading partial implementation rather than preserving dead scaffolding.

## Collaboration Policy

When discussing plans, we should distinguish clearly between:

- complete design vision
- current implementation state
- release scope
- deferred work

This project should feel ambitious, but never dishonest.

## Enforcement Rule

When deciding whether something belongs in the repository, ask:

1. Does it do real work now?
2. Is it complete for the scope we are claiming?
3. Would a new contributor mistake this for a finished feature when it is not?

If the answer to question 1 or 2 is no, it should not be merged.

If the answer to question 3 is yes, it must be redesigned, completed, or removed.
