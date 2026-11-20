//! Small string helpers shared by the parsers.

/// Split a line on `;` that is outside quotes and brackets.
pub fn split_statements(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote = false;
    let mut cur = String::new();
    for c in line.chars() {
        match c {
            '"' => quote = !quote,
            '[' | '(' | '{' if !quote => depth += 1,
            ']' | ')' | '}' if !quote => depth -= 1,
            ';' if !quote && depth <= 0 => {
                let t = cur.trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }
    out
}

pub fn strip_quotes(s: &str) -> &str {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        &t[1..t.len() - 1]
    } else {
        t
    }
}

/// Decode mermaid entity codes such as `#quot;`, `#amp;` and `#35;`.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('#') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('#') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        let decoded = tail.find(';').filter(|&j| j > 0 && j <= 8).and_then(|j| {
            let name = &tail[..j];
            let c = match name {
                "quot" => Some('"'),
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "nbsp" => Some(' '),
                _ => name.parse::<u32>().ok().and_then(char::from_u32),
            };
            c.map(|c| (c, j))
        });
        match decoded {
            Some((c, j)) => {
                out.push(c);
                rest = &tail[j + 1..];
            }
            None => {
                out.push('#');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}
