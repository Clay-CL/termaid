//! Character grid with line-junction merging and Unicode/ASCII charsets.

pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;

/// Owner ids for label collision checks; edges use `OWNER_EDGE0 + edge_index`.
pub const OWNER_NONE: u32 = 0;
pub const OWNER_SOLID: u32 = 1;
pub const OWNER_MULTI: u32 = u32::MAX;
pub const OWNER_EDGE0: u32 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Light,
    Round,
    Dotted,
    Thick,
    Double,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cell {
    Empty,
    Ch(char),
    /// Right half of a double-width character.
    Cont,
    Line { mask: u8, style: Style, base: u8 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

pub struct Canvas {
    rows: Vec<Vec<Cell>>,
    owners: Vec<Vec<u32>>,
    pub ascii: bool,
}

impl Canvas {
    pub fn new(ascii: bool) -> Self {
        Canvas { rows: Vec::new(), owners: Vec::new(), ascii }
    }

    fn ensure(&mut self, x: usize, y: usize) {
        if self.rows.len() <= y {
            self.rows.resize(y + 1, Vec::new());
            self.owners.resize(y + 1, Vec::new());
        }
        let row = &mut self.rows[y];
        if row.len() <= x {
            row.resize(x + 1, Cell::Empty);
            self.owners[y].resize(x + 1, OWNER_NONE);
        }
    }

    fn get(&self, x: i32, y: i32) -> Cell {
        if x < 0 || y < 0 {
            return Cell::Empty;
        }
        self.rows
            .get(y as usize)
            .and_then(|r| r.get(x as usize))
            .copied()
            .unwrap_or(Cell::Empty)
    }

    pub fn owner(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 {
            return OWNER_SOLID;
        }
        self.owners
            .get(y as usize)
            .and_then(|r| r.get(x as usize))
            .copied()
            .unwrap_or(OWNER_NONE)
    }

    pub fn is_empty(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && self.get(x, y) == Cell::Empty
    }

    /// True if the cell is a straight line run (no junction) along the given axis.
    pub fn is_straight(&self, x: i32, y: i32, horizontal: bool) -> bool {
        match self.get(x, y) {
            Cell::Line { mask, .. } => {
                if horizontal {
                    mask == LEFT | RIGHT
                } else {
                    mask == UP | DOWN
                }
            }
            _ => false,
        }
    }

    fn set_owner(&mut self, x: usize, y: usize, owner: u32) {
        let o = &mut self.owners[y][x];
        if *o == OWNER_NONE || *o == owner {
            *o = owner;
        } else {
            *o = OWNER_MULTI;
        }
    }

    /// Add line bits to a cell. Never overwrites text characters.
    pub fn line(&mut self, x: i32, y: i32, bits: u8, style: Style, owner: u32) {
        if x < 0 || y < 0 {
            return;
        }
        let (ux, uy) = (x as usize, y as usize);
        self.ensure(ux, uy);
        let cell = &mut self.rows[uy][ux];
        match *cell {
            Cell::Empty => *cell = Cell::Line { mask: bits, style, base: bits },
            Cell::Line { mask, style: old, base } => {
                let merged = if old == style || old == Style::Double {
                    old
                } else if old == Style::Round || style == Style::Round {
                    Style::Light
                } else if mask | bits == mask {
                    old
                } else {
                    Style::Light
                };
                let base = if style == old { base | bits } else { base };
                *cell = Cell::Line { mask: mask | bits, style: merged, base };
            }
            Cell::Ch(_) | Cell::Cont => return,
        }
        self.set_owner(ux, uy, owner);
    }

    /// Draw a straight segment between two cells (same row or column).
    pub fn segment(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, style: Style, owner: u32) {
        if x0 == x1 && y0 == y1 {
            return;
        }
        if y0 == y1 {
            let (a, b) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
            for x in a..=b {
                let mut bits = 0;
                if x > a {
                    bits |= LEFT;
                }
                if x < b {
                    bits |= RIGHT;
                }
                self.line(x, y0, bits, style, owner);
            }
        } else {
            let (a, b) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
            for y in a..=b {
                let mut bits = 0;
                if y > a {
                    bits |= UP;
                }
                if y < b {
                    bits |= DOWN;
                }
                self.line(x0, y, bits, style, owner);
            }
        }
    }

    /// Draw an axis-aligned polyline.
    pub fn polyline(&mut self, pts: &[(i32, i32)], style: Style, owner: u32) {
        for w in pts.windows(2) {
            self.segment(w[0].0, w[0].1, w[1].0, w[1].1, style, owner);
        }
    }

    /// Rectangle outline.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, style: Style, owner: u32) {
        let (r, b) = (x + w - 1, y + h - 1);
        self.polyline(&[(x, y), (r, y), (r, b), (x, b), (x, y)], style, owner);
    }

    /// Put one character, overwriting whatever is there. Returns its width.
    pub fn put(&mut self, x: i32, y: i32, c: char, owner: u32) -> i32 {
        let w = char_width(c) as i32;
        if x < 0 || y < 0 || w == 0 {
            return w;
        }
        let (ux, uy) = (x as usize, y as usize);
        self.ensure(ux + w as usize - 1, uy);
        self.rows[uy][ux] = Cell::Ch(c);
        self.owners[uy][ux] = owner;
        if w == 2 {
            self.rows[uy][ux + 1] = Cell::Cont;
            self.owners[uy][ux + 1] = owner;
        }
        w
    }

    pub fn text(&mut self, x: i32, y: i32, s: &str, owner: u32) {
        let mut cx = x;
        for c in s.chars() {
            cx += self.put(cx, y, c, owner);
        }
    }

    /// Blank a rectangle (e.g. under a note box).
    pub fn clear(&mut self, x: i32, y: i32, w: i32, h: i32) {
        for yy in y.max(0)..y + h {
            for xx in x.max(0)..x + w {
                let (ux, uy) = (xx as usize, yy as usize);
                if let Some(c) = self.rows.get_mut(uy).and_then(|r| r.get_mut(ux)) {
                    *c = Cell::Empty;
                    self.owners[uy][ux] = OWNER_NONE;
                }
            }
        }
    }

    pub fn render(&self) -> String {
        let mut lines: Vec<String> = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            let mut s = String::with_capacity(row.len() * 3);
            for &cell in row {
                match cell {
                    Cell::Empty => s.push(' '),
                    Cell::Ch(c) => s.push(c),
                    Cell::Cont => {}
                    Cell::Line { mask, style, base } => s.push(line_char(mask, style, base, self.ascii)),
                }
            }
            let t = s.trim_end().len();
            s.truncate(t);
            lines.push(s);
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let first = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
        let lines = &lines[first..];
        let indent = lines
            .iter()
            .filter(|l| !l.is_empty())
            .map(|l| l.len() - l.trim_start_matches(' ').len())
            .min()
            .unwrap_or(0);
        let mut out = String::new();
        for l in lines {
            if l.len() >= indent {
                out.push_str(&l[indent..]);
            }
            out.push('\n');
        }
        out
    }
}

fn line_char(mask: u8, style: Style, base: u8, ascii: bool) -> char {
    let horiz = mask & (UP | DOWN) == 0;
    let vert = mask & (LEFT | RIGHT) == 0;
    if ascii {
        return match (horiz, vert) {
            (true, _) => match style {
                Style::Dotted => '.',
                Style::Thick | Style::Double => '=',
                _ => '-',
            },
            (_, true) => match style {
                Style::Dotted => ':',
                _ => '|',
            },
            _ => {
                if style == Style::Round && (mask == DOWN | RIGHT || mask == DOWN | LEFT) {
                    '.'
                } else if style == Style::Round && (mask == UP | RIGHT || mask == UP | LEFT) {
                    '\''
                } else {
                    '+'
                }
            }
        };
    }
    if horiz {
        return match style {
            Style::Dotted => '┄',
            Style::Thick => '━',
            Style::Double => '═',
            _ => '─',
        };
    }
    if vert {
        return match style {
            Style::Dotted => '┆',
            Style::Thick => '┃',
            Style::Double => '║',
            _ => '│',
        };
    }
    const LIGHT: [char; 16] = [
        ' ', '│', '│', '│', '─', '┘', '┐', '┤', '─', '└', '┌', '├', '─', '┴', '┬', '┼',
    ];
    match style {
        Style::Round => match mask {
            m if m == DOWN | RIGHT => '╭',
            m if m == DOWN | LEFT => '╮',
            m if m == UP | RIGHT => '╰',
            m if m == UP | LEFT => '╯',
            _ => LIGHT[mask as usize],
        },
        Style::Thick => {
            const HEAVY: [char; 16] = [
                ' ', '┃', '┃', '┃', '━', '┛', '┓', '┫', '━', '┗', '┏', '┣', '━', '┻', '┳', '╋',
            ];
            HEAVY[mask as usize]
        }
        Style::Double => {
            if mask == base {
                const DOUBLE: [char; 16] = [
                    ' ', '║', '║', '║', '═', '╝', '╗', '╣', '═', '╚', '╔', '╠', '═', '╩', '╦', '╬',
                ];
                DOUBLE[mask as usize]
            } else if base == LEFT | RIGHT {
                match mask & (UP | DOWN) {
                    UP => '╧',
                    DOWN => '╤',
                    _ => '╪',
                }
            } else if base == UP | DOWN {
                match mask & (LEFT | RIGHT) {
                    LEFT => '╢',
                    RIGHT => '╟',
                    _ => '╫',
                }
            } else {
                LIGHT[mask as usize]
            }
        }
        _ => LIGHT[mask as usize],
    }
}

/// Terminal display width of a char (0, 1 or 2 columns).
pub fn char_width(c: char) -> usize {
    let u = c as u32;
    if u < 0x300 {
        return if u < 0x20 || (0x7f..0xa0).contains(&u) { 0 } else { 1 };
    }
    // Zero-width: combining marks, ZWJ/ZWSP, variation selectors.
    if (0x300..=0x36f).contains(&u)
        || (0x200b..=0x200f).contains(&u)
        || (0xfe00..=0xfe0f).contains(&u)
        || (0x1ab0..=0x1aff).contains(&u)
        || (0x20d0..=0x20ff).contains(&u)
    {
        return 0;
    }
    let wide = (0x1100..=0x115f).contains(&u)
        || (0x2e80..=0x303e).contains(&u)
        || (0x3041..=0x33ff).contains(&u)
        || (0x3400..=0x4dbf).contains(&u)
        || (0x4e00..=0x9fff).contains(&u)
        || (0xa000..=0xa4cf).contains(&u)
        || (0xac00..=0xd7a3).contains(&u)
        || (0xf900..=0xfaff).contains(&u)
        || (0xfe30..=0xfe4f).contains(&u)
        || (0xff00..=0xff60).contains(&u)
        || (0xffe0..=0xffe6).contains(&u)
        || (0x1f300..=0x1f64f).contains(&u)
        || (0x1f900..=0x1f9ff).contains(&u)
        || (0x1fa70..=0x1faff).contains(&u)
        || (0x20000..=0x3fffd).contains(&u);
    if wide { 2 } else { 1 }
}

pub fn str_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Direction-aware marker glyphs.
pub fn arrow(d: Dir, ascii: bool) -> char {
    match (d, ascii) {
        (Dir::Up, false) => '▲',
        (Dir::Down, false) => '▼',
        (Dir::Left, false) => '◄',
        (Dir::Right, false) => '►',
        (Dir::Up, true) => '^',
        (Dir::Down, true) => 'v',
        (Dir::Left, true) => '<',
        (Dir::Right, true) => '>',
    }
}

pub fn triangle(d: Dir, ascii: bool) -> char {
    match (d, ascii) {
        (Dir::Up, false) => '△',
        (Dir::Down, false) => '▽',
        (Dir::Left, false) => '◁',
        (Dir::Right, false) => '▷',
        _ => arrow(d, true),
    }
}
