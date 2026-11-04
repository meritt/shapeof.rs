# Architecture

## Scope

shapeof infers a JSON Schema 2020-12 from examples: one JSON document, loose JSON as it is copied from code, or JSON Lines. `infer` returns the schema as indented text with notes on what it had to generalize, or a refusal. The schema targets OpenAI or Anthropic structured outputs, or plain validation. Output is deterministic and pinned by `testdata/cases.json`.

## Components

```text
                     shapeof-core        pure functions; no I/O, no unsafe
                          │              ryu-js for numbers, unicode-ident for names
                     shapeof-ffi         C ABI; the only unsafe code
                ┌─────────┴──────────┐
    shapeof.wasm + shapeof.js    ShapeOfFFI.xcframework + ShapeOf (Swift)
```

Dependencies point down only. Bindings add no policy: they marshal arguments, map status codes and decode the report.

## Pipeline

```text
text ─► read ─► shape tree ─► emit ─► schema text + notes
         │                      │
         │                      └─ per target: required, nullable, additionalProperties, formats
         └─ whole text as one document; on failure with several lines, each line as a document
```

- **Read.** Values are folded straight into a shape tree; the data itself is never built. Each place records the types seen, how many objects carried each property and the format common to its strings. Besides JSON, the reader takes what code holds: unquoted and numeric names, single quotes, comments, trailing commas, a final `;`, `undefined`, `NaN`, `Infinity` and JavaScript number literals. A numeric name becomes the string JavaScript would make of it. Only broken structure is an error; it names the line, column, line text and a reason in Russian.
- **Emit.** The text is written as `JSON.stringify(value, null, 2)` writes it, except that short lists of strings stay on one line.

| Target | Root | Required | Missing in some examples |
|---|---|---|---|
| `openai` | object; a lone array of objects is a list of examples | every property | nullable; empty arrays hold strings; no `uri` |
| `anthropic` | object; a lone array of objects is a list of examples | present in every example | optional |
| `plain` | as given, with `$schema` | present in every example | optional |

Objects of `openai` and `anthropic` have `additionalProperties: false`. A string format is emitted only when every value matches it and a format-checking validator accepts it, so a schema never rejects its own examples. `Notes` counts empty arrays, unions, optional properties, depth, properties and name length.

## Bounds

| Limit | Value | On excess |
|---|---|---|
| reader nesting | 1000 levels | unreadable, `слишком глубокая вложенность` |
| `DEPTH` | 64 levels in the schema | `depth` |

## C ABI

- Text is UTF-8 in memory from `shapeof_alloc`, released with `shapeof_dealloc`.
- `shapeof_infer` fills a `shapeof_report`: a status and, on `SHAPEOF_OK`, JSON released with `shapeof_report_free`. The JSON is the same on every platform: `{ schema, notes }`, `{ error: { reason, line, column, source } }` or `{ error: "depth" }`.
- The status reports a call error only: input that is not UTF-8 or a target index out of range. Unreadable examples come inside the report.
- Calls keep no state between them and run on any thread.
- A panic never crosses the boundary. Native builds return `SHAPEOF_INTERNAL`; in wasm32 a panic traps the instance.
- `shapeof_version` identifies the build. Bindings refuse a library whose version differs from their own.
- cbindgen generates `include/shapeof.h` from `shapeof-ffi`; Miri runs the ABI tests.

## Bindings

**WebAssembly.** `shapeof.wasm` is the `cdylib` of `shapeof-ffi` with no generated glue, optimized with `wasm-opt -O3`. `shapeof.js` is one ES module without static imports, so it runs in a page, a worker and Node; it reads a file only for a `file:` URL. A lone surrogate reads as U+FFFD, as `TextEncoder` writes it. Results are read from memory only after the call, which may have grown it.

**Swift.** `ShapeOfFFI.xcframework` holds one universal static library for `arm64` and `x86_64` with the header as module `ShapeOfFFI` under `Headers/ShapeOfFFI/`, which keeps its module map distinct from other Rust libraries in the same app. `Package.swift` uses a local build when present, otherwise the release binary pinned by URL and checksum. `ShapeOf` decodes the same JSON and throws a typed `ShapeOfError`.

## Conformance

`testdata/cases.json` lists examples with the exact schema text and notes, or refusal, each must produce: every target, optional and nullable properties, unions, empty arrays, list roots, loose JSON, string formats, a scalar root and every refusal. The Rust, WebAssembly and Swift test suites all replay it.

## Distribution

| Artifact | Consumer |
|---|---|
| `shapeof-core` crate | crates.io |
| `shapeof-vX.Y.Z-npm.tgz`, package `@meritt/shapeof-wasm` | npmjs.com |
| `shapeof-vX.Y.Z-xcframework.zip` | SwiftPM, through `Package.swift`, macOS 26 |

The release set is reproducible: the tarball and the zip have fixed entry order, times and modes. The checksum pinned in `Package.swift` before tagging is therefore the checksum of the tagged build.
