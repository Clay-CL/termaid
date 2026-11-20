//! `flowchart` / `graph` parser.

use crate::graph_layout::{Direction, Edge, Graph, Line, LineKind, Marker, Shape, split_label};
use crate::text::{decode_entities, split_statements, strip_quotes};
use std::collections::HashMap;

#[derive(Clone)]
struct Link {
    kind: LineKind,
    start: Marker,
    end: Marker,
    minlen: usize,
    label: Option<String>,
}

struct Parser {
    order: Vec<String>,
    defs: HashMap<String, (String, Shape)>,
    member: HashMap<String, usize>,
    edges: Vec<(String, String, Link)>,
    /// (id, title, parent)
    clusters: Vec<(String, String, Option<usize>)>,
    stack: Vec<usize>,
}

fn is_id_char(s: &[char], i: usize) -> bool {
    let c = s[i];
    if c.is_alphanumeric() || c == '_' || c == '$' || c == '#' {
        return true;
    }
    if c == '-' || c == '.' {
        return s.get(i + 1).is_some_and(|n| n.is_alphanumeric() || *n == '_');
    }
    !c.is_ascii() && !c.is_whitespace()
}

fn skip_ws(s: &[char], mut i: usize) -> usize {
    while i < s.len() && s[i].is_whitespace() {
        i += 1;
    }
    i
}

fn starts(s: &[char], i: usize, pat: &str) -> bool {
    let mut j = i;
    for c in pat.chars() {
        if s.get(j) != Some(&c) {
            return false;
        }
        j += 1;
    }
    true
}

fn find(s: &[char], from: usize, pat: &str) -> Option<usize> {
    (from..s.len()).find(|&i| starts(s, i, pat))
}

fn clean_label(raw: &str) -> String {
    let t = strip_quotes(raw.trim());
    let t = t.trim_matches('`');
    decode_entities(t)
}

impl Parser {
    fn mention(&mut self, id: &str) {
        if !self.order.iter().any(|o| o == id) {
            self.order.push(id.to_string());
        }
        if let Some(&c) = self.stack.last() {
            self.member.insert(id.to_string(), c);
        }
    }

    /// Parse one node reference (id + optional shape). Returns id and new index.
    fn node(&mut self, s: &[char], mut i: usize) -> Result<(String, usize), String> {
        let st = i;
        while i < s.len() && is_id_char(s, i) {
            i += 1;
        }
        if i == st {
            return Err(format!("expected a node id at '{}'", s[st..].iter().collect::<String>()));
        }
        let id: String = s[st..i].iter().collect();
        self.mention(&id);
        i = self.skip_class(s, i);
        // Shape delimiters: (open, close, shape)
        const SHAPES: &[(&str, &str, Shape)] = &[
            ("(((", ")))", Shape::Rect),
            ("((", "))", Shape::Circle),
            ("([", "])", Shape::Rect),
            ("[[", "]]", Shape::Rect),
            ("[(", ")]", Shape::Rect),
            ("[/", "/]", Shape::Rect),
            ("[\\", "\\]", Shape::Rect),
            ("{{", "}}", Shape::Rect),
            ("[", "]", Shape::Rect),
            ("(", ")", Shape::Round),
            ("{", "}", Shape::Diamond),
            (">", "]", Shape::Rect),
        ];
        if starts(s, i, "@{") {
            let close = find(s, i, "}").ok_or("unterminated @{ ... }")?;
            let body: String = s[i + 2..close].iter().collect();
            let mut label = id.clone();
            let mut shape = Shape::Rect;
            for kv in body.split(',') {
                if let Some((k, v)) = kv.split_once(':') {
                    let v = strip_quotes(v.trim());
                    match k.trim() {
                        "label" => label = decode_entities(v),
                        "shape" => {
                            shape = match v {
                                "rounded" | "event" => Shape::Round,
                                "diam" | "decision" | "diamond" | "question" => Shape::Diamond,
                                "circle" | "circ" => Shape::Circle,
                                _ => Shape::Rect,
                            }
                        }
                        _ => {}
                    }
                }
            }
            self.defs.insert(id.clone(), (label, shape));
            return Ok((id, self.skip_class(s, close + 1)));
        }
        for &(open, close, shape) in SHAPES {
            if !starts(s, i, open) {
                continue;
            }
            // `>` only opens a shape directly after the id (no space).
            let mut j = i + open.chars().count();
            let content_start = j;
            let mut alt_close = close;
            if open == "[/" || open == "[\\" {
                alt_close = if open == "[/" { "\\]" } else { "/]" };
            }
            let jj = skip_ws(s, j);
            if s.get(jj) == Some(&'"') {
                j = find(s, jj + 1, "\"").map(|q| q + 1).unwrap_or(jj + 1);
            }
            let end = match (find(s, j, close), find(s, j, alt_close)) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => return Err(format!("unclosed '{}' after node '{}'", open, id)),
            };
            let raw: String = s[content_start..end].iter().collect();
            self.defs.insert(id.clone(), (clean_label(&raw), shape));
            let after = end + close.chars().count();
            return Ok((id, self.skip_class(s, after)));
        }
        Ok((id, i))
    }

    fn skip_class(&self, s: &[char], mut i: usize) -> usize {
        if starts(s, i, ":::") {
            i += 3;
            while i < s.len() && (s[i].is_alphanumeric() || s[i] == '_' || s[i] == '-') {
                i += 1;
            }
        }
        i
    }

    fn group(&mut self, s: &[char], mut i: usize) -> Result<(Vec<String>, usize), String> {
        let mut ids = Vec::new();
        loop {
            i = skip_ws(s, i);
            let (id, j) = self.node(s, i)?;
            ids.push(id);
            i = skip_ws(s, j);
            if s.get(i) == Some(&'&') {
                i += 1;
                continue;
            }
            return Ok((ids, i));
        }
    }

    fn link(&self, s: &[char], mut i: usize) -> Option<(Link, usize)> {
        let mut start = Marker::None;
        match s.get(i) {
            Some('<') => {
                start = Marker::Arrow;
                i += 1;
            }
            Some(&c @ ('o' | 'x')) if matches!(s.get(i + 1), Some('-' | '=' | '.')) => {
                start = if c == 'o' { Marker::Circle } else { Marker::Cross };
                i += 1;
            }
            _ => {}
        }
        let b0 = i;
        while i < s.len() && matches!(s[i], '-' | '=' | '.' | '~') {
            i += 1;
        }
        let mut body: String = s[b0..i].iter().collect();
        if body.is_empty() {
            return None;
        }
        let end_marker = |s: &[char], i: usize| -> Option<Marker> {
            match s.get(i) {
                Some('>') => Some(Marker::Arrow),
                Some(&c @ ('o' | 'x')) if s.get(i + 1).is_none_or(|n| n.is_whitespace()) => {
                    Some(if c == 'o' { Marker::Circle } else { Marker::Cross })
                }
                _ => None,
            }
        };
        let mut end = end_marker(s, i);
        let mut label = None;
        if end.is_none()
            && matches!(body.as_str(), "--" | "==" | "-.")
            && s.get(i).is_some_and(|c| c.is_whitespace())
        {
            // Text form: `-- text -->`, `== text ==>`, `-. text .->`
            let closer = if body == "-." { ".-" } else { &body[..] };
            let j = find(s, i, closer)?;
            label = Some(clean_label(&s[i..j].iter().collect::<String>()));
            let mut k = j;
            while k < s.len() && matches!(s[k], '-' | '=' | '.') {
                k += 1;
            }
            body = format!("{}{}", body, s[j..k].iter().collect::<String>());
            i = k;
            end = end_marker(s, i);
        }
        if end.is_some() {
            i += 1;
        }
        let kind = if body.contains('~') {
            LineKind::Invisible
        } else if body.contains('.') {
            LineKind::Dotted
        } else if body.contains('=') {
            LineKind::Thick
        } else {
            LineKind::Normal
        };
        let minlen = if kind == LineKind::Dotted {
            body.matches('.').count() - label.is_some() as usize
        } else {
            let n = body.chars().count() - if label.is_some() { 2 } else { 0 };
            if end.is_some() { n.saturating_sub(1) } else { n.saturating_sub(2) }
        }
        .max(1);
        let j = skip_ws(s, i);
        if s.get(j) == Some(&'|') {
            if let Some(k) = find(s, j + 1, "|") {
                label = Some(clean_label(&s[j + 1..k].iter().collect::<String>()));
                i = k + 1;
            }
        }
        Some((Link { kind, start, end: end.unwrap_or(Marker::None), minlen, label }, i))
    }

    fn statement(&mut self, st: &str) -> Result<(), String> {
        let first = st.split_whitespace().next().unwrap_or("");
        match first {
            "subgraph" => {
                let rest = st["subgraph".len()..].trim();
                let (id, title) = if let Some(b) = rest.find('[') {
                    let t = rest[b + 1..].trim_end().trim_end_matches(']');
                    (rest[..b].trim().to_string(), clean_label(t))
                } else {
                    let t = clean_label(rest);
                    (strip_quotes(rest).to_string(), t)
                };
                let parent = self.stack.last().copied();
                self.clusters.push((id, title, parent));
                self.stack.push(self.clusters.len() - 1);
                return Ok(());
            }
            "end" => {
                self.stack.pop();
                return Ok(());
            }
            "direction" | "classDef" | "class" | "style" | "linkStyle" | "click" | "accTitle" | "accDescr"
            | "title" => return Ok(()),
            _ => {}
        }
        if first.starts_with("accTitle") || first.starts_with("accDescr") {
            return Ok(());
        }
        let s: Vec<char> = st.chars().collect();
        let (mut prev, mut i) = self.group(&s, 0)?;
        loop {
            i = skip_ws(&s, i);
            if i >= s.len() {
                return Ok(());
            }
            let Some((link, j)) = self.link(&s, i) else {
                return Err(format!("cannot parse '{}'", s[i..].iter().collect::<String>()));
            };
            let (next, k) = self.group(&s, j)?;
            for a in &prev {
                for b in &next {
                    self.edges.push((a.clone(), b.clone(), link.clone()));
                }
            }
            prev = next;
            i = k;
        }
    }
}

pub fn parse(src: &str, dir_override: Option<Direction>) -> Result<Graph, String> {
    let mut lines = src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("%%"));
    let header = lines.next().ok_or("empty diagram")?;
    // Statements may follow the header on the same line after ';'
    let (head, header_rest) = header.split_once(';').unwrap_or((header, ""));
    let dir = head.split_whitespace().nth(1).and_then(Direction::parse).unwrap_or(Direction::TD);
    let mut p = Parser {
        order: Vec::new(),
        defs: HashMap::new(),
        member: HashMap::new(),
        edges: Vec::new(),
        clusters: Vec::new(),
        stack: Vec::new(),
    };
    for line in std::iter::once(header_rest).chain(lines) {
        for st in split_statements(line) {
            p.statement(&st)?;
        }
    }

    let mut g = Graph::new(dir_override.unwrap_or(dir));
    for (_, title, parent) in &p.clusters {
        g.add_cluster(title, *parent);
    }
    let sub_ids: HashMap<&str, usize> =
        p.clusters.iter().enumerate().map(|(i, c)| (c.0.as_str(), i)).collect();
    for id in &p.order {
        if sub_ids.contains_key(id.as_str()) && !p.defs.contains_key(id) {
            continue;
        }
        let n = g.intern(id);
        if let Some((label, shape)) = p.defs.get(id) {
            g.nodes[n].lines = split_label(label).iter().map(|l| Line::center(l)).collect();
            g.nodes[n].shape = *shape;
        }
        g.nodes[n].cluster = p.member.get(id).copied();
    }
    // An edge to a subgraph id attaches to that subgraph's first node.
    let resolve = |g: &mut Graph, id: &str| -> usize {
        if let Some(n) = g.get(id) {
            return n;
        }
        if let Some(&c) = sub_ids.get(id) {
            let found = g.nodes.iter().position(|nd| {
                let mut x = nd.cluster;
                while let Some(y) = x {
                    if y == c {
                        return true;
                    }
                    x = g.clusters[y].parent;
                }
                false
            });
            if let Some(f) = found {
                return f;
            }
        }
        g.intern(id)
    };
    for (a, b, l) in p.edges {
        let from = resolve(&mut g, &a);
        let to = resolve(&mut g, &b);
        g.edges.push(Edge {
            from,
            to,
            label: l.label,
            kind: l.kind,
            start: l.start,
            end: l.end,
            minlen: l.minlen,
        });
    }
    Ok(g)
}
