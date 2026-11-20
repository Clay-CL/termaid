# termaid

Render [mermaid](https://mermaid.js.org) diagrams as Unicode (or ASCII) text in
the terminal. Single static binary, no dependencies, about 1 ms per call.

```
$ printf 'graph LR\n  A[client] -->|request| B[server]\n' | termaid
┌────────┐             ┌────────┐
│ client ├───request──►│ server │
└────────┘             └────────┘
```

## Install

```sh
cargo build --release          # binary at target/release/termaid
cargo install --path .         # or install to ~/.cargo/bin
```

## Usage

```
termaid [FILE]             mermaid from FILE or stdin -> text diagram
termaid --md [FILE]        markdown in; each ```mermaid fence is replaced by
                           its rendering, everything else passes through
termaid --ascii            plain +-|<>^v characters instead of box drawing
termaid --max-width N      if wider than N columns, re-lay LR/RL graphs
                           top-down; warn on stderr if still too wide
```

- Run with no FILE at a terminal (nothing piped in), it prints the help to
  stderr and exits 2.
- Raw mode also accepts input wrapped in a ```` ```mermaid ```` fence.
- Errors go to stderr with exit code 1. In `--md` mode a block that fails to
  render is left unchanged and a warning is printed.

## Supported diagrams

| Type | Notes |
|---|---|
| `flowchart` / `graph` | TD/TB/BT/LR/RL, subgraphs (nested), edge labels, `-->` `---` `-.->` `==>` `--o` `--x` `<-->` `~~~`, `&` chains |
| `sequenceDiagram` | participant/actor aliases, all arrow types, notes, loop/alt/opt/par/critical/break, autonumber |
| `stateDiagram(-v2)` | `[*]`, composite states, fork/join/choice, descriptions, notes |
| `classDiagram` | members, annotations, generics, all relation types, cardinality, namespaces, notes |
| `erDiagram` | crow's-foot relations (cardinality shown in the label), attribute tables |

Styling lines (`classDef`, `style`, `linkStyle`, `click`, ...) are skipped.
See `important_notes.md` for the exact rendering choices and limitations.

## Using it from Claude Code

Add to `~/.claude/CLAUDE.md`:

```
When showing a mermaid diagram in chat, run `termaid` on it and paste the
output in a plain code block, followed by the mermaid block.
```
