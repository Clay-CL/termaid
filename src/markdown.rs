//! `--md`: replace each ```mermaid fence with its rendering, pass the rest through.

use crate::detect;

pub fn process(input: &str, ascii: bool, max_width: Option<usize>) -> String {
    let lines: Vec<&str> = input.split_inclusive('\n').collect();
    let mut out = String::with_capacity(input.len() * 2);
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let Some((indent, fch, flen)) = open_fence(line) else {
            out.push_str(line);
            i += 1;
            continue;
        };
        let close = (i + 1..lines.len()).find(|&j| is_close(lines[j], fch, flen));
        let Some(close) = close else {
            // Unclosed fence: pass through untouched.
            for l in &lines[i..] {
                out.push_str(l);
            }
            break;
        };
        let body: String = lines[i + 1..close].concat();
        match detect::render(&body, ascii, max_width) {
            Ok(art) => {
                for l in art.lines() {
                    if !l.is_empty() {
                        out.push_str(indent);
                    }
                    out.push_str(l);
                    out.push('\n');
                }
                if !lines[close].ends_with('\n') && out.ends_with('\n') {
                    out.pop();
                }
            }
            Err(e) => {
                eprintln!("termaid: warning: mermaid block at line {} left unrendered: {}", i + 1, e);
                for l in &lines[i..=close] {
                    out.push_str(l);
                }
            }
        }
        i = close + 1;
    }
    out
}

/// Returns (indent, fence char, fence length) for a ```mermaid opening line.
fn open_fence(line: &str) -> Option<(&str, char, usize)> {
    let t = line.trim_start();
    let indent = &line[..line.len() - t.len()];
    let fch = t.chars().next()?;
    if fch != '`' && fch != '~' {
        return None;
    }
    let flen = t.chars().take_while(|&c| c == fch).count();
    if flen < 3 {
        return None;
    }
    let info = t[flen..].trim();
    let info = info.trim_start_matches('{').trim_end_matches('}');
    (info.split_whitespace().next() == Some("mermaid")).then_some((indent, fch, flen))
}

fn is_close(line: &str, fch: char, flen: usize) -> bool {
    let t = line.trim();
    t.len() >= flen && t.chars().all(|c| c == fch)
}

/// Raw mode: strip a surrounding ```mermaid fence if the input has one.
pub fn strip_fence(input: &str) -> &str {
    let t = input.trim_start();
    let Some(first_nl) = t.find('\n') else { return input };
    let first = &t[..first_nl];
    let Some((_, fch, flen)) = open_fence(first).or_else(|| {
        let f = first.trim();
        (f.len() >= 3 && (f.chars().all(|c| c == '`') || f.chars().all(|c| c == '~')))
            .then(|| ("", f.chars().next().unwrap(), f.len()))
    }) else {
        return input;
    };
    let body = &t[first_nl + 1..];
    let trimmed = body.trim_end();
    match trimmed.rfind('\n') {
        Some(j) if is_close(&trimmed[j + 1..], fch, flen) => &body[..j + 1],
        None if is_close(trimmed, fch, flen) => "",
        _ => body,
    }
}
