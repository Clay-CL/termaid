//! Shared layered ("Sugiyama-style") layout for box-and-arrow diagrams.
//! Layout runs in abstract (cross, main) coordinates; TD/BT/LR/RL only change
//! the final mapping to screen (x, y). See important_notes.md for details.

use crate::canvas::{self, Canvas, Dir, OWNER_EDGE0, OWNER_SOLID, Style, str_width};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    TD,
    BT,
    LR,
    RL,
}

impl Direction {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "TD" | "TB" => Some(Direction::TD),
            "BT" => Some(Direction::BT),
            "LR" => Some(Direction::LR),
            "RL" => Some(Direction::RL),
            _ => None,
        }
    }
    pub fn horizontal(self) -> bool {
        matches!(self, Direction::LR | Direction::RL)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Rect,
    Round,
    Diamond,
    Circle,
    /// No border: start/end states, fork bars.
    Bare,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Marker {
    None,
    Arrow,
    Circle,
    Cross,
    Triangle,
    Diamond,
    HollowDiamond,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineKind {
    Normal,
    Dotted,
    Thick,
    Invisible,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub text: String,
    pub center: bool,
}

impl Line {
    pub fn center(s: &str) -> Line {
        Line { text: s.to_string(), center: true }
    }
    pub fn left(s: &str) -> Line {
        Line { text: s.to_string(), center: false }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub lines: Vec<Line>,
    /// A divider row is drawn before `lines[i]` for each i listed.
    pub dividers: Vec<usize>,
    pub shape: Shape,
    pub cluster: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
    pub kind: LineKind,
    pub start: Marker,
    pub end: Marker,
    pub minlen: usize,
}

impl Edge {
    pub fn new(from: usize, to: usize) -> Edge {
        Edge {
            from,
            to,
            label: None,
            kind: LineKind::Normal,
            start: Marker::None,
            end: Marker::Arrow,
            minlen: 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Cluster {
    pub title: String,
    pub parent: Option<usize>,
}

pub struct Graph {
    pub dir: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub clusters: Vec<Cluster>,
    ids: HashMap<String, usize>,
}

impl Graph {
    pub fn new(dir: Direction) -> Graph {
        Graph { dir, nodes: Vec::new(), edges: Vec::new(), clusters: Vec::new(), ids: HashMap::new() }
    }

    /// Get or create a node; new nodes are labelled with their id.
    pub fn intern(&mut self, id: &str) -> usize {
        if let Some(&i) = self.ids.get(id) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(Node {
            lines: vec![Line::center(id)],
            dividers: Vec::new(),
            shape: Shape::Rect,
            cluster: None,
        });
        self.ids.insert(id.to_string(), i);
        i
    }

    pub fn get(&self, id: &str) -> Option<usize> {
        self.ids.get(id).copied()
    }

    pub fn add_cluster(&mut self, title: &str, parent: Option<usize>) -> usize {
        self.clusters.push(Cluster { title: title.to_string(), parent });
        self.clusters.len() - 1
    }
}

/// Split a label on `<br>` variants and literal "\n".
pub fn split_label(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        let lower = rest.to_ascii_lowercase();
        let mut cut: Option<(usize, usize)> = None;
        for pat in ["<br/>", "<br />", "<br>", "\\n"] {
            if let Some(i) = lower.find(pat) {
                if cut.is_none_or(|(c, _)| i < c) {
                    cut = Some((i, pat.len()));
                }
            }
        }
        match cut {
            Some((i, l)) => {
                out.push(rest[..i].trim().to_string());
                rest = &rest[i + l..];
            }
            None => {
                out.push(rest.trim().to_string());
                break;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------

const DUMMY: usize = usize::MAX;

struct NodeBox {
    w: i32,
    h: i32,
    lines: Vec<(String, bool)>,
    divs: Vec<usize>,
    shape: Shape,
}

struct Item {
    node: usize,
    cluster: Option<usize>,
    layer: usize,
    /// Cross-axis size including self-loop reserve.
    cs: i32,
    /// Cross-axis size of the box itself.
    bcs: i32,
    /// Main-axis size.
    ms: i32,
    bare: bool,
}

struct Seg {
    a: usize,
    b: usize,
    pa: i32,
    pb: i32,
    track: i32,
}

struct Chain {
    edge: usize,
    segs: Vec<usize>,
    reversed: bool,
}

/// `ports`: most edges on one side; the box grows so each gets its own port.
fn node_box(nd: &Node, ascii: bool, tall: bool, ports: usize, horiz: bool) -> NodeBox {
    let mut lines: Vec<(String, bool)> = nd.lines.iter().map(|l| (l.text.clone(), l.center)).collect();
    if lines.is_empty() {
        lines.push((String::new(), true));
    }
    if nd.shape == Shape::Diamond {
        let p = if ascii { "<>" } else { "◆" };
        lines[0].0 = format!("{} {}", p, lines[0].0);
    }
    let tw = lines.iter().map(|l| str_width(&l.0)).max().unwrap_or(0) as i32;
    if nd.shape == Shape::Bare {
        return NodeBox { w: tw.max(1), h: lines.len() as i32, lines, divs: Vec::new(), shape: nd.shape };
    }
    let mut h = (lines.len() + nd.dividers.len()) as i32 + 2;
    if tall {
        h = h.max(4);
    }
    let mut w = tw + 4;
    let ports = ports as i32;
    if horiz {
        h = h.max(ports + 2);
    } else {
        w = w.max(2 * ports + 3);
    }
    NodeBox { w, h, lines, divs: nd.dividers.clone(), shape: nd.shape }
}

fn draw_box(cv: &mut Canvas, b: &NodeBox, x: i32, y: i32) {
    if b.shape == Shape::Bare {
        for (i, (t, _)) in b.lines.iter().enumerate() {
            let tx = x + (b.w - str_width(t) as i32) / 2;
            cv.text(tx, y + i as i32, t, OWNER_SOLID);
        }
        return;
    }
    let style = match b.shape {
        Shape::Round => Style::Round,
        Shape::Circle => Style::Double,
        _ => Style::Light,
    };
    cv.rect(x, y, b.w, b.h, style, OWNER_SOLID);
    let content = (b.lines.len() + b.divs.len()) as i32;
    let mut row = y + 1 + (b.h - 2 - content) / 2;
    for (i, (t, center)) in b.lines.iter().enumerate() {
        if b.divs.contains(&i) {
            cv.segment(x, row, x + b.w - 1, row, Style::Light, OWNER_SOLID);
            row += 1;
        }
        let tw = str_width(t) as i32;
        let tx = if *center { x + 1 + (b.w - 2 - tw) / 2 } else { x + 2 };
        cv.text(tx, row, t, OWNER_SOLID);
        row += 1;
    }
}

fn lca(a: Option<usize>, b: Option<usize>, cpar: &[Option<usize>]) -> Option<usize> {
    let mut anc = Vec::new();
    let mut x = a;
    while let Some(c) = x {
        anc.push(c);
        x = cpar[c];
    }
    let mut y = b;
    while let Some(c) = y {
        if anc.contains(&c) {
            return Some(c);
        }
        y = cpar[c];
    }
    None
}

fn in_cluster(mut x: Option<usize>, c: usize, cpar: &[Option<usize>]) -> bool {
    while let Some(y) = x {
        if y == c {
            return true;
        }
        x = cpar[y];
    }
    false
}

/// The child of `parent` on the path from cluster `c` up to `parent`, if any.
fn child_of(c: Option<usize>, parent: Option<usize>, cpar: &[Option<usize>]) -> Option<usize> {
    if c == parent {
        return None;
    }
    let mut x = c;
    while let Some(y) = x {
        if cpar[y] == parent {
            return Some(y);
        }
        x = cpar[y];
    }
    None
}

/// Sort a layer by key while keeping each cluster's items contiguous.
fn cluster_order(
    list: &[usize],
    parent: Option<usize>,
    key: &[f64],
    icl: &[Option<usize>],
    cpar: &[Option<usize>],
    out: &mut Vec<usize>,
) {
    // (key sum, count, first index, cluster, item)
    let mut ents: Vec<(f64, f64, usize, Option<usize>, usize)> = Vec::new();
    for (j, &it) in list.iter().enumerate() {
        match child_of(icl[it], parent, cpar) {
            None => ents.push((key[it], 1.0, j, None, it)),
            Some(c) => {
                if let Some(e) = ents.iter_mut().find(|e| e.3 == Some(c)) {
                    e.0 += key[it];
                    e.1 += 1.0;
                } else {
                    ents.push((key[it], 1.0, j, Some(c), it));
                }
            }
        }
    }
    ents.sort_by(|a, b| (a.0 / a.1).partial_cmp(&(b.0 / b.1)).unwrap().then(a.2.cmp(&b.2)));
    for e in &ents {
        match e.3 {
            None => out.push(e.4),
            Some(c) => {
                let sub: Vec<usize> =
                    list.iter().copied().filter(|&it| child_of(icl[it], parent, cpar) == Some(c)).collect();
                cluster_order(&sub, Some(c), key, icl, cpar, out);
            }
        }
    }
}

fn count_crossings(by_gap: &[Vec<usize>], segs: &[Seg], idx: &[usize]) -> usize {
    let mut total = 0;
    for list in by_gap {
        for i in 0..list.len() {
            let (a1, b1) = (idx[segs[list[i]].a], idx[segs[list[i]].b]);
            for &s in &list[i + 1..] {
                let (a2, b2) = (idx[segs[s].a], idx[segs[s].b]);
                if (a1 < a2 && b1 > b2) || (a1 > a2 && b1 < b2) {
                    total += 1;
                }
            }
        }
    }
    total
}

/// Weighted isotonic regression (pool adjacent violators): nondecreasing fit.
fn pav(target: &[f64], w: &[f64]) -> Vec<f64> {
    let mut blocks: Vec<(f64, f64, usize)> = Vec::with_capacity(target.len());
    for i in 0..target.len() {
        let mut b = (target[i], w[i], 1usize);
        while let Some(&last) = blocks.last() {
            if last.0 > b.0 {
                let wt = last.1 + b.1;
                b = ((last.0 * last.1 + b.0 * b.1) / wt, wt, last.2 + b.2);
                blocks.pop();
            } else {
                break;
            }
        }
        blocks.push(b);
    }
    let mut out = Vec::with_capacity(target.len());
    for b in blocks {
        for _ in 0..b.2 {
            out.push(b.0);
        }
    }
    out
}

fn sign(v: i32) -> i32 {
    v.signum()
}

fn dir_of(a: (i32, i32), b: (i32, i32)) -> Dir {
    if b.0 > a.0 {
        Dir::Right
    } else if b.0 < a.0 {
        Dir::Left
    } else if b.1 < a.1 {
        Dir::Up
    } else {
        Dir::Down
    }
}

pub fn marker_char(m: Marker, d: Dir, ascii: bool) -> char {
    match m {
        Marker::None => ' ',
        Marker::Arrow => canvas::arrow(d, ascii),
        Marker::Triangle => canvas::triangle(d, ascii),
        Marker::Circle => if ascii { 'o' } else { '○' },
        Marker::Cross => if ascii { 'x' } else { '×' },
        Marker::Diamond => if ascii { '*' } else { '◆' },
        Marker::HollowDiamond => if ascii { 'o' } else { '◇' },
    }
}

fn line_style(k: LineKind) -> Style {
    match k {
        LineKind::Dotted => Style::Dotted,
        LineKind::Thick => Style::Thick,
        _ => Style::Light,
    }
}

/// Draw a polyline whose endpoints sit on node borders (or next to bare nodes),
/// replacing an end cell with its marker where one is set.
#[allow(clippy::too_many_arguments)]
fn draw_edge(
    cv: &mut Canvas,
    pts: &mut Vec<(i32, i32)>,
    sm: Marker,
    em: Marker,
    sbare: bool,
    ebare: bool,
    style: Style,
    owner: u32,
) {
    pts.dedup();
    let n = pts.len();
    if n < 2 {
        return;
    }
    let ascii = cv.ascii;
    let d_end = dir_of(pts[n - 2], pts[n - 1]);
    let d_start = dir_of(pts[1], pts[0]);
    let mut marks = Vec::new();
    if em != Marker::None {
        if !ebare {
            let (p, q) = (pts[n - 1], pts[n - 2]);
            pts[n - 1] = (p.0 + sign(q.0 - p.0), p.1 + sign(q.1 - p.1));
        }
        marks.push((pts[n - 1], marker_char(em, d_end, ascii)));
    }
    if sm != Marker::None {
        if !sbare {
            let (p, q) = (pts[0], pts[1]);
            pts[0] = (p.0 + sign(q.0 - p.0), p.1 + sign(q.1 - p.1));
        }
        marks.push((pts[0], marker_char(sm, d_start, ascii)));
    }
    cv.polyline(pts, style, owner);
    for ((x, y), c) in marks {
        cv.put(x, y, c, owner);
    }
}

fn path_cells(pts: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut cells = Vec::new();
    for w in pts.windows(2) {
        let (mut x, mut y) = w[0];
        let (dx, dy) = (sign(w[1].0 - x), sign(w[1].1 - y));
        cells.push((x, y));
        while (x, y) != w[1] {
            x += dx;
            y += dy;
            cells.push((x, y));
        }
    }
    cells
}

/// Find a free spot for an edge label along `cells`; `main_target` is the
/// preferred main-axis screen coordinate of the label centre.
fn place_label(cv: &mut Canvas, cells: &[(i32, i32)], label: &str, owner: u32, horiz: bool, main_target: i32) {
    let lw = str_width(label) as i32;
    if lw == 0 || cells.is_empty() {
        return;
    }
    let main_of = |x: i32, y: i32| if horiz { x } else { y };
    let mut best: Option<(i32, i32, i32)> = None; // score, x, y
    let mut consider = |score: i32, x: i32, y: i32| {
        if best.is_none_or(|b| score < b.0) {
            best = Some((score, x, y));
        }
    };
    for &(x, y) in cells {
        if cv.owner(x, y) != owner {
            continue;
        }
        if cv.is_straight(x, y, false) {
            let pen = if horiz { 100 } else { 0 };
            let d = (main_of(x, y) - main_target).abs() * 2;
            // Keep two clear cells beyond the label so neighbours don't read as one.
            if (1..=lw + 3).all(|i| cv.is_empty(x + i, y)) {
                consider(d + pen, x + 2, y);
            }
            if (1..=lw + 3).all(|i| cv.is_empty(x - i, y)) {
                consider(d + pen + 1, x - 1 - lw, y);
            }
        }
        if cv.is_straight(x, y, true)
            && (0..=lw + 1).all(|i| cv.is_straight(x + i, y, true) && cv.owner(x + i, y) == owner)
        {
            let pen = if horiz { 0 } else { 100 };
            let center = if horiz { x + 1 + lw / 2 } else { y };
            consider((center - main_target).abs() + pen, x + 1, y);
        }
    }
    let (x, y) = match best {
        Some((_, x, y)) => (x, y),
        None => {
            // Overwrite next to the path cell nearest the preferred spot.
            let &(x, y) = cells
                .iter()
                .min_by_key(|&&(x, y)| (main_of(x, y) - main_target).abs())
                .unwrap();
            (x + 2, y)
        }
    };
    cv.text(x, y, label, OWNER_SOLID);
}

pub fn render(g: &Graph, ascii: bool) -> String {
    let n = g.nodes.len();
    if n == 0 {
        return String::new();
    }
    let horiz = g.dir.horizontal();
    let flip = matches!(g.dir, Direction::BT | Direction::RL);

    // Self-loops are drawn beside their node and kept out of the layering.
    let mut loops: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut lay_edges: Vec<usize> = Vec::new();
    for (i, e) in g.edges.iter().enumerate() {
        if e.from == e.to {
            if e.kind != LineKind::Invisible {
                loops[e.from].push(i);
            }
        } else {
            lay_edges.push(i);
        }
    }
    // ---- cycle removal: DFS from sources in declaration order
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for &e in &lay_edges {
        adj[g.edges[e].from].push(e);
        indeg[g.edges[e].to] += 1;
    }
    let mut reversed = vec![false; g.edges.len()];
    let mut state = vec![0u8; n];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for r in (0..n).filter(|&i| indeg[i] == 0).chain(0..n) {
        if state[r] != 0 {
            continue;
        }
        state[r] = 1;
        stack.push((r, 0));
        while let Some(top) = stack.last_mut() {
            let u = top.0;
            if top.1 < adj[u].len() {
                let e = adj[u][top.1];
                top.1 += 1;
                let v = g.edges[e].to;
                if state[v] == 1 {
                    reversed[e] = true;
                } else if state[v] == 0 {
                    state[v] = 1;
                    stack.push((v, 0));
                }
            } else {
                state[u] = 2;
                stack.pop();
            }
        }
    }
    let ends = |e: usize| -> (usize, usize) {
        let ed = &g.edges[e];
        if reversed[e] { (ed.to, ed.from) } else { (ed.from, ed.to) }
    };
    let mut pin = vec![0usize; n];
    let mut pout = vec![0usize; n];
    for &e in &lay_edges {
        if g.edges[e].kind != LineKind::Invisible {
            let (u, v) = ends(e);
            pout[u] += 1;
            pin[v] += 1;
        }
    }
    let boxes: Vec<NodeBox> = g
        .nodes
        .iter()
        .enumerate()
        .map(|(i, nd)| node_box(nd, ascii, !loops[i].is_empty() && !horiz, pin[i].max(pout[i]), horiz))
        .collect();

    // ---- layering: longest path, then pull sources down next to their targets
    let mut dout: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut din = vec![0usize; n];
    for &e in &lay_edges {
        let (u, v) = ends(e);
        dout[u].push(e);
        din[v] += 1;
    }
    let mut layer = vec![0usize; n];
    let mut cnt = din.clone();
    let mut topo: Vec<usize> = (0..n).filter(|&i| din[i] == 0).collect();
    let mut qi = 0;
    while qi < topo.len() {
        let u = topo[qi];
        qi += 1;
        for &e in &dout[u] {
            let v = ends(e).1;
            layer[v] = layer[v].max(layer[u] + g.edges[e].minlen.max(1));
            cnt[v] -= 1;
            if cnt[v] == 0 {
                topo.push(v);
            }
        }
    }
    for &u in topo.iter().rev() {
        if din[u] == 0 && !dout[u].is_empty() {
            layer[u] = dout[u].iter().map(|&e| layer[ends(e).1] - g.edges[e].minlen.max(1)).min().unwrap();
        }
    }

    // ---- clusters
    let ncl = g.clusters.len();
    let cpar: Vec<Option<usize>> = g.clusters.iter().map(|c| c.parent).collect();
    let mut cdepth = vec![0usize; ncl];
    for (c, d) in cdepth.iter_mut().enumerate() {
        let mut x = Some(c);
        while let Some(y) = x {
            *d += 1;
            x = cpar[y];
        }
    }
    let dep = |c: Option<usize>| c.map_or(0, |c| cdepth[c]);

    // ---- items (real nodes first, then dummies) and edge chains
    let loop_reserve = |i: usize| -> i32 {
        if loops[i].is_empty() {
            0
        } else if horiz {
            2
        } else {
            let lw = loops[i].iter().filter_map(|&e| g.edges[e].label.as_deref()).map(str_width).max();
            3 + lw.map_or(0, |w| w as i32 + 1)
        }
    };
    let mut items: Vec<Item> = (0..n)
        .map(|i| {
            let b = &boxes[i];
            let (bcs, ms) = if horiz { (b.h, b.w) } else { (b.w, b.h) };
            Item {
                node: i,
                cluster: g.nodes[i].cluster,
                layer: layer[i],
                cs: bcs + loop_reserve(i),
                bcs,
                ms,
                bare: b.shape == Shape::Bare,
            }
        })
        .collect();
    let mut chains: Vec<Chain> = Vec::new();
    let mut segs: Vec<Seg> = Vec::new();
    for &e in &lay_edges {
        let (u, v) = ends(e);
        let cl = lca(g.nodes[u].cluster, g.nodes[v].cluster, &cpar);
        let mut chain = Chain { edge: e, segs: Vec::new(), reversed: reversed[e] };
        let mut prev = u;
        let mut push_seg = |a: usize, b: usize, segs: &mut Vec<Seg>| {
            chain.segs.push(segs.len());
            segs.push(Seg { a, b, pa: 0, pb: 0, track: -1 });
        };
        for l in layer[u] + 1..layer[v] {
            let d = items.len();
            items.push(Item { node: DUMMY, cluster: cl, layer: l, cs: 1, bcs: 1, ms: 1, bare: false });
            push_seg(prev, d, &mut segs);
            prev = d;
        }
        push_seg(prev, v, &mut segs);
        chains.push(chain);
    }
    let ni = items.len();
    let nl = items.iter().map(|it| it.layer).max().unwrap() + 1;
    let icl: Vec<Option<usize>> = items.iter().map(|i| i.cluster).collect();

    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); nl];
    for (i, it) in items.iter().enumerate() {
        layers[it.layer].push(i);
    }
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); ni];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); ni];
    let mut by_gap: Vec<Vec<usize>> = vec![Vec::new(); nl];
    for (si, s) in segs.iter().enumerate() {
        succs[s.a].push(s.b);
        preds[s.b].push(s.a);
        by_gap[items[s.a].layer].push(si);
    }

    // ---- ordering: barycenter sweeps with cluster-contiguous sorting
    let mut idx = vec![0usize; ni];
    let reindex = |layers: &[Vec<usize>], idx: &mut Vec<usize>| {
        for l in layers {
            for (j, &it) in l.iter().enumerate() {
                idx[it] = j;
            }
        }
    };
    let mut key = vec![0f64; ni];
    for l in layers.iter_mut() {
        for (j, &it) in l.iter().enumerate() {
            key[it] = j as f64;
        }
        let mut out = Vec::with_capacity(l.len());
        cluster_order(l, None, &key, &icl, &cpar, &mut out);
        *l = out;
    }
    reindex(&layers, &mut idx);
    let mut best = layers.clone();
    let mut best_c = count_crossings(&by_gap, &segs, &idx);
    for _ in 0..8 {
        if best_c == 0 {
            break;
        }
        for pass in 0..2 {
            let order: Vec<usize> = if pass == 0 { (1..nl).collect() } else { (0..nl.saturating_sub(1)).rev().collect() };
            let nb = if pass == 0 { &preds } else { &succs };
            for k in order {
                for &it in &layers[k] {
                    key[it] = if nb[it].is_empty() {
                        idx[it] as f64
                    } else {
                        nb[it].iter().map(|&p| idx[p] as f64).sum::<f64>() / nb[it].len() as f64
                    };
                }
                let mut out = Vec::with_capacity(layers[k].len());
                cluster_order(&layers[k], None, &key, &icl, &cpar, &mut out);
                for (j, &it) in out.iter().enumerate() {
                    idx[it] = j;
                }
                layers[k] = out;
            }
        }
        let c = count_crossings(&by_gap, &segs, &idx);
        if c < best_c {
            best_c = c;
            best = layers.clone();
        }
    }
    layers = best;
    reindex(&layers, &mut idx);

    // ---- cross-axis positions
    let gap = |a: usize, b: usize| -> i32 {
        let (ia, ib) = (&items[a], &items[b]);
        let base = if horiz {
            1
        } else if ia.node == DUMMY || ib.node == DUMMY {
            2
        } else {
            3
        };
        let l = lca(ia.cluster, ib.cluster, &cpar);
        base + 2 * ((dep(ia.cluster) - dep(l)) + (dep(ib.cluster) - dep(l))) as i32
    };
    let layer_gaps: Vec<Vec<i32>> =
        layers.iter().map(|l| l.windows(2).map(|w| gap(w[0], w[1])).collect()).collect();
    let mut fpos = vec![0f64; ni];
    for (l, gaps) in layers.iter().zip(&layer_gaps) {
        let mut x = 0.0;
        for (j, &it) in l.iter().enumerate() {
            if j > 0 {
                x += gaps[j - 1] as f64;
            }
            fpos[it] = x;
            x += items[it].cs as f64;
        }
    }
    let align = |l: &[usize], gaps: &[i32], nb: &[Vec<usize>], fpos: &mut Vec<f64>| {
        if l.is_empty() {
            return;
        }
        let mut off = 0.0;
        let mut tgt = Vec::with_capacity(l.len());
        let mut w = Vec::with_capacity(l.len());
        for (j, &it) in l.iter().enumerate() {
            if j > 0 {
                off += items[l[j - 1]].cs as f64 + gaps[j - 1] as f64;
            }
            let half = items[it].bcs as f64 / 2.0;
            let (c, wt) = if nb[it].is_empty() {
                (fpos[it] + half, 0.3)
            } else {
                let s: f64 = nb[it].iter().map(|&p| fpos[p] + items[p].bcs as f64 / 2.0).sum();
                (s / nb[it].len() as f64, if items[it].node == DUMMY { 4.0 } else { 1.0 })
            };
            tgt.push(c - half - off);
            w.push(wt);
        }
        let y = pav(&tgt, &w);
        let mut off = 0.0;
        for (j, &it) in l.iter().enumerate() {
            if j > 0 {
                off += items[l[j - 1]].cs as f64 + gaps[j - 1] as f64;
            }
            fpos[it] = y[j] + off;
        }
    };
    for _ in 0..10 {
        for k in 1..nl {
            align(&layers[k], &layer_gaps[k], &preds, &mut fpos);
        }
        for k in (0..nl.saturating_sub(1)).rev() {
            align(&layers[k], &layer_gaps[k], &succs, &mut fpos);
        }
    }
    let mut pos: Vec<i32> = fpos.iter().map(|p| p.round() as i32).collect();
    let enforce = |pos: &mut Vec<i32>| {
        for (l, gaps) in layers.iter().zip(&layer_gaps) {
            for j in 1..l.len() {
                let need = pos[l[j - 1]] + items[l[j - 1]].cs + gaps[j - 1];
                if pos[l[j]] < need {
                    pos[l[j]] = need;
                }
            }
        }
    };

    // ---- cluster rectangles must not overlap outsiders or each other
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); ncl];
    for (i, it) in items.iter().enumerate() {
        let mut x = it.cluster;
        while let Some(c) = x {
            members[c].push(i);
            x = cpar[c];
        }
    }
    let crange: Vec<(usize, usize)> = members
        .iter()
        .map(|m| {
            let lo = m.iter().map(|&i| items[i].layer).min().unwrap_or(0);
            let hi = m.iter().map(|&i| items[i].layer).max().unwrap_or(0);
            (lo, hi)
        })
        .collect();
    let mut by_depth: Vec<usize> = (0..ncl).filter(|&c| !members[c].is_empty()).collect();
    by_depth.sort_by_key(|&c| std::cmp::Reverse(cdepth[c]));
    let title_w: Vec<i32> = g.clusters.iter().map(|c| str_width(&c.title) as i32).collect();
    let rects = |pos: &[i32]| -> Vec<(i32, i32)> {
        let mut r = vec![(0, 0); ncl];
        for &c in &by_depth {
            let mut lo = i32::MAX;
            let mut hi = i32::MIN;
            for &m in &members[c] {
                if items[m].cluster == Some(c) {
                    lo = lo.min(pos[m] - 2);
                    hi = hi.max(pos[m] + items[m].cs - 1 + 2);
                }
            }
            for &d in &by_depth {
                if cpar[d] == Some(c) {
                    lo = lo.min(r[d].0 - 2);
                    hi = hi.max(r[d].1 + 2);
                }
            }
            if !horiz {
                hi = hi.max(lo + title_w[c] + 4);
            }
            r[c] = (lo, hi);
        }
        r
    };
    for _ in 0..400 {
        enforce(&mut pos);
        let r = rects(&pos);
        let mut fix: Option<(Vec<usize>, i32)> = None;
        'scan: for &c in &by_depth {
            let (lo, hi) = r[c];
            let (l0, l1) = crange[c];
            for l in &layers[l0..=l1] {
                let first_mem = l.iter().position(|&it| in_cluster(icl[it], c, &cpar));
                for (j, &it) in l.iter().enumerate() {
                    if in_cluster(icl[it], c, &cpar) {
                        continue;
                    }
                    let (a, b) = (pos[it], pos[it] + items[it].cs - 1);
                    if b < lo - 1 || a > hi + 1 {
                        continue;
                    }
                    let before = match first_mem {
                        Some(f) => j < f,
                        None => a + b < lo + hi,
                    };
                    fix = Some(if before { (members[c].clone(), b + 2 - lo) } else { (vec![it], hi + 2 - a) });
                    break 'scan;
                }
            }
            for &d in &by_depth {
                if d == c || in_cluster(Some(d), c, &cpar) || in_cluster(Some(c), d, &cpar) {
                    continue;
                }
                let (dl0, dl1) = crange[d];
                if dl1 < l0 || dl0 > l1 {
                    continue;
                }
                let (dlo, dhi) = r[d];
                if dhi < lo - 1 || dlo > hi + 1 {
                    continue;
                }
                fix = Some(if (lo + hi, c) < (dlo + dhi, d) {
                    (members[d].clone(), hi + 2 - dlo)
                } else {
                    (members[c].clone(), dhi + 2 - lo)
                });
                break 'scan;
            }
        }
        match fix {
            None => break,
            Some((list, d)) => {
                for it in list {
                    pos[it] += d.max(1);
                }
            }
        }
    }
    enforce(&mut pos);
    let crect = rects(&pos);
    let mut minc = pos.iter().copied().min().unwrap_or(0);
    for &c in &by_depth {
        minc = minc.min(crect[c].0);
    }
    for p in pos.iter_mut() {
        *p -= minc;
    }
    let crect: Vec<(i32, i32)> = crect.iter().map(|&(a, b)| (a - minc, b - minc)).collect();

    // ---- ports
    let mut outs: Vec<Vec<usize>> = vec![Vec::new(); ni];
    let mut ins: Vec<Vec<usize>> = vec![Vec::new(); ni];
    for (si, s) in segs.iter().enumerate() {
        outs[s.a].push(si);
        ins[s.b].push(si);
    }
    let prange = |i: usize| -> (i32, i32) {
        let it = &items[i];
        let p = pos[i];
        if it.node == DUMMY {
            (p, p)
        } else if it.bare || it.bcs < 3 {
            (p, p + it.bcs - 1)
        } else {
            (p + 1, p + it.bcs - 2)
        }
    };
    let pcenter = |i: usize| {
        let (a, b) = prange(i);
        a + b
    };
    for i in 0..ni {
        let (lo, hi) = prange(i);
        let len = hi - lo + 1;
        for side in 0..2 {
            let mut list = if side == 0 { outs[i].clone() } else { ins[i].clone() };
            let other = |s: usize| if side == 0 { segs[s].b } else { segs[s].a };
            list.sort_by_key(|&s| (pcenter(other(s)), idx[other(s)]));
            let k = list.len() as i32;
            for (j, &s) in list.iter().enumerate() {
                let p = if k == 1 { lo + (len - 1) / 2 } else { lo + ((2 * j as i32 + 1) * len) / (2 * k) };
                let p = p.min(hi);
                if side == 0 {
                    segs[s].pa = p;
                } else {
                    segs[s].pb = p;
                }
            }
        }
    }
    // Straighten where one side is free to move.
    for s in segs.iter_mut() {
        let (alo, ahi) = prange(s.a);
        let (blo, bhi) = prange(s.b);
        let a_free = outs[s.a].len() == 1;
        let b_free = ins[s.b].len() == 1;
        if a_free && b_free {
            let (lo, hi) = (alo.max(blo), ahi.min(bhi));
            if lo <= hi {
                let p = (s.pa.clamp(lo, hi) + s.pb.clamp(lo, hi)) / 2;
                s.pa = p;
                s.pb = p;
            }
        } else if b_free && (blo..=bhi).contains(&s.pa) {
            s.pb = s.pa;
        } else if a_free && (alo..=ahi).contains(&s.pb) {
            s.pa = s.pb;
        }
    }

    // ---- tracks for jogs between layers
    let mut ntracks = vec![0i32; nl];
    for k in 0..nl.saturating_sub(1) {
        let mut list: Vec<usize> = by_gap[k].iter().copied().filter(|&s| segs[s].pa != segs[s].pb).collect();
        list.sort_by_key(|&s| {
            let sg = &segs[s];
            if sg.pb > sg.pa { (0, -sg.pa) } else { (1, sg.pa) }
        });
        // A seg leaving cross coord p must turn before any seg arriving at p,
        // else their runs along p overlap. Kahn order, smallest sort key first.
        let m = list.len();
        let mut indeg = vec![0usize; m];
        for a in 0..m {
            for b in 0..m {
                if a != b && segs[list[a]].pa == segs[list[b]].pb {
                    indeg[b] += 1;
                }
            }
        }
        let mut done = vec![false; m];
        let mut order = Vec::with_capacity(m);
        while order.len() < m {
            let next = (0..m).find(|&i| !done[i] && indeg[i] == 0).or_else(|| (0..m).find(|&i| !done[i])).unwrap();
            done[next] = true;
            order.push(next);
            for b in 0..m {
                if !done[b] && segs[list[next]].pa == segs[list[b]].pb {
                    indeg[b] = indeg[b].saturating_sub(1);
                }
            }
        }
        // (lo, hi, pa, pb, going right)
        let mut tracks: Vec<Vec<(i32, i32, i32, i32, bool)>> = Vec::new();
        for &oi in &order {
            let s = list[oi];
            let (pa, pb) = (segs[s].pa, segs[s].pb);
            let min_t = order
                .iter()
                .map(|&o| list[o])
                .filter(|&o| o != s && segs[o].pa == pb && segs[o].track >= 0)
                .map(|o| segs[o].track + 1)
                .max()
                .unwrap_or(0) as usize;
            let me = (pa.min(pb), pa.max(pb), pa, pb, pb > pa);
            let ok = |t: &(i32, i32, i32, i32, bool)| {
                me.1 < t.0 - 1
                    || me.0 > t.1 + 1
                    || (t.2 == pa && (t.4 == me.4 || me.0 == t.1 || me.1 == t.0))
                    || (t.3 == pb && (t.4 == me.4 || me.0 == t.1 || me.1 == t.0))
            };
            let t = match (min_t..tracks.len()).find(|&t| tracks[t].iter().all(ok)) {
                Some(t) => t,
                None => {
                    while tracks.len() < min_t {
                        tracks.push(Vec::new());
                    }
                    tracks.push(Vec::new());
                    tracks.len() - 1
                }
            };
            tracks[t].push(me);
            segs[s].track = t as i32;
        }
        ntracks[k] = tracks.len() as i32;
    }

    // ---- main-axis coordinates
    let edge_label = |e: usize| g.edges[e].label.as_deref().filter(|l| !l.is_empty());
    let layout_markers = |ch: &Chain| {
        let e = &g.edges[ch.edge];
        if ch.reversed { (e.end, e.start) } else { (e.start, e.end) }
    };
    let mut lab = vec![0i32; nl];
    let mut smark = vec![false; nl];
    for ch in &chains {
        if g.edges[ch.edge].kind == LineKind::Invisible {
            continue;
        }
        let k = items[segs[ch.segs[0]].a].layer;
        if let Some(l) = edge_label(ch.edge) {
            lab[k] = lab[k].max(str_width(l) as i32);
        }
        if layout_markers(ch).0 != Marker::None {
            smark[k] = true;
        }
    }
    let mut d_start = vec![1i32; ncl];
    let mut d_end = vec![1i32; ncl];
    for &c in &by_depth {
        for &d in &by_depth {
            if cpar[d] == Some(c) {
                if crange[d].0 == crange[c].0 {
                    d_start[c] = d_start[c].max(d_start[d] + 1);
                }
                if crange[d].1 == crange[c].1 {
                    d_end[c] = d_end[c].max(d_end[d] + 1);
                }
            }
        }
    }
    let mut start_lv = vec![0i32; nl];
    let mut end_lv = vec![0i32; nl];
    for &c in &by_depth {
        start_lv[crange[c].0] = start_lv[crange[c].0].max(d_start[c]);
        end_lv[crange[c].1] = end_lv[crange[c].1].max(d_end[c]);
    }
    let stub: Vec<i32> = (0..nl).map(|k| if horiz { 2 } else { 1 } + smark[k] as i32).collect();
    let labsz: Vec<i32> =
        (0..nl).map(|k| if lab[k] == 0 { 0 } else if horiz { lab[k] + 2 } else { 1 }).collect();
    let arrow_sz = if horiz { 2 } else { 1 };
    let mut thick: Vec<i32> = vec![1; nl];
    for it in &items {
        thick[it.layer] = thick[it.layer].max(it.ms);
    }
    let mut lstart = vec![0i32; nl];
    let mut gstart = vec![0i32; nl];
    let mut total;
    loop {
        let mut m = 2 * start_lv[0];
        for k in 0..nl {
            lstart[k] = m;
            m += thick[k];
            gstart[k] = m;
            if k + 1 < nl {
                m += 2 * end_lv[k] + stub[k] + ntracks[k] + labsz[k] + arrow_sz + 2 * start_lv[k + 1];
            }
        }
        total = m + 2 * end_lv[nl - 1];
        let mut changed = false;
        if horiz {
            for &c in &by_depth {
                let (l0, l1) = crange[c];
                let width = (lstart[l1] + thick[l1] + 2 * d_end[c]) - (lstart[l0] - 2 * d_start[c]);
                let need = title_w[c] + 5;
                if width < need {
                    thick[l1] += need - width;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut mpos = vec![0i32; ni];
    let mut ems = vec![0i32; ni];
    for (i, it) in items.iter().enumerate() {
        let k = it.layer;
        if it.node == DUMMY {
            mpos[i] = lstart[k];
            ems[i] = thick[k];
        } else {
            mpos[i] = lstart[k] + (thick[k] - it.ms) / 2;
            ems[i] = it.ms;
        }
    }

    // ---- drawing
    let mut cv = Canvas::new(ascii);
    let ox = 2 + lab.iter().copied().max().unwrap_or(0) + 2;
    let oy = 2;
    let screen = |c: i32, m: i32| -> (i32, i32) {
        let m = if flip { total - 1 - m } else { m };
        if horiz { (m + ox, c + oy) } else { (c + ox, m + oy) }
    };
    // Rect from cross range [c0, c1] and main range [m0, m1] -> (x, y, w, h)
    let screen_rect = |c0: i32, c1: i32, m0: i32, m1: i32| -> (i32, i32, i32, i32) {
        let (xa, ya) = screen(c0, m0);
        let (xb, yb) = screen(c1, m1);
        (xa.min(xb), ya.min(yb), (xa - xb).abs() + 1, (ya - yb).abs() + 1)
    };

    let mut by_depth_asc = by_depth.clone();
    by_depth_asc.reverse();
    let mut title_at = Vec::new();
    for &c in &by_depth_asc {
        let (l0, l1) = crange[c];
        let top = lstart[l0] - 2 * d_start[c];
        let bot = lstart[l1] + thick[l1] - 1 + 2 * d_end[c];
        let (x, y, w, h) = screen_rect(crect[c].0, crect[c].1, top, bot);
        cv.rect(x, y, w, h, Style::Light, OWNER_SOLID);
        title_at.push((x, y, c));
    }

    let mut node_screen = vec![(0i32, 0i32); n];
    for i in 0..n {
        let b = &boxes[i];
        let (bw, bh) = (b.w, b.h);
        let (cw, mh) = if horiz { (bh, bw) } else { (bw, bh) };
        let (x, y, _, _) = screen_rect(pos[i], pos[i] + cw - 1, mpos[i], mpos[i] + mh - 1);
        node_screen[i] = (x, y);
        draw_box(&mut cv, b, x, y);
    }

    let exit_m = |i: usize| {
        let it = &items[i];
        if it.bare { mpos[i] + ems[i] } else { mpos[i] + ems[i] - 1 }
    };
    let entry_m = |i: usize| if items[i].bare { mpos[i] - 1 } else { mpos[i] };
    let mut labels: Vec<(Vec<(i32, i32)>, &str, u32, i32)> = Vec::new();
    for ch in &chains {
        let e = &g.edges[ch.edge];
        if e.kind == LineKind::Invisible {
            continue;
        }
        let mut pts: Vec<(i32, i32)> = Vec::new();
        let mut seg0_end = 0;
        for (j, &si) in ch.segs.iter().enumerate() {
            let s = &segs[si];
            pts.push((s.pa, exit_m(s.a)));
            if s.pa != s.pb {
                let k = items[s.a].layer;
                let tm = gstart[k] + 2 * end_lv[k] + stub[k] + s.track;
                pts.push((s.pa, tm));
                pts.push((s.pb, tm));
            }
            pts.push((s.pb, entry_m(s.b)));
            if j == 0 {
                seg0_end = pts.len();
            }
        }
        let k0 = items[segs[ch.segs[0]].a].layer;
        let lab_m = gstart[k0] + 2 * end_lv[k0] + stub[k0] + ntracks[k0] + labsz[k0] / 2;
        let mut scr: Vec<(i32, i32)> = pts.iter().map(|&(c, m)| screen(c, m)).collect();
        let seg0: Vec<(i32, i32)> = scr[..seg0_end].to_vec();
        let (sm, em) = layout_markers(ch);
        let sbare = items[segs[ch.segs[0]].a].bare;
        let ebare = items[segs[*ch.segs.last().unwrap()].b].bare;
        let owner = OWNER_EDGE0 + ch.edge as u32;
        draw_edge(&mut cv, &mut scr, sm, em, sbare, ebare, line_style(e.kind), owner);
        if let Some(l) = edge_label(ch.edge) {
            let (tx, ty) = screen(0, lab_m);
            labels.push((path_cells(&seg0), l, owner, if horiz { tx } else { ty }));
        }
    }

    for i in 0..n {
        let (x, y) = node_screen[i];
        let (w, h) = (boxes[i].w, boxes[i].h);
        for &ei in &loops[i] {
            let e = &g.edges[ei];
            let (mut pts, lpos) = if horiz {
                (
                    vec![(x + w - 4, y + h - 1), (x + w - 4, y + h), (x + w - 2, y + h), (x + w - 2, y + h - 1)],
                    (x + w, y + h),
                )
            } else {
                (vec![(x + w - 1, y + 1), (x + w + 1, y + 1), (x + w + 1, y + 2), (x + w - 1, y + 2)], (x + w + 3, y + 1))
            };
            let owner = OWNER_EDGE0 + ei as u32;
            draw_edge(&mut cv, &mut pts, e.start, e.end, false, false, line_style(e.kind), owner);
            if let Some(l) = edge_label(ei) {
                cv.text(lpos.0, lpos.1, l, OWNER_SOLID);
            }
        }
    }

    for (cells, l, owner, target) in &labels {
        place_label(&mut cv, cells, l, *owner, horiz, *target);
    }
    for (x, y, c) in title_at {
        let t = &g.clusters[c].title;
        if !t.is_empty() {
            cv.put(x + 1, y, ' ', OWNER_SOLID);
            cv.text(x + 2, y, t, OWNER_SOLID);
            cv.put(x + 2 + str_width(t) as i32, y, ' ', OWNER_SOLID);
        }
    }
    cv.render()
}
