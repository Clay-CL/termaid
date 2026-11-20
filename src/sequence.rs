//! `sequenceDiagram` parser and column/row layout.

use crate::canvas::{Canvas, OWNER_SOLID, Style, arrow, str_width};
use crate::canvas::Dir;
use crate::graph_layout::split_label;
use crate::text::decode_entities;

#[derive(Clone, Copy, PartialEq)]
enum Head {
    None,
    Arrow,
    Cross,
    Open,
}

enum NoteKind {
    Left,
    Right,
    Over,
}

enum Ev {
    Msg { a: usize, b: usize, label: String, dotted: bool, start: Head, end: Head },
    Note { kind: NoteKind, a: usize, b: usize, lines: Vec<String> },
    Start { title: String },
    Else { title: String },
    End,
}

struct Part {
    id: String,
    lines: Vec<String>,
    actor: bool,
}

struct Seq {
    parts: Vec<Part>,
    evs: Vec<Ev>,
}

impl Seq {
    fn part(&mut self, id: &str) -> usize {
        let id = id.trim();
        if let Some(i) = self.parts.iter().position(|p| p.id == id) {
            return i;
        }
        self.parts.push(Part { id: id.to_string(), lines: vec![id.to_string()], actor: false });
        self.parts.len() - 1
    }

    fn declare(&mut self, rest: &str, actor: bool) {
        let (id, label) = match rest.split_once(" as ") {
            Some((id, l)) => (id.trim(), decode_entities(l.trim().trim_matches('"'))),
            None => (rest.trim(), rest.trim().to_string()),
        };
        let i = self.part(id);
        self.parts[i].lines = split_label(&label);
        self.parts[i].actor = actor;
    }
}

/// Arrow tokens, longest first: (token, dotted, start head, end head)
const ARROWS: &[(&str, bool, Head, Head)] = &[
    ("<<-->>", true, Head::Arrow, Head::Arrow),
    ("<<->>", false, Head::Arrow, Head::Arrow),
    ("-->>", true, Head::None, Head::Arrow),
    ("->>", false, Head::None, Head::Arrow),
    ("--x", true, Head::None, Head::Cross),
    ("-x", false, Head::None, Head::Cross),
    ("--)", true, Head::None, Head::Open),
    ("-)", false, Head::None, Head::Open),
    ("-->", true, Head::None, Head::None),
    ("->", false, Head::None, Head::None),
];

fn parse_msg(line: &str) -> Option<(String, String, String, bool, Head, Head)> {
    let (left, label) = match line.split_once(':') {
        Some((l, r)) => (l, r.trim()),
        None => (line, ""),
    };
    let mut best: Option<(usize, &(&str, bool, Head, Head))> = None;
    for a in ARROWS {
        if let Some(i) = left.find(a.0) {
            if best.is_none_or(|(bi, ba)| i < bi || (i == bi && a.0.len() > ba.0.len())) {
                best = Some((i, a));
            }
        }
    }
    let (i, &(tok, dotted, start, end)) = best?;
    let from = left[..i].trim();
    let to = left[i + tok.len()..].trim().trim_start_matches(['+', '-']).trim();
    if from.is_empty() || to.is_empty() {
        return None;
    }
    Some((from.to_string(), to.to_string(), decode_entities(label), dotted, start, end))
}

fn parse(src: &str) -> Result<Seq, String> {
    let mut lines = src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("%%"));
    lines.next(); // header
    let mut s = Seq { parts: Vec::new(), evs: Vec::new() };
    let mut autonumber = false;
    let mut num = 0;
    // true = frame block (loop/alt/...), false = box/rect (not drawn)
    let mut blocks: Vec<bool> = Vec::new();
    for line in lines {
        let (first, rest) = match line.split_once(char::is_whitespace) {
            Some((f, r)) => (f, r.trim()),
            None => (line, ""),
        };
        match first {
            "participant" => s.declare(rest, false),
            "actor" => s.declare(rest, true),
            "create" => {
                if let Some(r) = rest.strip_prefix("participant ") {
                    s.declare(r, false);
                } else if let Some(r) = rest.strip_prefix("actor ") {
                    s.declare(r, true);
                }
            }
            "autonumber" => autonumber = rest != "off",
            "activate" | "deactivate" | "destroy" | "title" | "accTitle" | "accDescr" | "links" | "link"
            | "properties" | "details" => {}
            "box" | "rect" => blocks.push(false),
            "loop" | "alt" | "opt" | "par" | "critical" | "break" => {
                blocks.push(true);
                let title = if rest.is_empty() { first.to_string() } else { format!("{} [{}]", first, rest) };
                s.evs.push(Ev::Start { title });
            }
            "else" | "and" | "option" => {
                let title = if rest.is_empty() { first.to_string() } else { format!("{} [{}]", first, rest) };
                s.evs.push(Ev::Else { title });
            }
            "end" => {
                if blocks.pop() == Some(true) {
                    s.evs.push(Ev::End);
                }
            }
            _ if first.eq_ignore_ascii_case("note") => {
                let (pos, text) = rest.split_once(':').ok_or_else(|| format!("note without ':' in '{}'", line))?;
                let pos = pos.trim();
                let (kind, who) = if let Some(w) = pos.strip_prefix("right of") {
                    (NoteKind::Right, w)
                } else if let Some(w) = pos.strip_prefix("left of") {
                    (NoteKind::Left, w)
                } else if let Some(w) = pos.strip_prefix("over") {
                    (NoteKind::Over, w)
                } else {
                    return Err(format!("cannot parse note '{}'", line));
                };
                let mut ids = who.split(',');
                let a = s.part(ids.next().unwrap_or(""));
                let b = ids.next().map(|w| s.part(w)).unwrap_or(a);
                let lines = split_label(&decode_entities(text.trim()));
                s.evs.push(Ev::Note { kind, a, b, lines });
            }
            _ => {
                let (from, to, mut label, dotted, start, end) =
                    parse_msg(line).ok_or_else(|| format!("cannot parse '{}'", line))?;
                if autonumber {
                    num += 1;
                    label = if label.is_empty() { format!("{}.", num) } else { format!("{}. {}", num, label) };
                }
                let label = split_label(&label).join(" ");
                let a = s.part(&from);
                let b = s.part(&to);
                s.evs.push(Ev::Msg { a, b, label, dotted, start, end });
            }
        }
    }
    if s.parts.is_empty() {
        return Err("sequence diagram has no participants".into());
    }
    Ok(s)
}

fn lines_w(lines: &[String]) -> i32 {
    lines.iter().map(|l| str_width(l)).max().unwrap_or(0) as i32
}

fn head_char(h: Head, d: Dir, ascii: bool) -> char {
    match h {
        Head::Arrow => arrow(d, ascii),
        Head::Cross => if ascii { 'x' } else { '×' },
        Head::Open => match (d, ascii) {
            (Dir::Left, false) => '◁',
            (_, false) => '▷',
            (Dir::Left, true) => '<',
            _ => '>',
        },
        Head::None => ' ',
    }
}

pub fn render(src: &str, ascii: bool) -> Result<String, String> {
    let s = parse(src)?;
    let n = s.parts.len();
    let box_w: Vec<i32> = s.parts.iter().map(|p| lines_w(&p.lines) + 4).collect();
    let box_h = s.parts.iter().map(|p| p.lines.len() as i32 + 2).max().unwrap();

    // ---- column centres from pairwise minimum distances
    let mut cons: Vec<(usize, usize, i32)> = Vec::new();
    for i in 0..n.saturating_sub(1) {
        cons.push((i, i + 1, box_w[i] - box_w[i] / 2 + box_w[i + 1] / 2 + 2));
    }
    let mut right_extra = 0;
    let mut left_extra = 0;
    for ev in &s.evs {
        match ev {
            Ev::Msg { a, b, label, .. } => {
                let lw = str_width(label) as i32;
                if a == b {
                    if *a + 1 < n {
                        cons.push((*a, a + 1, lw + 7));
                    } else {
                        right_extra = right_extra.max(lw + 7);
                    }
                } else {
                    cons.push(((*a).min(*b), (*a).max(*b), lw + 4));
                }
            }
            Ev::Note { kind, a, b, lines } => {
                let nw = lines_w(lines) + 4;
                match kind {
                    NoteKind::Right if a + 1 < n => cons.push((*a, a + 1, nw + 3)),
                    NoteKind::Left if *a > 0 => cons.push((a - 1, *a, nw + 3)),
                    NoteKind::Right => right_extra = right_extra.max(nw + 3),
                    NoteKind::Left => left_extra = left_extra.max(nw + 3),
                    NoteKind::Over if a == b => {
                        if *a + 1 < n {
                            cons.push((*a, a + 1, nw / 2 + 3));
                        }
                        if *a > 0 {
                            cons.push((a - 1, *a, nw / 2 + 3));
                        }
                    }
                    NoteKind::Over => cons.push(((*a).min(*b), (*a).max(*b), nw - 4)),
                }
            }
            _ => {}
        }
    }
    let mut c = vec![0i32; n];
    for j in 1..n {
        c[j] = cons.iter().filter(|k| k.1 == j).map(|k| c[k.0] + k.2).max().unwrap_or(c[j - 1] + 1);
    }
    let depth = s.evs.iter().filter(|e| matches!(e, Ev::Start { .. })).count() as i32;
    let ox = 4 + box_w.iter().max().unwrap() + left_extra + 2 * depth;
    let _ = right_extra;
    for x in c.iter_mut() {
        *x += ox;
    }

    // ---- pass 1: rows and frame extents
    struct Frame {
        top: i32,
        bottom: i32,
        lo: i32,
        hi: i32,
        title: String,
        divs: Vec<(i32, String)>,
    }
    let mut frames: Vec<Frame> = Vec::new();
    let mut open: Vec<(usize, i32, i32)> = Vec::new(); // frame index, content lo, hi
    let mut rows: Vec<i32> = Vec::with_capacity(s.evs.len());
    let mut y = box_h;
    let widen = |open: &mut Vec<(usize, i32, i32)>, lo: i32, hi: i32| {
        if let Some(o) = open.last_mut() {
            o.1 = o.1.min(lo);
            o.2 = o.2.max(hi);
        }
    };
    // -> (x, width)
    let note_rect = |kind: &NoteKind, a: usize, b: usize, lines: &[String]| -> (i32, i32) {
        let nw = lines_w(lines) + 4;
        match kind {
            NoteKind::Right => (c[a] + 2, nw),
            NoteKind::Left => (c[a] - 1 - nw, nw),
            NoteKind::Over => {
                let (l, r) = (c[a].min(c[b]), c[a].max(c[b]));
                let w = (r - l + 5).max(nw);
                ((l + r) / 2 - w / 2, w)
            }
        }
    };
    for ev in &s.evs {
        rows.push(y);
        match ev {
            Ev::Msg { a, b, label, .. } => {
                if a == b {
                    widen(&mut open, c[*a], c[*a] + 5 + str_width(label) as i32);
                    y += 3;
                } else {
                    widen(&mut open, c[*a].min(c[*b]), c[*a].max(c[*b]));
                    y += 2;
                }
            }
            Ev::Note { kind, a, b, lines } => {
                let (x, w) = note_rect(kind, *a, *b, lines);
                widen(&mut open, x, x + w - 1);
                y += lines.len() as i32 + 3;
            }
            Ev::Start { title } => {
                frames.push(Frame { top: y, bottom: y, lo: 0, hi: 0, title: title.clone(), divs: Vec::new() });
                open.push((frames.len() - 1, i32::MAX, i32::MIN));
                y += 1;
            }
            Ev::Else { title } => {
                if let Some(o) = open.last() {
                    frames[o.0].divs.push((y, title.clone()));
                }
                y += 1;
            }
            Ev::End => {
                if let Some((fi, mut lo, mut hi)) = open.pop() {
                    if lo > hi {
                        lo = c[0];
                        hi = c[0];
                    }
                    let f = &mut frames[fi];
                    f.bottom = y;
                    f.lo = lo - 3;
                    f.hi = (hi + 3).max(f.lo + str_width(&f.title) as i32 + 4);
                    for d in &f.divs {
                        f.hi = f.hi.max(f.lo + str_width(&d.1) as i32 + 6);
                    }
                    let (l, h) = (f.lo, f.hi);
                    widen(&mut open, l, h);
                }
                y += 1;
            }
        }
    }
    // Unclosed frames end at the bottom.
    while let Some((fi, lo, hi)) = open.pop() {
        let f = &mut frames[fi];
        f.bottom = y;
        f.lo = if lo > hi { c[0] - 3 } else { lo - 3 };
        f.hi = (if lo > hi { c[0] } else { hi } + 3).max(f.lo + str_width(&f.title) as i32 + 4);
        y += 1;
    }
    let foot = y + 1;

    // ---- draw
    let mut cv = Canvas::new(ascii);
    for (i, p) in s.parts.iter().enumerate() {
        let style = if p.actor { Style::Round } else { Style::Light };
        let x = c[i] - box_w[i] / 2;
        for top in [0, foot] {
            cv.rect(x, top, box_w[i], box_h, style, OWNER_SOLID);
            for (k, l) in p.lines.iter().enumerate() {
                let tx = x + 1 + (box_w[i] - 2 - str_width(l) as i32) / 2;
                cv.text(tx, top + 1 + k as i32, l, OWNER_SOLID);
            }
        }
        cv.segment(c[i], box_h - 1, c[i], foot, Style::Light, OWNER_SOLID);
    }
    for f in &frames {
        cv.rect(f.lo, f.top, f.hi - f.lo + 1, f.bottom - f.top + 1, Style::Light, OWNER_SOLID);
        for (dy, _) in &f.divs {
            cv.segment(f.lo, *dy, f.hi, *dy, Style::Dotted, OWNER_SOLID);
        }
    }
    let mut texts: Vec<(i32, i32, String)> = Vec::new();
    for (ev, &y) in s.evs.iter().zip(&rows) {
        match ev {
            Ev::Msg { a, b, label, dotted, start, end } => {
                let style = if *dotted { Style::Dotted } else { Style::Light };
                let (ca, cb) = (c[*a], c[*b]);
                if a == b {
                    let y1 = y + 1;
                    cv.polyline(&[(ca, y1), (ca + 3, y1), (ca + 3, y1 + 1), (ca, y1 + 1)], style, OWNER_SOLID);
                    if *end != Head::None {
                        cv.put(ca + 1, y1 + 1, head_char(*end, Dir::Left, ascii), OWNER_SOLID);
                    }
                    texts.push((ca + 5, y1, label.clone()));
                } else {
                    let ly = y + 1;
                    let dir = if cb > ca { Dir::Right } else { Dir::Left };
                    let step = if cb > ca { 1 } else { -1 };
                    // A line with a head stops at the head, leaving the lifeline plain.
                    let x0 = if *start != Head::None { ca + step } else { ca };
                    let x1 = if *end != Head::None { cb - step } else { cb };
                    cv.segment(x0, ly, x1, ly, style, OWNER_SOLID);
                    if *end != Head::None {
                        cv.put(cb - step, ly, head_char(*end, dir, ascii), OWNER_SOLID);
                    }
                    if *start != Head::None {
                        let back = if dir == Dir::Right { Dir::Left } else { Dir::Right };
                        cv.put(ca + step, ly, head_char(*start, back, ascii), OWNER_SOLID);
                    }
                    let lw = str_width(label) as i32;
                    let (l, r) = (ca.min(cb), ca.max(cb));
                    texts.push((l + (r - l - lw + 1) / 2, y, label.clone()));
                }
            }
            Ev::Note { kind, a, b, lines } => {
                let (x, w) = note_rect(kind, *a, *b, lines);
                let h = lines.len() as i32 + 2;
                cv.clear(x, y + 1, w, h);
                cv.rect(x, y + 1, w, h, Style::Light, OWNER_SOLID);
                for (k, l) in lines.iter().enumerate() {
                    texts.push((x + 2, y + 2 + k as i32, l.clone()));
                }
            }
            _ => {}
        }
    }
    for f in &frames {
        texts.push((f.lo + 1, f.top, format!(" {} ", f.title)));
        for (dy, t) in &f.divs {
            texts.push((f.lo + 2, *dy, format!(" {} ", t)));
        }
    }
    for (x, y, t) in texts {
        if !t.trim().is_empty() {
            cv.text(x, y, &t, OWNER_SOLID);
        }
    }
    Ok(cv.render())
}
