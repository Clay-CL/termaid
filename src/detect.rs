//! Pick the diagram type from the first keyword and render it.

use crate::canvas::str_width;
use crate::graph_layout::{self, Direction, Graph};
use crate::{class, er, flowchart, sequence, state};

/// Drop a leading `---` YAML front-matter block.
fn strip_front_matter(src: &str) -> &str {
    let t = src.trim_start();
    if let Some(rest) = t.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            return after.find('\n').map_or("", |i| &after[i + 1..]);
        }
    }
    src
}

fn width(s: &str) -> usize {
    s.lines().map(str_width).max().unwrap_or(0)
}

fn warn_width(w: usize, max: usize) {
    eprintln!("termaid: warning: diagram is {} columns wide, exceeds --max-width {}", w, max);
}

fn render_graph(
    build: impl Fn(Option<Direction>) -> Result<Graph, String>,
    ascii: bool,
    max_width: Option<usize>,
) -> Result<String, String> {
    let g = build(None)?;
    let horiz = g.dir.horizontal();
    let out = graph_layout::render(&g, ascii);
    let Some(max) = max_width else { return Ok(out) };
    if width(&out) <= max {
        return Ok(out);
    }
    if horiz {
        let flipped = graph_layout::render(&build(Some(Direction::TD))?, ascii);
        if width(&flipped) <= max {
            return Ok(flipped);
        }
        warn_width(width(&flipped), max);
        return Ok(flipped);
    }
    warn_width(width(&out), max);
    Ok(out)
}

pub fn render(src: &str, ascii: bool, max_width: Option<usize>) -> Result<String, String> {
    let body = strip_front_matter(src);
    let kw = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
        .and_then(|l| l.split(|c: char| c.is_whitespace() || c == ';').next())
        .ok_or("empty input")?;
    match kw {
        "flowchart" | "graph" | "flowchart-elk" => {
            render_graph(|d| flowchart::parse(body, d), ascii, max_width)
        }
        "stateDiagram" | "stateDiagram-v2" => render_graph(|d| state::parse(body, d, ascii), ascii, max_width),
        "classDiagram" | "classDiagram-v2" => render_graph(|d| class::parse(body, d, ascii), ascii, max_width),
        "erDiagram" => render_graph(|d| er::parse(body, d), ascii, max_width),
        "sequenceDiagram" => {
            let out = sequence::render(body, ascii)?;
            if let Some(max) = max_width {
                if width(&out) > max {
                    warn_width(width(&out), max);
                }
            }
            Ok(out)
        }
        other => Err(format!("unsupported diagram type '{}'", other)),
    }
}
