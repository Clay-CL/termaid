//! `stateDiagram` / `stateDiagram-v2` parser.

use crate::graph_layout::{Direction, Edge, Graph, Line, LineKind, Marker, Shape, split_label};
use crate::text::{decode_entities, strip_quotes};
use std::collections::{HashMap, HashSet};

enum End {
    Node(usize),
    /// Composite state id; resolved after parsing. bool = used as source.
    Composite(String, bool),
}

struct Parser {
    g: Graph,
    ascii: bool,
    composite_ids: HashSet<String>,
    pending: Vec<(End, End, Option<String>)>,
    /// Composite state id -> cluster
    composites: HashMap<String, usize>,
    /// Scope stack of composite clusters
    stack: Vec<usize>,
    /// Descriptions per state id
    descs: HashMap<String, Vec<String>>,
    labels: HashMap<String, String>,
    note_count: usize,
}

impl Parser {
    fn scope(&self) -> Option<usize> {
        self.stack.last().copied()
    }

    fn state(&mut self, id: &str) -> usize {
        let n = self.g.intern(id);
        if self.g.nodes[n].shape == Shape::Rect {
            self.g.nodes[n].shape = Shape::Round;
            self.g.nodes[n].cluster = self.scope();
        }
        n
    }

    fn pseudo(&mut self, start: bool) -> usize {
        let scope = self.scope();
        let id = format!("[*]{}{}", if start { "start" } else { "end" }, scope.map_or(-1, |c| c as i64));
        let n = self.g.intern(&id);
        let nd = &mut self.g.nodes[n];
        nd.shape = Shape::Bare;
        nd.cluster = scope;
        let sym = match (start, self.ascii) {
            (true, false) => "●",
            (false, false) => "◉",
            (true, true) => "*",
            (false, true) => "@",
        };
        nd.lines = vec![Line::center(sym)];
        n
    }

    fn endpoint(&mut self, raw: &str, as_source: bool) -> End {
        let id = raw.split(":::").next().unwrap_or("").trim();
        if id == "[*]" {
            return End::Node(self.pseudo(as_source));
        }
        if self.composite_ids.contains(id) {
            return End::Composite(id.to_string(), as_source);
        }
        End::Node(self.state(id))
    }

    /// A composite is entered at its start dot and left from its end dot.
    fn resolve(&mut self, end: End) -> usize {
        match end {
            End::Node(n) => n,
            End::Composite(id, as_source) => {
                let Some(&c) = self.composites.get(&id) else { return self.state(&id) };
                let key = format!("[*]{}{}", if as_source { "end" } else { "start" }, c);
                if let Some(n) = self.g.get(&key) {
                    return n;
                }
                let inside = |mut x: Option<usize>, g: &Graph| {
                    while let Some(y) = x {
                        if y == c {
                            return true;
                        }
                        x = g.clusters[y].parent;
                    }
                    false
                };
                let found = if as_source {
                    self.g.nodes.iter().rposition(|nd| inside(nd.cluster, &self.g))
                } else {
                    self.g.nodes.iter().position(|nd| inside(nd.cluster, &self.g))
                };
                match found {
                    Some(n) => n,
                    None => {
                        // Empty composite: give it a placeholder state inside.
                        let n = self.g.intern(&format!("[empty]{}", c));
                        self.g.nodes[n].lines = vec![Line::center("")];
                        self.g.nodes[n].cluster = Some(c);
                        n
                    }
                }
            }
        }
    }

    fn statement(&mut self, line: &str) -> Result<(), String> {
        let first = line.split_whitespace().next().unwrap_or("");
        match first {
            "direction" => {
                if self.stack.is_empty() {
                    if let Some(d) = line.split_whitespace().nth(1).and_then(Direction::parse) {
                        self.g.dir = d;
                    }
                }
                return Ok(());
            }
            "classDef" | "class" | "style" | "accTitle" | "accDescr" | "click" => return Ok(()),
            "--" => return Ok(()),
            "}" => {
                self.stack.pop();
                return Ok(());
            }
            _ => {}
        }
        if let Some(rest) = line.strip_prefix("state ") {
            return self.state_decl(rest.trim());
        }
        if let Some((l, r)) = line.split_once("-->") {
            let (to, label) = match r.split_once(':') {
                Some((t, lab)) => (t, Some(decode_entities(lab.trim()))),
                None => (r, None),
            };
            let from = self.endpoint(l, true);
            let to = self.endpoint(to, false);
            self.pending.push((from, to, label.filter(|l| !l.is_empty())));
            return Ok(());
        }
        if let Some((id, desc)) = line.split_once(':') {
            let id = id.trim();
            if !id.is_empty() && !id.contains(' ') {
                self.state(id);
                self.descs.entry(id.to_string()).or_default().push(decode_entities(desc.trim()));
                return Ok(());
            }
        }
        let id = line.trim();
        if !id.is_empty() && !id.contains(char::is_whitespace) {
            self.state(id);
            return Ok(());
        }
        Err(format!("cannot parse '{}'", line))
    }

    fn state_decl(&mut self, rest: &str) -> Result<(), String> {
        let opens = rest.ends_with('{');
        let rest = rest.trim_end_matches('{').trim();
        // state "Description" as id
        let (id, label) = if rest.starts_with('"') {
            let close = rest[1..].find('"').map(|i| i + 1).ok_or("unterminated quote")?;
            let label = &rest[1..close];
            let after = rest[close + 1..].trim();
            let id = after.strip_prefix("as").map(str::trim).unwrap_or(label);
            (id.to_string(), Some(label.to_string()))
        } else if let Some((id, label)) = rest.split_once(" as ") {
            // state id as "label" is not mermaid, but `state "x" as id` is; accept both
            (label.trim().to_string(), Some(strip_quotes(id).to_string()))
        } else {
            let mut parts = rest.split_whitespace();
            (parts.next().unwrap_or("").to_string(), None)
        };
        let id = id.split(":::").next().unwrap_or("").to_string();
        if id.is_empty() {
            return Err("state without id".into());
        }
        if opens {
            let title = label.unwrap_or_else(|| id.clone());
            let c = self.g.add_cluster(&title, self.scope());
            self.composites.insert(id, c);
            self.stack.push(c);
            return Ok(());
        }
        let n = self.state(&id);
        if rest.contains("<<fork>>") || rest.contains("<<join>>") {
            let nd = &mut self.g.nodes[n];
            nd.shape = Shape::Bare;
            nd.lines = vec![Line::center(if self.ascii { "=======" } else { "━━━━━━━" })];
        } else if rest.contains("<<choice>>") {
            let nd = &mut self.g.nodes[n];
            nd.shape = Shape::Diamond;
            nd.lines = vec![Line::center("")];
        } else if let Some(l) = label {
            self.labels.insert(id, l);
        }
        Ok(())
    }

    fn note(&mut self, target: &str, text: &str) {
        let Some(t) = self.g.get(target) else { return };
        self.note_count += 1;
        let n = self.g.intern(&format!("[note]{}", self.note_count));
        let cluster = self.g.nodes[t].cluster;
        let nd = &mut self.g.nodes[n];
        nd.lines = split_label(text).iter().map(|l| Line::left(l)).collect();
        nd.cluster = cluster;
        let mut e = Edge::new(t, n);
        e.kind = LineKind::Dotted;
        e.end = Marker::None;
        self.g.edges.push(e);
    }
}

pub fn parse(src: &str, dir: Option<Direction>, ascii: bool) -> Result<Graph, String> {
    let mut lines = src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("%%"));
    lines.next(); // header
    let composite_ids: HashSet<String> = lines
        .clone()
        .filter_map(|l| l.strip_prefix("state ")?.strip_suffix('{'))
        .map(|r| {
            let r = r.trim();
            match r.split_once(" as ") {
                Some((_, id)) => id.trim().to_string(),
                None => r.split_whitespace().next().unwrap_or("").to_string(),
            }
        })
        .collect();
    let mut p = Parser {
        g: Graph::new(Direction::TD),
        ascii,
        composite_ids,
        pending: Vec::new(),
        composites: HashMap::new(),
        stack: Vec::new(),
        descs: HashMap::new(),
        labels: HashMap::new(),
        note_count: 0,
    };
    let mut note: Option<(String, Vec<String>)> = None;
    for line in lines {
        if let Some((target, buf)) = note.as_mut() {
            if line == "end note" {
                let (t, b) = (target.clone(), buf.join("\\n"));
                p.note(&t, &b);
                note = None;
            } else {
                buf.push(line.to_string());
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("note ") {
            // note right of X : text   |   note left of X  ...  end note
            let rest = rest.trim();
            let after = rest.split_once(" of ").map(|(_, a)| a).unwrap_or(rest);
            match after.split_once(':') {
                Some((t, text)) => p.note(t.trim(), text.trim()),
                None => note = Some((after.trim().to_string(), Vec::new())),
            }
            continue;
        }
        p.statement(line)?;
    }
    for (from, to, label) in std::mem::take(&mut p.pending) {
        let from = p.resolve(from);
        let to = p.resolve(to);
        let mut e = Edge::new(from, to);
        e.label = label;
        p.g.edges.push(e);
    }
    for (id, label) in std::mem::take(&mut p.labels) {
        if let Some(n) = p.g.get(&id) {
            p.g.nodes[n].lines = vec![Line::center(&label)];
        }
    }
    for (id, descs) in std::mem::take(&mut p.descs) {
        if let Some(n) = p.g.get(&id) {
            let nd = &mut p.g.nodes[n];
            nd.dividers = vec![nd.lines.len()];
            for d in descs {
                nd.lines.extend(split_label(&d).iter().map(|l| Line::left(l)));
            }
        }
    }
    if let Some(d) = dir {
        p.g.dir = d;
    }
    Ok(p.g)
}
