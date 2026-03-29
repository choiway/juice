# Pipeline Architecture

## Overview

Juice compiles JavaScript source files to Erlang BEAM bytecode. The pipeline has three stages: parse, compile, and assemble.

```
                    Juice                           Erlang
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│  JS      │    │  JS AST  │    │  Erlang  │    │  BEAM    │
│  source  │───>│  (oxc)   │───>│  source  │───>│  .beam   │
│  .js     │    │          │    │  .erl    │    │          │
└──────────┘    └──────────┘    └──────────┘    └──────────┘
   input        oxc_parser      compiler.rs        erlc
                                erlang.rs
```

## Stage 1: Parse (oxc_parser)

**File:** `src/main.rs:31-41`

The JavaScript source is parsed into a full AST using `oxc_parser`, a production-grade JS parser written in Rust. We parse as ESM (`SourceType::mjs()`).

oxc uses an arena allocator (`oxc_allocator::Allocator`) that owns all AST nodes. The parser returns a `ParserReturn` containing the `Program` AST and any errors.

**Input:** JavaScript source string
**Output:** `oxc_ast::ast::Program`

## Stage 2: Compile (JS AST to Erlang source)

**File:** `src/compiler.rs`

The compiler walks the oxc AST and emits Erlang source text. Currently it:

1. Iterates over top-level statements
2. Matches `ExpressionStatement` nodes containing `CallExpression` nodes
3. Recognizes `console.log(string)` calls by checking for a `StaticMemberExpression` where object is `console` and property is `log`
4. Maps `console.log("text")` to `io:format("text~n")`
5. Wraps the output in an Erlang module with a single exported `main/0` function

The module name is derived from the input filename (e.g., `hello.js` becomes `-module(hello).`).

**File:** `src/erlang.rs`

Helper functions for generating syntactically correct Erlang source text:

- `module_attribute(name)` — `-module(name).`
- `export_attribute(funs)` — `-export([name/arity]).`
- `function_def(name, body)` — `name() -> body.`
- `io_format(text)` — `io:format("text~n")`

**Input:** `Program` AST + module name
**Output:** Erlang source string

## Stage 3: Assemble (erlc)

**File:** `src/main.rs:56-68`

The generated Erlang source is written to a `.erl` file and compiled to BEAM bytecode by shelling out to `erlc`. This delegates optimization and bytecode emission entirely to the Erlang compiler.

**Input:** `.erl` file
**Output:** `.beam` file

## Example transformation

**Input** (`hello.js`):
```javascript
console.log("hello, world")
```

**Intermediate** (`hello.erl`):
```erlang
-module(hello).
-export([main/0]).

main() ->
    io:format("hello, world~n").
```

**Output** (`hello.beam`): BEAM bytecode, runnable with:
```
erl -noshell -s hello main -s init stop
```

## Design decisions

- **Erlang source text, not Abstract Format tuples**: v0 emits readable `.erl` files for easier debugging. The pipeline can later switch to emitting Erlang Abstract Format without changing the parsing or AST-walking stages.
- **Shell out to erlc**: Rather than linking the Erlang compiler or encoding BEAM bytecode directly, we let `erlc` handle it. This keeps the Rust side focused on the JS-to-Erlang transformation and gets BEAM optimization passes for free.
