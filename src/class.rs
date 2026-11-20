//! `classDiagram` parser.

use crate::graph_layout::{Direction, Edge, Graph, Line, LineKind, Marker, Shape, split_label};
use crate::text::{decode_entities, strip_quotes};
use std::collections::HashMap;

#[derive(Default)]
struct Class {
    label: Option<String>,
    annotation: Option<String>,
    attrs: Vec<String>,
    methods: Vec<String>,
    cluster: Option<usize>,
}

struct Parser {
    g: Graph,
    order: Vec<String>,
    classes: HashMap<String, Class>,
    ns_stack: Vec<usize>,
    /// Inside `class X {`: the class id
    open_class: Option<String>,
    rels: Vec<(String, String, Edge)>,
    notes: Vec<(Option<String>, String)>,
}

/// `Name~T~` -> `Name<T>`
fn generic(s: &str) -> String {
    let mut out = String::new();
    let mut open = true;
    for c in s.chars() {
        if c == '~' {
            out.push(if open { '<' } else { '>' });
            open = !open;
        } else {
            out.push(c);
        }
    }
    out
}

/// Split `Name["Label"]` into id and optional label.
fn id_label(s: &str) -> (String, Option<String>) {
    let s = s.trim();
    if let Some(b) = s.find('[') {
        let label = strip_quotes(s[b + 1..].trim_end_matches(']')).to_string();
        return (s[..b].trim().to_string(), Some(label));
    }
    (s.to_string(), None)
}

impl Parser {
    fn class(&mut self, raw_id: &str) -> &mut Class {
        let id = raw_id.split(":::").next().unwrap_or("").trim();
        let base = id.split('~').next().unwrap_or(id).to_string();
        if !self.classes.contains_key(&base) {
            self.order.push(base.clone());
            let label = (id.contains('~')).then(|| generic(id));
            let cluster = self.ns_stack.last().copied();
            self.classes.insert(base.clone(), Class { label, cluster, ..Default::default() });
        }
        self.classes.get_mut(&base).unwrap()
    }

    fn member(&mut self, id: &str, m: &str) {
        let m = m.trim();
        if m.is_empty() {
            return;
        }
        if let Some(a) = m.strip_prefix("<<").and_then(|r| r.strip_suffix(">>")) {
            self.class(id).annotation = Some(a.trim().to_string());
            return;
        }
        let text = generic(&decode_entities(m));
        let c = self.class(id);
        if text.contains('(') {
            c.methods.push(text);
        } else {
            c.attrs.push(text);
        }
    }

    fn relation(&mut self, line: &str) -> bool {
        let (body, label) = match line.split_once(':') {
            Some((b, l)) => (b, Some(strip_quotes(l.trim()).to_string())),
            None => (line, None),
        };
        let pos = [body.find("--"), body.find("..")].into_iter().flatten().min();
        let Some(i) = pos else { return false };
        let dotted = body[i..].starts_with("..");
        let mut left = body[..i].trim_end();
        let mut right = body[i + 2..].trim_start();
        let mut start = Marker::None;
        let mut end = Marker::None;
        for (suffix, m) in [("<|", Marker::Triangle), ("*", Marker::Diamond), ("<", Marker::Arrow)] {
            if let Some(l) = left.strip_suffix(suffix) {
                left = l;
                start = m;
                break;
            }
        }
        if start == Marker::None {
            if let Some(l) = left.strip_suffix('o') {
                if l.ends_with(|c: char| c.is_whitespace() || c == '"') {
                    left = l;
                    start = Marker::HollowDiamond;
                }
            }
        }
        for (prefix, m) in [("|>", Marker::Triangle), ("*", Marker::Diamond), (">", Marker::Arrow)] {
            if let Some(r) = right.strip_prefix(prefix) {
                right = r;
                end = m;
                break;
            }
        }
        if end == Marker::None {
            if let Some(r) = right.strip_prefix('o') {
                if r.starts_with(|c: char| c.is_whitespace() || c == '"') {
                    right = r;
                    end = Marker::HollowDiamond;
                }
            }
        }
        // Cardinalities: A "1" --> "*" B
        let (lname, lcard) = split_card(left.trim(), true);
        let (rname, rcard) = split_card(right.trim(), false);
        if lname.is_empty() || rname.is_empty() || lname.contains(' ') || rname.contains(' ') {
            return false;
        }
        let mut text = label.filter(|l| !l.is_empty()).unwrap_or_default();
        if lcard.is_some() || rcard.is_some() {
            let card = format!("[{} : {}]", lcard.unwrap_or_default(), rcard.unwrap_or_default());
            text = if text.is_empty() { card } else { format!("{} {}", text, card) };
        }
        self.class(&lname);
        self.class(&rname);
        let base = |s: &str| s.split('~').next().unwrap_or(s).split(":::").next().unwrap_or("").to_string();
        let mut e = Edge::new(0, 0);
        e.kind = if dotted { LineKind::Dotted } else { LineKind::Normal };
        e.start = start;
        e.end = end;
        e.label = (!text.is_empty()).then_some(text);
        self.rels.push((base(&lname), base(&rname), e));
        true
    }

    fn statement(&mut self, line: &str) -> Result<(), String> {
        if let Some(id) = self.open_class.clone() {
            if line == "}" {
                self.open_class = None;
            } else {
                self.member(&id, line);
            }
            return Ok(());
        }
        let first = line.split_whitespace().next().unwrap_or("");
        match first {
            "direction" => {
                if let Some(d) = line.split_whitespace().nth(1).and_then(Direction::parse) {
                    self.g.dir = d;
                }
                return Ok(());
            }
            "classDef" | "style" | "cssClass" | "click" | "callback" | "link" | "accTitle" | "accDescr"
            | "title" => return Ok(()),
            "}" => {
                self.ns_stack.pop();
                return Ok(());
            }
            "namespace" => {
                let name = line["namespace".len()..].trim().trim_end_matches('{').trim();
                let c = self.g.add_cluster(name, self.ns_stack.last().copied());
                self.ns_stack.push(c);
                return Ok(());
            }
            "note" => {
                let rest = line["note".len()..].trim();
                let (target, text) = match rest.strip_prefix("for ") {
                    Some(r) => {
                        let r = r.trim();
                        let sp = r.find(char::is_whitespace).unwrap_or(r.len());
                        (Some(r[..sp].to_string()), r[sp..].trim())
                    }
                    None => (None, rest),
                };
                self.notes.push((target, strip_quotes(text).to_string()));
                return Ok(());
            }
            "class" => {
                let rest = line["class".len()..].trim();
                let opens = rest.ends_with('{');
                let decl = rest.trim_end_matches('{').trim();
                let (id, label) = id_label(decl);
                let id = id.split(":::").next().unwrap_or("").to_string();
                let c = self.class(&id);
                if let Some(l) = label {
                    c.label = Some(l);
                }
                if opens {
                    let base = id.split('~').next().unwrap_or(&id).to_string();
                    self.open_class = Some(base);
                }
                return Ok(());
            }
            _ => {}
        }
        if let Some(rest) = line.strip_prefix("<<") {
            if let Some((a, id)) = rest.split_once(">>") {
                self.class(id.trim()).annotation = Some(a.trim().to_string());
                return Ok(());
            }
        }
        if self.relation(line) {
            return Ok(());
        }
        if let Some((id, m)) = line.split_once(':') {
            let id = id.trim();
            if !id.is_empty() && !id.contains(' ') {
                self.member(id, m);
                return Ok(());
            }
        }
        let id = line.trim();
        if !id.is_empty() && !id.contains(char::is_whitespace) {
            self.class(id);
            return Ok(());
        }
        Err(format!("cannot parse '{}'", line))
    }
}

/// `Name "1"` (left) or `"1" Name` (right) -> (name, cardinality)
fn split_card(s: &str, left: bool) -> (String, Option<String>) {
    if left {
        if let Some(stripped) = s.strip_suffix('"') {
            if let Some(q) = stripped.rfind('"') {
                return (stripped[..q].trim().to_string(), Some(stripped[q + 1..].to_string()));
            }
        }
    } else if let Some(stripped) = s.strip_prefix('"') {
        if let Some(q) = stripped.find('"') {
            return (stripped[q + 1..].trim().to_string(), Some(stripped[..q].to_string()));
        }
    }
    (s.to_string(), None)
}

pub fn parse(src: &str, dir: Option<Direction>, ascii: bool) -> Result<Graph, String> {
    let mut lines = src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("%%"));
    lines.next(); // header
    let mut p = Parser {
        g: Graph::new(Direction::TD),
        order: Vec::new(),
        classes: HashMap::new(),
        ns_stack: Vec::new(),
        open_class: None,
        rels: Vec::new(),
        notes: Vec::new(),
    };
    for line in lines {
        // `class A { +x }` on one line
        if let (Some(o), true) = (line.find('{'), line.ends_with('}') && line.starts_with("class ")) {
            p.statement(&line[..=o])?;
            for m in line[o + 1..line.len() - 1].split(';') {
                p.statement(m.trim())?;
            }
            p.statement("}")?;
            continue;
        }
        p.statement(line)?;
    }

    for id in &p.order {
        let c = &p.classes[id];
        let n = p.g.intern(id);
        let nd = &mut p.g.nodes[n];
        nd.lines.clear();
        if let Some(a) = &c.annotation {
            let t = if ascii { format!("<<{}>>", a) } else { format!("«{}»", a) };
            nd.lines.push(Line::center(&t));
        }
        nd.lines.push(Line::center(c.label.as_deref().unwrap_or(id)));
        if !c.attrs.is_empty() {
            nd.dividers.push(nd.lines.len());
            nd.lines.extend(c.attrs.iter().map(|a| Line::left(a)));
        }
        if !c.methods.is_empty() {
            nd.dividers.push(nd.lines.len());
            nd.lines.extend(c.methods.iter().map(|m| Line::left(m)));
        }
        nd.cluster = c.cluster;
    }
    for (a, b, mut e) in std::mem::take(&mut p.rels) {
        e.from = p.g.intern(&a);
        e.to = p.g.intern(&b);
        p.g.edges.push(e);
    }
    for (i, (target, text)) in std::mem::take(&mut p.notes).into_iter().enumerate() {
        let n = p.g.intern(&format!("[note]{}", i));
        let nd = &mut p.g.nodes[n];
        nd.lines = split_label(&text).iter().map(|l| Line::left(l)).collect();
        nd.shape = Shape::Rect;
        if let Some(t) = target.and_then(|t| p.g.get(&t)) {
            let mut e = Edge::new(t, n);
            e.kind = LineKind::Dotted;
            e.end = Marker::None;
            p.g.edges.push(e);
        }
    }
    if let Some(d) = dir {
        p.g.dir = d;
    }
    Ok(p.g)
}
