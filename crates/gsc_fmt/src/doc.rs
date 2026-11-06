//! A Wadler/Prettier-style document IR and its line-fitting printer.
//!
//! A document is a tree of [`Node`]s held in an arena ([`Doc`]). `Group` is printed flat (every
//! `Line` a space, every `SoftLine` nothing) when its contents fit in the rest of the line, and
//! broken (every direct `Line`/`SoftLine` a line break) otherwise. `Fill` breaks only the
//! separators whose next item would not fit. `Hard` always breaks and breaks every enclosing
//! group. The algorithm is the one of Prettier's `printDocToString`, including its look-ahead
//! into the commands that follow a group.

/// The index of a node in a [`Doc`].
pub type Id = u32;

/// A separator, before it is resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sep {
    /// Nothing.
    None,
    /// Nothing when flat, a line break when broken.
    SoftLine,
    /// One space.
    Space,
    /// A space when flat, a line break when broken.
    Line,
    /// Always a line break.
    Hard,
    /// A line break, keeping one or more blank lines of the source (up to the configured
    /// maximum). The blank lines themselves are decided when the separator is resolved.
    HardKeep,
}

#[derive(Clone, Debug)]
pub enum Node<'a> {
    /// Text without line breaks.
    Text(&'a str),
    /// Text that does not count when measuring a line: a comment that ends the line, and the
    /// blanks before it.
    Suffix(&'a str),
    /// Text printed as is; may hold line breaks (multi-line comments, verbatim regions).
    Raw(&'a str),
    Space,
    Line,
    SoftLine,
    /// A line break followed by `blank` empty lines, then the indentation.
    Hard {
        blank: u8,
    },
    /// A line break followed by `blank` empty lines and no indentation (preprocessor lines and
    /// verbatim regions bring their own).
    Literal {
        blank: u8,
    },
    Concat(Vec<Id>),
    Group {
        children: Vec<Id>,
        broken: bool,
    },
    Indent(Vec<Id>),
    /// Alternating items and separators: `[item, sep, item, sep, item]`.
    Fill(Vec<Id>),
}

/// A document under construction: an arena of nodes and a stack of open containers.
pub struct Doc<'a> {
    pub nodes: Vec<Node<'a>>,
    /// Open containers: (node id, children so far).
    stack: Vec<(Id, Vec<Id>)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Container {
    Concat,
    Group,
    Indent,
    Fill,
}

impl<'a> Doc<'a> {
    pub fn new() -> Doc<'a> {
        let mut d = Doc {
            nodes: Vec::with_capacity(1024),
            stack: Vec::new(),
        };
        d.nodes.push(Node::Concat(Vec::new()));
        d.stack.push((0, Vec::new()));
        d
    }

    /// Add a node to the current container.
    pub fn push(&mut self, node: Node<'a>) -> Id {
        let id = self.nodes.len() as Id;
        self.nodes.push(node);
        self.stack.last_mut().expect("an open container").1.push(id);
        id
    }

    /// Replace a leaf node (a separator placeholder) in place.
    pub fn set(&mut self, id: Id, node: Node<'a>) {
        self.nodes[id as usize] = node;
    }

    /// Add a node that is not yet attached to any container (to be used in [`Doc::set`]).
    pub fn detached(&mut self, node: Node<'a>) -> Id {
        let id = self.nodes.len() as Id;
        self.nodes.push(node);
        id
    }

    /// The id of the current container.
    pub fn current(&self) -> Id {
        self.stack.last().expect("an open container").0
    }

    /// Is container `id` still open (the current one or one of its ancestors)?
    pub fn is_open(&self, id: Id) -> bool {
        self.stack.iter().any(|(c, _)| *c == id)
    }

    pub fn open(&mut self, kind: Container) -> Id {
        let node = match kind {
            Container::Concat => Node::Concat(Vec::new()),
            Container::Group => Node::Group {
                children: Vec::new(),
                broken: false,
            },
            Container::Indent => Node::Indent(Vec::new()),
            Container::Fill => Node::Fill(Vec::new()),
        };
        let id = self.push(node);
        self.stack.push((id, Vec::new()));
        id
    }

    pub fn close(&mut self) {
        let (id, kids) = self.stack.pop().expect("an open container");
        match &mut self.nodes[id as usize] {
            Node::Concat(c) | Node::Indent(c) | Node::Fill(c) => *c = kids,
            Node::Group { children, .. } => *children = kids,
            _ => unreachable!("not a container"),
        }
    }

    /// Close every container and return the root.
    pub fn finish(mut self) -> Doc<'a> {
        while self.stack.len() > 1 {
            self.close();
        }
        let (_, kids) = self.stack.pop().expect("the root");
        self.nodes[0] = Node::Concat(kids);
        propagate_breaks(&mut self.nodes);
        self
    }
}

fn children<'n>(node: &'n Node<'_>) -> &'n [Id] {
    match node {
        Node::Concat(c) | Node::Indent(c) | Node::Fill(c) => c,
        Node::Group { children, .. } => children,
        _ => &[],
    }
}

/// Mark every group that holds a hard line break (or multi-line raw text) as broken.
fn propagate_breaks(nodes: &mut [Node<'_>]) {
    // Children always have larger ids than their container except placeholders replaced by
    // `set` with detached nodes, which also come later. So a reverse scan sees children first.
    let mut hard = vec![false; nodes.len()];
    for i in (0..nodes.len()).rev() {
        let h = match &nodes[i] {
            Node::Hard { .. } | Node::Literal { .. } => true,
            Node::Raw(s) => s.contains(['\n', '\r']),
            n => children(n).iter().any(|&c| hard[c as usize]),
        };
        hard[i] = h;
        if h {
            if let Node::Group { broken, .. } = &mut nodes[i] {
                *broken = true;
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Flat,
    Break,
}

#[derive(Clone, Copy, Debug)]
struct Cmd {
    indent: u32,
    mode: Mode,
    id: Id,
    /// For a `Fill`: the index of its first child still to print.
    from: u32,
}

/// How to print a [`Doc`].
pub struct PrintOptions {
    pub width: usize,
    /// The text of one indentation level.
    pub indent: String,
    /// The width of one indentation level, and of a tab inside text.
    pub indent_width: usize,
    pub tab_width: usize,
    pub newline: &'static str,
}

fn text_width(s: &str, tab: usize) -> usize {
    s.chars().map(|c| if c == '\t' { tab } else { 1 }).sum()
}

/// The width of the last line of `s` and whether `s` holds a line break; for a multi-line text
/// also the width of its first line.
fn raw_widths(s: &str, tab: usize) -> (usize, usize, bool) {
    match s.rfind(['\n', '\r']) {
        None => {
            let w = text_width(s, tab);
            (w, w, false)
        }
        Some(last) => {
            let first = s.find(['\n', '\r']).unwrap_or(0);
            (
                text_width(&s[..first], tab),
                text_width(&s[last + 1..], tab),
                true,
            )
        }
    }
}

struct Printer<'d, 'a> {
    nodes: &'d [Node<'a>],
    o: &'d PrintOptions,
    out: String,
    pos: usize,
    /// The end of the last text pushed: trimming before a line break stops there.
    protect: usize,
}

impl Printer<'_, '_> {
    fn newline(&mut self, blank: u8, indent: Option<u32>) {
        // No trailing blanks before a line break.
        let trimmed = self
            .out
            .trim_end_matches([' ', '\t'])
            .len()
            .max(self.protect);
        self.out.truncate(trimmed);
        for _ in 0..=blank {
            self.out.push_str(self.o.newline);
        }
        self.pos = 0;
        if let Some(level) = indent {
            for _ in 0..level {
                self.out.push_str(&self.o.indent);
            }
            self.pos = level as usize * self.o.indent_width;
        }
    }

    /// Does `next` fit in `width` columns, followed by the commands of `rest` (from the top of
    /// the stack) up to their first line break? With `must_be_flat`, a broken group fails.
    fn fits(&self, next: &[Cmd], rest: &[Cmd], width: isize, must_be_flat: bool) -> bool {
        let mut remaining = width;
        let mut rest_idx = rest.len();
        let mut in_rest = false;
        let mut stack: Vec<(Mode, Id, u32)> =
            next.iter().rev().map(|c| (c.mode, c.id, c.from)).collect();
        loop {
            if remaining < 0 {
                return false;
            }
            let Some((mode, id, from)) = stack.pop() else {
                if rest_idx == 0 {
                    return true;
                }
                rest_idx -= 1;
                in_rest = true;
                let c = rest[rest_idx];
                stack.push((c.mode, c.id, c.from));
                continue;
            };
            match &self.nodes[id as usize] {
                Node::Text(s) => remaining -= text_width(s, self.o.tab_width) as isize,
                Node::Suffix(_) => {}
                Node::Raw(s) => {
                    let (first, _, multi) = raw_widths(s, self.o.tab_width);
                    remaining -= first as isize;
                    if multi {
                        return remaining >= 0;
                    }
                }
                Node::Space => remaining -= 1,
                Node::Line => {
                    if mode == Mode::Break {
                        return true;
                    }
                    remaining -= 1;
                }
                Node::SoftLine => {
                    if mode == Mode::Break {
                        return true;
                    }
                }
                Node::Hard { .. } | Node::Literal { .. } => return true,
                Node::Concat(c) | Node::Indent(c) => {
                    for &k in c.iter().rev() {
                        stack.push((mode, k, 0));
                    }
                }
                Node::Fill(c) => {
                    for &k in c[from as usize..].iter().rev() {
                        stack.push((mode, k, 0));
                    }
                }
                Node::Group { children, broken } => {
                    if must_be_flat && !in_rest && *broken {
                        return false;
                    }
                    let m = if *broken { Mode::Break } else { mode };
                    for &k in children.iter().rev() {
                        stack.push((m, k, 0));
                    }
                }
            }
        }
    }

    fn print(&mut self, root: Id) {
        let mut cmds = vec![Cmd {
            indent: 0,
            mode: Mode::Break,
            id: root,
            from: 0,
        }];
        while let Some(cmd) = cmds.pop() {
            let Cmd {
                indent, mode, id, ..
            } = cmd;
            match &self.nodes[id as usize] {
                Node::Text(s) | Node::Suffix(s) => {
                    self.out.push_str(s);
                    self.pos += text_width(s, self.o.tab_width);
                    if !s.trim_matches([' ', '\t']).is_empty() {
                        self.protect = self.out.len();
                    }
                }
                Node::Raw(s) => {
                    self.out.push_str(s);
                    if !s.trim_matches([' ', '\t']).is_empty() {
                        self.protect = self.out.len();
                    }
                    let (_, last, multi) = raw_widths(s, self.o.tab_width);
                    if multi {
                        self.pos = last;
                    } else {
                        self.pos += last;
                    }
                }
                Node::Space => {
                    self.out.push(' ');
                    self.pos += 1;
                }
                Node::Line => match mode {
                    Mode::Flat => {
                        self.out.push(' ');
                        self.pos += 1;
                    }
                    Mode::Break => self.newline(0, Some(indent)),
                },
                Node::SoftLine => {
                    if mode == Mode::Break {
                        self.newline(0, Some(indent));
                    }
                }
                Node::Hard { blank } => self.newline(*blank, Some(indent)),
                Node::Literal { blank } => self.newline(*blank, None),
                Node::Concat(c) => {
                    for &k in c.iter().rev() {
                        cmds.push(Cmd {
                            indent,
                            mode,
                            id: k,
                            from: 0,
                        });
                    }
                }
                Node::Indent(c) => {
                    for &k in c.iter().rev() {
                        cmds.push(Cmd {
                            indent: indent + 1,
                            mode,
                            id: k,
                            from: 0,
                        });
                    }
                }
                Node::Group { children, broken } => {
                    let flat = |k| Cmd {
                        indent,
                        mode: Mode::Flat,
                        id: k,
                        from: 0,
                    };
                    let m = if mode == Mode::Flat && !*broken {
                        Mode::Flat
                    } else if *broken {
                        Mode::Break
                    } else {
                        let rem = self.o.width as isize - self.pos as isize;
                        let probe = Cmd {
                            indent,
                            mode: Mode::Flat,
                            id,
                            from: 0,
                        };
                        if self.fits(&[probe], &cmds, rem, false) {
                            Mode::Flat
                        } else {
                            Mode::Break
                        }
                    };
                    for &k in children.iter().rev() {
                        let mut c = flat(k);
                        c.mode = m;
                        cmds.push(c);
                    }
                }
                Node::Fill(parts) => self.fill(cmd, parts, &mut cmds),
            }
        }
    }

    fn fill(&mut self, cmd: Cmd, parts: &[Id], cmds: &mut Vec<Cmd>) {
        let from = cmd.from as usize;
        let parts_left = &parts[from..];
        if parts_left.is_empty() {
            return;
        }
        let rem = self.o.width as isize - self.pos as isize;
        let at = |id: Id, mode: Mode| Cmd {
            indent: cmd.indent,
            mode,
            id,
            from: 0,
        };
        let content = parts_left[0];
        // The last item is measured with what follows the fill (`)`, `;`).
        let last = parts_left.len() <= 2;
        let rest: &[Cmd] = if last { cmds } else { &[] };
        let content_fits = self.fits(&[at(content, Mode::Flat)], rest, rem, true);
        if parts_left.len() == 1 {
            cmds.push(at(
                content,
                if content_fits {
                    Mode::Flat
                } else {
                    Mode::Break
                },
            ));
            return;
        }
        let ws = parts_left[1];
        if parts_left.len() == 2 {
            let m = if content_fits {
                Mode::Flat
            } else {
                Mode::Break
            };
            cmds.push(at(ws, m));
            cmds.push(at(content, m));
            return;
        }
        let second = parts_left[2];
        let remaining = Cmd {
            from: (from + 2) as u32,
            ..cmd
        };
        // Do `content ws second` fit flat? (Measured with the rest when `second` is last.)
        let pair_rest: &[Cmd] = if parts_left.len() == 3 { cmds } else { &[] };
        let pair_fits = self.fits(
            &[
                at(content, Mode::Flat),
                at(ws, Mode::Flat),
                at(second, Mode::Flat),
            ],
            pair_rest,
            rem,
            true,
        );
        cmds.push(remaining);
        if pair_fits {
            cmds.push(at(ws, Mode::Flat));
            cmds.push(at(content, Mode::Flat));
        } else if content_fits {
            cmds.push(at(ws, Mode::Break));
            cmds.push(at(content, Mode::Flat));
        } else {
            cmds.push(at(ws, Mode::Break));
            cmds.push(at(content, Mode::Break));
        }
    }
}

/// Print a finished document.
pub fn print(doc: &Doc<'_>, o: &PrintOptions) -> String {
    let mut p = Printer {
        nodes: &doc.nodes,
        o,
        out: String::new(),
        pos: 0,
        protect: 0,
    };
    p.print(0);
    p.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(width: usize) -> PrintOptions {
        PrintOptions {
            width,
            indent: "\t".to_owned(),
            indent_width: 4,
            tab_width: 4,
            newline: "\n",
        }
    }

    /// `f( a, b, c );` as a group with an indented body.
    fn call<'a>(d: &mut Doc<'a>, args: &[&'a str]) {
        d.open(Container::Group);
        d.push(Node::Text("f("));
        d.open(Container::Indent);
        d.push(Node::Line);
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                d.push(Node::Text(","));
                d.push(Node::Line);
            }
            d.push(Node::Text(a));
        }
        d.close();
        d.push(Node::Line);
        d.push(Node::Text(")"));
        d.close();
        d.push(Node::Text(";"));
    }

    #[test]
    fn group_flat_or_broken() {
        let mut d = Doc::new();
        call(&mut d, &["aaa", "bbb"]);
        let d = d.finish();
        assert_eq!(print(&d, &opts(80)), "f( aaa, bbb );");
        // `;` counts: 14 columns do not fit in 13.
        assert_eq!(print(&d, &opts(13)), "f(\n\taaa,\n\tbbb\n);");
    }

    #[test]
    fn hard_breaks_enclosing_groups() {
        let mut d = Doc::new();
        d.open(Container::Group);
        d.push(Node::Text("a"));
        d.push(Node::Line);
        d.push(Node::Text("// c"));
        d.push(Node::Hard { blank: 0 });
        d.push(Node::Text("b"));
        d.close();
        let d = d.finish();
        assert_eq!(print(&d, &opts(80)), "a\n// c\nb");
    }

    #[test]
    fn fill_packs_items() {
        let mut d = Doc::new();
        d.push(Node::Text("x("));
        d.open(Container::Indent);
        d.open(Container::Fill);
        for (i, w) in ["aaaa", "bbbb", "cccc", "dddd"].iter().enumerate() {
            if i > 0 {
                d.push(Node::Line);
            }
            d.open(Container::Concat);
            d.push(Node::Text(w));
            if i < 3 {
                d.push(Node::Text(","));
            }
            d.close();
        }
        d.close();
        d.close();
        d.push(Node::Text(");"));
        let d = d.finish();
        assert_eq!(print(&d, &opts(80)), "x(aaaa, bbbb, cccc, dddd);");
        // The last item is measured with the `);` after the fill.
        assert_eq!(print(&d, &opts(16)), "x(aaaa, bbbb,\n\tcccc, dddd);");
        assert_eq!(print(&d, &opts(15)), "x(aaaa, bbbb,\n\tcccc,\n\tdddd);");
    }
}
