# cviz

A CLI tool to visualize WebAssembly component composition structure.

cviz parses composed WebAssembly components and generates diagrams showing how component instances are wired together. It's particularly useful for understanding middleware chains in WASI HTTP components.

Curious what this tool does? Clone the repo and run the demo! `cargo run --example demo`

## Installation

```bash
# From source
cargo install --path .

# Or from git
cargo install --git https://github.com/cosmonic-labs/cviz
```

## Usage

```bash
cviz <FILE> [OPTIONS]
```

Run `cviz --help` for the full flag reference. The sections below cover
the features worth knowing about.

## Output formats

- `-f ascii` (default) — terminal-friendly box diagrams.
- `-f mermaid` — Mermaid source for docs / browsers / CI artifacts.
- `-f json` / `-f json-pretty` — the parsed composition graph as JSON.

## Detail levels

The `-l` flag selects what the renderer draws:

- `handler-chain` (default) — the request-flow chain for each exported
  handler-style interface. Compact, focused on middleware ordering.
- `all-interfaces` — every interface connection, including host imports.
- `full` — everything, including synthetic instances and component
  indices.
- `graph` — a 2D layout with boxed nodes and request-flow arrows, one
  section per top-level export. This is the most readable view for
  non-trivial compositions and the only level that supports
  highlighting.

A `graph` render of a small two-component composition (`-t false` to
suppress type symbols):

```
╞══ handler ════════════════════════════════════════

                ┌─────┐             ┌───────┐
 ext:handler ──▶│ srv │───handler──▶│ srv-b │
                └─────┘             └───────┘
```

Instances reachable from more than one export render with a double-line
border (`╔═╗`) on subsequent occurrences so shared infrastructure is
easy to spot. The renderer auto-detects terminal width and wraps wide
diagrams onto multiple bands; when the terminal is too narrow it
suggests rerunning with `-f mermaid`.

## Highlighting

The `graph` view can mark nodes and edges via `--highlight` (see
`cviz --help` for the syntax). Each highlight takes a canonical id, an
optional color, and an opaque tag string that any arbitrary tool can use
to attach reason information to the rendering. Tags surface as a
numbered list under the diagram, keyed to `[N]` brackets on the
highlighted boxes and arrows.

## How It Works

cviz uses [wasmparser](https://crates.io/crates/wasmparser) to parse the WebAssembly component model structure. It extracts:

1. **Component instances** - The instantiated components within the composition
2. **Interface connections** - How instances are wired together via their imports/exports
3. **Export chain** - What the composed component exports to the outside world

For WASI HTTP middleware compositions, it specifically traces the `wasi:http/handler` interface chain to show the request flow through middleware layers.
