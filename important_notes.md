# termaid: implementation notes

## Layout (flowchart, state, class, ER)

`src/graph_layout.rs` is a layered (Sugiyama-style) layout, the same family as
Graphviz `dot` / dagre:

1. Self-loops are taken out and drawn beside their node.
2. Cycles are broken by DFS from sources in declaration order; back edges are
   reversed for layout and drawn with the arrow at the other end.
3. Layers: longest path, then sources are pulled down next to their targets.
   `--->` (extra dashes) adds minimum length.
4. Edges spanning several layers get one-cell "dummy" items.
5. Order within layers: barycenter sweeps; clusters (subgraphs) are kept
   contiguous by sorting cluster groups recursively.
6. Cross-axis positions: isotonic regression (pool adjacent violators) toward
   neighbour centres, then a push pass so cluster rectangles do not overlap
   outsiders or sibling clusters (capped at 400 pushes).
7. Ports are spread along each box side; boxes grow so every edge gets its own
   port. Jogs between layers get "tracks"; a segment leaving cross position p
   must turn before any segment arriving at p (else their runs overlap).
8. Everything is computed in (cross, main) coordinates; TD/BT/LR/RL only change
   the final mapping to screen (BT/RL mirror the main axis).

Line junctions merge automatically on the canvas (`├ ┬ ┼ ...`): cells store
direction bitmasks, not characters.

## Rendering choices (agreed with the user)

- Shapes: `[x]` box, `(x)` rounded corners, `{x}` box with `◆` prefix,
  `((x))` double border. Everything else is a plain box.
- Edges: `-->` light, `-.->` dotted `┄┆`, `==>` thick `━┃`.
- ER cardinality is folded into the label: `places [1 : 0..N]`.
- State: start `●`, end `◉` (ASCII `*` / `@`), fork/join `━━━━━━━`, choice `◆` box.
- Sequence: participant boxes repeated at the bottom, like mermaid.
- `--md`: rendered art replaces the fence directly (no surrounding fence).
- Raw mode strips a surrounding ```` ```mermaid ```` fence quietly.

Smaller choices made without asking:
- Sequence `actor` uses rounded corners; `-)` async uses `▷`; autonumber
  prefixes `1. `; class cardinality is folded into the label like ER.
- Notes (state/class) become note boxes joined by a dotted line.
- An edge to a subgraph/composite id attaches to its first node (for state
  composites: the inner start dot when entering, end dot or last state when leaving).

## Limitations

- Dense graphs with many edges crossing between subgraphs can get very wide and
  cluster boxes may overlap once the push cap is hit.
- Edge labels are never wrapped; long labels widen LR gaps.
- Character widths use a built-in table (CJK and common emoji are 2 cells).
- Unsupported: gantt, pie, journey, gitGraph, mindmap, timeline, etc.
