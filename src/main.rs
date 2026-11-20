mod canvas;
mod class;
mod detect;
mod er;
mod flowchart;
mod graph_layout;
mod markdown;
mod sequence;
mod state;
mod text;

use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

const USAGE: &str = "\
termaid - render mermaid diagrams as text

Usage: termaid [OPTIONS] [FILE]

Reads mermaid source from FILE (or stdin when FILE is omitted or '-')
and prints the diagram as Unicode box-drawing text. Run with no FILE
at a terminal (nothing piped in), it prints this help and exits 2.

Options:
  --md               Treat input as markdown: replace every ```mermaid
                     fence with its rendering, pass other text through
  --ascii            Draw with plain ASCII (+-|<>^v) instead of Unicode
  --max-width N      If the diagram is wider than N columns, re-layout
                     LR/RL graphs top-down; warn if still too wide
  -h, --help         Show this help
  -V, --version      Show version

Supported: flowchart/graph, sequenceDiagram, stateDiagram(-v2),
classDiagram, erDiagram.
";

fn main() -> ExitCode {
    let mut ascii = false;
    let mut md = false;
    let mut max_width: Option<usize> = None;
    let mut file: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--ascii" => ascii = true,
            "--md" => md = true,
            "-h" | "--help" => {
                print!("{}", USAGE);
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("termaid {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--max-width" => match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => max_width = Some(n),
                None => return fail("--max-width needs a number"),
            },
            s if s.starts_with("--max-width=") => match s["--max-width=".len()..].parse() {
                Ok(n) => max_width = Some(n),
                Err(_) => return fail("--max-width needs a number"),
            },
            s if s.starts_with('-') && s != "-" => return fail(&format!("unknown option '{}' (see --help)", s)),
            s => {
                if file.is_some() {
                    return fail("only one input file is supported");
                }
                file = Some(s.to_string());
            }
        }
    }

    // Run plainly at a terminal: show usage instead of waiting on stdin.
    if file.is_none() && std::io::stdin().is_terminal() {
        eprint!("{}", USAGE);
        return ExitCode::from(2);
    }

    let mut input = String::new();
    let read = match file.as_deref() {
        None | Some("-") => std::io::stdin().read_to_string(&mut input).map(|_| ()),
        Some(path) => std::fs::read_to_string(path).map(|s| input = s),
    };
    if let Err(e) = read {
        return fail(&format!("cannot read input: {}", e));
    }

    let out = if md {
        markdown::process(&input, ascii, max_width)
    } else {
        match detect::render(markdown::strip_fence(&input), ascii, max_width) {
            Ok(s) => s,
            Err(e) => return fail(&e),
        }
    };
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
    ExitCode::SUCCESS
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("termaid: error: {}", msg);
    ExitCode::from(1)
}
