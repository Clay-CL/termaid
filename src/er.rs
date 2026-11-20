//! `erDiagram` parser. Cardinality is folded into the edge label, e.g.
//! `CUSTOMER ||--o{ ORDER : places` -> "places [1 : 0..N]".

use crate::canvas::str_width;
use crate::graph_layout::{Direction, Edge, Graph, Line, LineKind, Marker};
use crate::text::strip_quotes;
use std::collections::HashMap;

fn card(s: &str, left: bool) -> Option<&'static str> {
    Some(match (s, left) {
        ("||", _) => "1",
        ("|o", true) | ("o|", false) => "0..1",
        ("}|", true) | ("|{", false) => "1..N",
        ("}o", true) | ("o{", false) => "0..N",
        _ => return None,
    })
}

/// `p[Person]` / `"Name"` -> (id, label)
fn entity(s: &str) -> (String, String) {
    let s = s.trim();
    if let Some(b) = s.find('[') {
        let label = strip_quotes(s[b + 1..].trim_end_matches(']')).to_string();
        return (s[..b].trim().to_string(), label);
    }
    let id = strip_quotes(s).to_string();
    (id.clone(), id)
}

pub fn parse(src: &str, dir: Option<Direction>) -> Result<Graph, String> {
    let mut lines = src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("%%"));
    lines.next(); // header
    let mut g = Graph::new(Direction::TD);
    let mut attrs: HashMap<usize, Vec<Vec<String>>> = HashMap::new();
    let mut open: Option<usize> = None;
    let mut labels: HashMap<usize, String> = HashMap::new();
    let ent = |g: &mut Graph, s: &str, labels: &mut HashMap<usize, String>| {
        let (id, label) = entity(s);
        let n = g.intern(&id);
        if label != id || !labels.contains_key(&n) {
            labels.insert(n, label);
        }
        n
    };
    for line in lines {
        if let Some(n) = open {
            if line == "}" {
                open = None;
            } else {
                // type name [PK|FK|UK[, ...]] ["comment"]
                let (main, comment) = match line.find('"') {
                    Some(q) => (&line[..q], Some(strip_quotes(&line[q..]).to_string())),
                    None => (line, None),
                };
                let mut cols: Vec<String> = main.split_whitespace().map(str::to_string).collect();
                if cols.len() > 3 {
                    let keys = cols[2..].join(" ");
                    cols.truncate(2);
                    cols.push(keys);
                }
                while cols.len() < 3 {
                    cols.push(String::new());
                }
                cols.push(comment.unwrap_or_default());
                attrs.entry(n).or_default().push(cols);
            }
            continue;
        }
        let first = line.split_whitespace().next().unwrap_or("");
        match first {
            "direction" => {
                if let Some(d) = line.split_whitespace().nth(1).and_then(Direction::parse) {
                    g.dir = d;
                }
                continue;
            }
            "classDef" | "class" | "style" | "accTitle" | "accDescr" | "title" => continue,
            _ => {}
        }
        if let Some(name) = line.strip_suffix('{') {
            open = Some(ent(&mut g, name, &mut labels));
            continue;
        }
        let (body, label) = match line.split_once(':') {
            Some((b, l)) => (b, strip_quotes(l.trim()).to_string()),
            None => (line, String::new()),
        };
        let pos = [body.find("--"), body.find("..")].into_iter().flatten().min();
        match pos {
            Some(i) if i >= 2 && body.len() >= i + 4 => {
                let lc = card(&body[i - 2..i], true);
                let rc = card(&body[i + 2..i + 4], false);
                let (Some(lc), Some(rc)) = (lc, rc) else {
                    return Err(format!("cannot parse relationship '{}'", line));
                };
                let a = ent(&mut g, &body[..i - 2], &mut labels);
                let b = ent(&mut g, &body[i + 4..], &mut labels);
                let mut e = Edge::new(a, b);
                e.end = Marker::None;
                e.kind = if body[i..].starts_with("..") { LineKind::Dotted } else { LineKind::Normal };
                let card = format!("[{} : {}]", lc, rc);
                e.label = Some(if label.is_empty() { card } else { format!("{} {}", label, card) });
                g.edges.push(e);
            }
            _ => {
                if line.contains(char::is_whitespace) && !line.contains('[') {
                    return Err(format!("cannot parse '{}'", line));
                }
                ent(&mut g, line, &mut labels);
            }
        }
    }
    for (n, label) in labels {
        g.nodes[n].lines = vec![Line::center(&label)];
    }
    for (n, rows) in attrs {
        // Align columns: type, name, keys, comment
        let mut w = [0usize; 4];
        for r in &rows {
            for (i, c) in r.iter().enumerate() {
                w[i] = w[i].max(str_width(c));
            }
        }
        let nd = &mut g.nodes[n];
        nd.dividers = vec![1];
        for r in rows {
            let mut s = String::new();
            for (i, c) in r.iter().enumerate() {
                if w[i] == 0 {
                    continue;
                }
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(c);
                s.push_str(&" ".repeat(w[i] - str_width(c)));
            }
            nd.lines.push(Line::left(s.trim_end()));
        }
    }
    if let Some(d) = dir {
        g.dir = d;
    }
    Ok(g)
}
