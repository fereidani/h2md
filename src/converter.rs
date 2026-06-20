//! # HTML to Markdown Conversion
//!
//! This module implements HTML-to-Markdown conversion using a custom DOM
//! implementation that integrates with html5ever's parser.
//!
//! ## Architecture
//!
//! 1. **Parsing**: The HTML input is parsed using html5ever, which builds a DOM
//!    tree using our custom `RcDom` implementation of the `TreeSink` trait.
//!
//! 2. **DOM Representation**: Nodes are stored as `Rc<Node>` with
//!    parent/child/sibling links, allowing efficient tree traversal.
//!
//! 3. **Conversion**: The `Converter` struct walks the DOM tree and outputs
//!    Markdown directly to a `Write` target, avoiding unnecessary allocations.
//!
//! ## Safety
//!
//! - Recursion depth is limited to `MAX_DEPTH` (200) to prevent stack overflow
//! - All error paths use proper `Result` propagation
//! - Debug assertions verify invariants in debug builds

use std::{cell::RefCell, fmt, io::{self, Write}};

use html5ever::Attribute;

use crate::dom::{Handle, NodeData, RcDom, iter_children};

/// The error type for HTML-to-Markdown conversion.
#[derive(Debug)]
pub enum Error {
    /// An error occurred while parsing the HTML input.
    Parse(String),
    /// An I/O error occurred while writing output.
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(msg) => write!(f, "HTML parse error: {msg}"),
            Error::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Parse(_) => None,
            Error::Io(err) => Some(err),
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

/// Convert HTML to Markdown, writing directly to any [`Write`] target.
///
/// # Errors
///
/// Returns [`Error::Parse`] if the HTML input cannot be parsed.
/// Returns [`Error::Io`] if writing to the output fails.
pub fn convert<W: Write>(html: &[u8], out: &mut W) -> Result<(), Error> {
    debug_assert!(!html.is_empty(), "input html must not be empty");
    let dom = RcDom::parse(html)?;
    let mut cvt = Converter {
        out,
        redirect_stack: Vec::new(),
        in_pre: false,
        at_line_start: true,
        pending_space: false,
        trailing_nls: 0,
        list_stack: Vec::new(),
        table_stack: Vec::new(),
        code_buf: String::new(),
        depth: 0,
    };
    cvt.walk(&dom.document)?;
    cvt.finalize()?;
    Ok(())
}

/// Maximum recursion depth to prevent stack overflow on malicious HTML.
const MAX_DEPTH: u32 = 200;

/// Pre-computed indent strings for list nesting depths (0–15 levels = 0–30
/// spaces).
const LIST_INDENTS: [&str; 16] = [
    "",
    "  ",
    "    ",
    "      ",
    "        ",
    "          ",
    "            ",
    "              ",
    "                ",
    "                  ",
    "                    ",
    "                      ",
    "                        ",
    "                          ",
    "                            ",
    "                              ",
];

/// Returns the indent string for a given list nesting level.
/// Falls back to maximum available indent for very deep nesting.
fn list_indent_str(level: usize) -> &'static str {
    LIST_INDENTS.get(level).copied().unwrap_or_else(|| {
        // For very deep nesting beyond our pre-computed values,
        // return the maximum available indent.
        LIST_INDENTS[LIST_INDENTS.len() - 1]
    })
}

// Converter

struct RedirectState {
    buf: Vec<u8>,
    saved_at_line_start: bool,
    saved_pending_space: bool,
    saved_trailing_nls: u8,
}

struct Converter<'a, W: Write> {
    out: &'a mut W,
    redirect_stack: Vec<RedirectState>,
    in_pre: bool,
    at_line_start: bool,
    pending_space: bool,
    trailing_nls: u8,
    list_stack: Vec<ListInfo>,
    table_stack: Vec<TableState>,
    code_buf: String,
    depth: u32,
}

struct ListInfo {
    ordered: bool,
    counter: u32,
}

struct TableState {
    rows: Vec<TableRow>,
    current_row: Vec<String>,
    current_cell: Vec<u8>,
    in_header: bool,
    current_row_has_th: bool,
}

struct TableRow {
    cells: Vec<String>,
    is_header: bool,
}

impl<'a, W: Write> Converter<'a, W> {
    fn finalize(&mut self) -> io::Result<()> {
        if self.trailing_nls == 0 {
            self.out.write_all(b"\n")?;
        }
        Ok(())
    }

    fn enter_redirect(&mut self) {
        debug_assert!(
            self.redirect_stack.len() < 100,
            "redirect stack too deep, possible leak"
        );
        self.redirect_stack.push(RedirectState {
            buf: Vec::new(),
            saved_at_line_start: self.at_line_start,
            saved_pending_space: self.pending_space,
            saved_trailing_nls: self.trailing_nls,
        });
        self.at_line_start = true;
        self.pending_space = false;
        self.trailing_nls = 0;
    }

    fn leave_redirect(&mut self) -> Option<RedirectState> {
        debug_assert!(
            !self.redirect_stack.is_empty(),
            "leave_redirect called with empty stack"
        );
        let state = self.redirect_stack.pop()?;
        self.at_line_start = state.saved_at_line_start;
        self.pending_space = state.saved_pending_space;
        self.trailing_nls = state.saved_trailing_nls;
        Some(state)
    }

    fn walk(&mut self, handle: &Handle) -> io::Result<()> {
        self.depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| io::Error::other("HTML nesting depth overflow"))?;
        if self.depth > MAX_DEPTH {
            return Err(io::Error::other(format!(
                "HTML nesting exceeds maximum depth of {MAX_DEPTH}"
            )));
        }
        let result = match &handle.data {
            NodeData::Document => self.walk_children(handle),
            NodeData::Text { contents } => {
                let text = contents.borrow();
                self.emit_text(&text)
            }
            NodeData::Element { name, attrs, .. } => {
                let tag: &str = &name.local;
                self.handle_element(tag, attrs, handle)
            }
            NodeData::Doctype { .. }
            | NodeData::Comment { .. }
            | NodeData::ProcessingInstruction { .. } => Ok(()),
        };
        self.depth -= 1;
        result
    }

    fn walk_children(&mut self, handle: &Handle) -> io::Result<()> {
        for child in iter_children(handle) {
            self.walk(&child)?;
        }
        Ok(())
    }

    fn handle_element(
        &mut self,
        tag: &str,
        attrs: &RefCell<Vec<Attribute>>,
        handle: &Handle,
    ) -> io::Result<()> {
        match tag {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = (tag.as_bytes()[1] - b'0') as usize;
                debug_assert!((1..=6).contains(&level), "heading level 1-6");
                self.handle_heading(level, handle)
            }
            "p" => self.handle_paragraph(handle),
            "blockquote" => self.handle_blockquote(handle),
            "pre" => self.handle_pre(handle),
            "hr" => self.handle_hr(),
            "br" => self.handle_br(),
            "div" | "section" | "article" | "main" | "header" | "footer" | "nav" | "aside"
            | "figure" | "figcaption" | "details" | "summary" | "address" => {
                self.ensure_blank_line()?;
                self.walk_children(handle)
            }
            "ul" | "ol" => self.handle_list(tag == "ol", attrs, handle),
            "li" => self.handle_list_item(handle),
            "table" => self.handle_table(handle),
            "thead" => self.handle_thead(handle),
            "tbody" | "tfoot" => self.walk_children(handle),
            "tr" => self.handle_tr(handle),
            "th" => self.handle_cell(true, handle),
            "td" => self.handle_cell(false, handle),
            "strong" | "b" => self.handle_inline("**", handle),
            "em" | "i" => self.handle_inline("*", handle),
            "del" | "s" | "strike" => self.handle_inline("~~", handle),
            "code" => self.handle_code(handle),
            "a" => self.handle_link(attrs, handle),
            "img" => self.handle_image(attrs),
            "script" | "style" | "noscript" | "head" | "meta" | "link" => Ok(()),
            _ => self.walk_children(handle),
        }
    }

    // Block elements

    fn handle_heading(&mut self, level: usize, handle: &Handle) -> io::Result<()> {
        self.ensure_blank_line()?;
        const HASHES: [&str; 7] = ["", "#", "##", "###", "####", "#####", "######"];
        debug_assert!((1..=6).contains(&level));
        self.emit(HASHES[level])?;
        self.emit(" ")?;
        self.walk_children(handle)?;
        self.emit("\n\n")?;
        self.at_line_start = true;
        Ok(())
    }

    fn handle_paragraph(&mut self, handle: &Handle) -> io::Result<()> {
        if self.table_stack.last().is_some() {
            return self.walk_children(handle);
        }
        self.ensure_blank_line()?;
        self.walk_children(handle)?;
        self.emit("\n\n")?;
        self.at_line_start = true;
        Ok(())
    }

    fn handle_blockquote(&mut self, handle: &Handle) -> io::Result<()> {
        self.ensure_blank_line()?;
        self.enter_redirect();
        // Suppress leading newlines in redirected content
        self.trailing_nls = 2;
        self.walk_children(handle)?;
        let Some(state) = self.leave_redirect() else {
            return Ok(());
        };
        let mut content = state.buf;
        while content.last() == Some(&b'\n') || content.last() == Some(&b' ') {
            content.pop();
        }
        if content.is_empty() {
            return Ok(());
        }
        for line in content.split(|&b| b == b'\n') {
            self.emit("> ")?;
            if !line.is_empty() {
                self.raw_write(line)?;
            }
            self.emit("\n")?;
        }
        self.emit("\n")?;
        self.at_line_start = true;
        Ok(())
    }

    fn handle_pre(&mut self, handle: &Handle) -> io::Result<()> {
        self.ensure_blank_line()?;
        let lang = self.extract_code_language(handle);
        self.emit("```")?;
        if let Some(ref l) = lang {
            self.emit(l)?;
        }
        self.emit("\n")?;
        self.enter_redirect();
        self.in_pre = true;
        self.walk_children(handle)?;
        self.in_pre = false;
        let Some(state) = self.leave_redirect() else {
            return Ok(());
        };
        let mut content = state.buf;
        while content.last() == Some(&b'\n') {
            content.pop();
        }
        self.raw_write(&content)?;
        self.trailing_nls = 0;
        self.emit("\n```\n")?;
        self.at_line_start = true;
        Ok(())
    }

    fn handle_hr(&mut self) -> io::Result<()> {
        self.ensure_blank_line()?;
        self.emit("---\n")?;
        self.at_line_start = true;
        Ok(())
    }

    fn handle_br(&mut self) -> io::Result<()> {
        if self.in_pre {
            self.emit("\n")?;
        } else {
            self.emit("  \n")?;
        }
        self.at_line_start = true;
        Ok(())
    }

    // Lists

    fn handle_list(
        &mut self,
        ordered: bool,
        attrs: &RefCell<Vec<Attribute>>,
        handle: &Handle,
    ) -> io::Result<()> {
        if self.list_stack.is_empty() {
            self.ensure_blank_line()?;
        } else if !self.at_line_start {
            self.emit("\n")?;
        }
        let start = if ordered {
            attrs
                .borrow()
                .iter()
                .find(|a| &*a.name.local == "start")
                .and_then(|a| a.value.parse::<u32>().ok())
                .unwrap_or(1)
        } else {
            1
        };
        self.list_stack.push(ListInfo {
            ordered,
            counter: start,
        });
        self.walk_children(handle)?;
        self.list_stack.pop();
        if self.list_stack.is_empty() {
            self.emit("\n")?;
            self.at_line_start = true;
        }
        Ok(())
    }

    fn handle_list_item(&mut self, handle: &Handle) -> io::Result<()> {
        debug_assert!(
            !self.list_stack.is_empty(),
            "list item must be child of ul/ol"
        );
        let indent = self.list_indent();
        if indent > 0 {
            let idx = indent / 2;
            self.emit(list_indent_str(idx))?;
        }
        let ordered = self.list_stack.last().is_some_and(|info| info.ordered);
        if ordered {
            let Some(info) = self.list_stack.last_mut() else {
                return Ok(());
            };
            let counter = info.counter;
            info.counter += 1;
            self.emit_u32(counter)?;
            self.emit(". ")?;
        } else {
            self.emit("- ")?;
        }
        self.at_line_start = false;
        self.walk_children(handle)?;
        self.emit("\n")?;
        self.at_line_start = true;
        Ok(())
    }

    #[inline]
    fn list_indent(&self) -> usize {
        self.list_stack.len().saturating_sub(1) * 2
    }

    // Tables

    fn handle_table(&mut self, handle: &Handle) -> io::Result<()> {
        debug_assert!(self.table_stack.is_empty(), "nested tables not supported");
        self.ensure_blank_line()?;
        self.table_stack.push(TableState {
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: Vec::new(),
            in_header: false,
            current_row_has_th: false,
        });
        self.walk_children(handle)?;
        self.finish_table()
    }

    fn handle_thead(&mut self, handle: &Handle) -> io::Result<()> {
        if let Some(table) = self.table_stack.last_mut() {
            table.in_header = true;
        }
        self.walk_children(handle)?;
        if let Some(table) = self.table_stack.last_mut() {
            table.in_header = false;
        }
        Ok(())
    }

    fn handle_tr(&mut self, handle: &Handle) -> io::Result<()> {
        self.walk_children(handle)?;
        if let Some(table) = self.table_stack.last_mut()
            && !table.current_row.is_empty()
        {
            let is_header = table.in_header || table.current_row_has_th;
            table.current_row_has_th = false;
            table.rows.push(TableRow {
                cells: table.current_row.drain(..).collect(),
                is_header,
            });
        }
        Ok(())
    }

    fn handle_cell(&mut self, is_th: bool, handle: &Handle) -> io::Result<()> {
        debug_assert!(!self.table_stack.is_empty(), "cell must be child of table");
        self.walk_children(handle)?;
        if let Some(table) = self.table_stack.last_mut() {
            if is_th {
                table.current_row_has_th = true;
            }
            let raw = std::mem::take(&mut table.current_cell);
            let mut cell_text = String::from_utf8(raw).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8 in table cell")
            })?;
            let trimmed = cell_text.trim();
            if trimmed.len() < cell_text.len() {
                cell_text = trimmed.to_owned();
            }
            table.current_row.push(cell_text);
        }
        Ok(())
    }

    fn finish_table(&mut self) -> io::Result<()> {
        self.pending_space = false;
        let table = match self.table_stack.pop() {
            Some(t) => t,
            None => {
                debug_assert!(false, "finish_table called with empty stack");
                return Ok(());
            }
        };
        if table.rows.is_empty() {
            return Ok(());
        }
        let ncols = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
        debug_assert!(ncols > 0, "table must have columns");
        let mut widths = Vec::new();
        compute_col_widths(&table.rows, ncols, &mut widths);
        let has_explicit = table.rows.iter().any(|r| r.is_header);
        for (idx, row) in table.rows.iter().enumerate() {
            self.emit_row(row, &widths, ncols)?;
            if row.is_header || (!has_explicit && idx == 0) {
                self.emit_sep(&widths, ncols)?;
            }
        }
        self.emit("\n")?;
        self.at_line_start = true;
        Ok(())
    }

    fn emit_row(&mut self, row: &TableRow, widths: &[usize], ncols: usize) -> io::Result<()> {
        debug_assert!(
            widths.len() >= ncols,
            "widths array must have at least ncols elements"
        );
        self.emit("|")?;
        for (i, width) in widths.iter().enumerate().take(ncols) {
            self.emit(" ")?;
            let cell = row.cells.get(i).map(String::as_str).unwrap_or("");
            if !cell.is_empty() {
                self.emit(cell)?;
            }
            for _ in 0..width.saturating_sub(cell.len()) {
                self.emit(" ")?;
            }
            self.emit(" |")?;
        }
        self.emit("\n")
    }

    fn emit_sep(&mut self, widths: &[usize], ncols: usize) -> io::Result<()> {
        debug_assert!(!widths.is_empty(), "widths array must not be empty");
        self.emit("|")?;
        for width in widths.iter().take(ncols) {
            self.emit(" ")?;
            for _ in 0..*width {
                self.emit("-")?;
            }
            self.emit(" |")?;
        }
        self.emit("\n")
    }

    // Inline elements

    fn handle_inline(&mut self, marker: &str, handle: &Handle) -> io::Result<()> {
        let alt = match marker {
            "**" => "__",
            "*" => "_",
            _ => marker,
        };
        let mut text = String::new();
        collect_text_recursive(handle, &mut text, self.depth).map_err(io::Error::other)?;
        let chosen = if text.contains(marker) && marker != alt && !text.contains(alt) {
            alt
        } else {
            marker
        };
        self.emit(chosen)?;
        self.walk_children(handle)?;
        self.emit(chosen)
    }

    fn handle_code(&mut self, handle: &Handle) -> io::Result<()> {
        if self.in_pre {
            return self.walk_children(handle);
        }
        self.code_buf.clear();
        collect_text_recursive(handle, &mut self.code_buf, self.depth).map_err(io::Error::other)?;
        let buf = std::mem::take(&mut self.code_buf);

        // Determine the minimum number of backticks needed for delimiters.
        // Must exceed the longest run of consecutive backticks in the content
        // so that the content cannot be mistaken for a closing delimiter.
        let max_run = longest_backtick_run(&buf);
        let delim_len = if max_run > 0 {
            max_run + 1
        } else if buf.starts_with(' ') || buf.ends_with(' ') {
            // Even without backticks, use double delimiters when the content
            // has leading or trailing spaces so we can add padding and
            // prevent CommonMark from stripping them.
            2
        } else {
            1
        };

        // Padding spaces are needed when using multi-backtick delimiters and
        // the content has leading/trailing whitespace.  CommonMark strips
        // one leading and one trailing space from code spans, so we add one
        // extra space on each side to preserve the original whitespace.
        let need_padding =
            delim_len > 1 && (buf.starts_with(' ') || buf.ends_with(' ') || buf.is_empty());

        let delim = "`".repeat(delim_len);
        self.emit(&delim)?;
        if need_padding {
            self.emit(" ")?;
        }
        self.emit(&buf)?;
        if need_padding {
            self.emit(" ")?;
        }
        self.emit(&delim)?;

        self.code_buf = buf;
        Ok(())
    }

    fn handle_link(&mut self, attrs: &RefCell<Vec<Attribute>>, handle: &Handle) -> io::Result<()> {
        let href = attrs
            .borrow()
            .iter()
            .find(|a| &*a.name.local == "href")
            .map(|a| a.value.clone());
        self.emit("[")?;
        self.walk_children(handle)?;
        self.emit("](")?;
        if let Some(ref h) = href {
            if h.contains('(') || h.contains(')') || h.contains(' ') {
                self.emit("<")?;
                self.emit(h)?;
                self.emit(">")?;
            } else {
                self.emit(h)?;
            }
        }
        self.emit(")")
    }

    fn handle_image(&mut self, attrs: &RefCell<Vec<Attribute>>) -> io::Result<()> {
        let borrowed = attrs.borrow();
        let src = borrowed.iter().find(|a| &*a.name.local == "src");
        let alt = borrowed.iter().find(|a| &*a.name.local == "alt");
        self.emit("![")?;
        if let Some(a) = alt {
            self.emit(&a.value)?;
        }
        self.emit("](")?;
        if let Some(s) = src {
            if s.value.contains('(') || s.value.contains(')') || s.value.contains(' ') {
                self.emit("<")?;
                self.emit(&s.value)?;
                self.emit(">")?;
            } else {
                self.emit(&s.value)?;
            }
        }
        self.emit(")")
    }

    // Output helpers

    /// Write bytes directly to the current target, bypassing state tracking.
    /// Used for buffered content that was already tracked during redirect.
    fn raw_write(&mut self, data: &[u8]) -> io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        if let Some(state) = self.redirect_stack.last_mut() {
            state.buf.extend_from_slice(data);
        } else if let Some(table) = self.table_stack.last_mut() {
            table.current_cell.extend_from_slice(data);
        } else {
            self.out.write_all(data)?;
        }
        self.at_line_start = data.last() == Some(&b'\n');
        self.trailing_nls = 0;
        Ok(())
    }

    fn emit(&mut self, s: &str) -> io::Result<()> {
        debug_assert!(!s.is_empty(), "emit called with empty string");
        if self.pending_space && !s.starts_with('\n') && !s.starts_with(' ') {
            self.pending_space = false;
            self.write_one(b' ')?;
        } else if self.pending_space && s.starts_with('\n') {
            self.pending_space = false;
        }
        let bytes = s.as_bytes();
        let all_newlines = bytes.iter().all(|&b| b == b'\n');
        if all_newlines {
            let needed = 2usize.saturating_sub(self.trailing_nls as usize);
            let to_write = bytes.len().min(needed);
            if to_write > 0 {
                self.write_all(&bytes[..to_write])?;
            }
            self.trailing_nls = (self.trailing_nls + to_write as u8).min(2);
            self.at_line_start = true;
            return Ok(());
        }
        self.write_all(bytes)?;
        let len = bytes.len();
        let mut nls: u8 = 0;
        for &b in bytes.iter().rev().take(2) {
            if b == b'\n' {
                nls += 1;
            } else {
                break;
            }
        }
        if nls > 0 {
            if len > nls as usize {
                self.trailing_nls = nls;
            } else {
                self.trailing_nls = (self.trailing_nls + nls).min(2);
            }
        } else {
            self.trailing_nls = 0;
        }
        self.at_line_start = s.ends_with('\n');
        Ok(())
    }

    fn write_one(&mut self, b: u8) -> io::Result<()> {
        if let Some(state) = self.redirect_stack.last_mut() {
            state.buf.push(b);
        } else if let Some(table) = self.table_stack.last_mut() {
            table.current_cell.push(b);
        } else {
            let buf = [b];
            self.out.write_all(&buf)?;
        }
        Ok(())
    }

    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        if let Some(state) = self.redirect_stack.last_mut() {
            state.buf.extend_from_slice(data);
        } else if let Some(table) = self.table_stack.last_mut() {
            table.current_cell.extend_from_slice(data);
        } else {
            self.out.write_all(data)?;
        }
        Ok(())
    }

    /// Write a `u32` as decimal ASCII directly to the output, no heap
    /// allocation.
    fn emit_u32(&mut self, mut n: u32) -> io::Result<()> {
        if n == 0 {
            self.write_one(b'0')?;
            return Ok(());
        }
        let mut buf = [0u8; 10];
        let mut pos = 10;
        while n > 0 {
            pos -= 1;
            buf[pos] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        self.write_all(&buf[pos..])
    }

    fn emit_text(&mut self, text: &str) -> io::Result<()> {
        if self.in_pre {
            if !text.is_empty() {
                self.emit(text)?;
            }
            return Ok(());
        }
        self.emit_text_normalized(text)
    }

    fn emit_text_normalized(&mut self, text: &str) -> io::Result<()> {
        let text = if self.at_line_start {
            let trimmed = text.trim_start();
            if trimmed.is_empty() {
                return Ok(());
            }
            trimmed
        } else {
            text
        };
        let mut last_ws = false;
        let mut seg_start = 0;
        for (i, ch) in text.char_indices() {
            if ch.is_whitespace() {
                if !last_ws && seg_start < i {
                    self.emit(&text[seg_start..i])?;
                }
                last_ws = true;
                seg_start = i + ch.len_utf8();
            } else {
                if last_ws && seg_start <= i {
                    self.emit(" ")?;
                }
                last_ws = false;
            }
        }
        if seg_start < text.len() && !last_ws {
            self.emit(&text[seg_start..])?;
        }
        if last_ws {
            self.pending_space = true;
        }
        Ok(())
    }

    fn ensure_blank_line(&mut self) -> io::Result<()> {
        self.pending_space = false;
        if self.table_stack.last().is_some() {
            return Ok(());
        }
        match self.trailing_nls {
            0 => self.emit("\n\n")?,
            1 => self.emit("\n")?,
            _ => {}
        }
        self.at_line_start = true;
        Ok(())
    }

    fn extract_code_language(&self, handle: &Handle) -> Option<String> {
        for child in iter_children(handle) {
            if let NodeData::Element { name, attrs, .. } = &child.data
                && &*name.local == "code"
            {
                for attr in attrs.borrow().iter() {
                    if &*attr.name.local == "class"
                        && let Some(lang) = attr.value.strip_prefix("language-")
                    {
                        return Some(lang.to_owned());
                    }
                }
            }
        }
        None
    }
}

fn collect_text_recursive(handle: &Handle, out: &mut String, depth: u32) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("text collection exceeds maximum depth".to_owned());
    }
    match &handle.data {
        NodeData::Text { contents } => {
            out.push_str(&contents.borrow());
        }
        NodeData::Element { .. } => {
            for child in iter_children(handle) {
                collect_text_recursive(&child, out, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Returns the length of the longest run of consecutive backtick characters
/// in `s`. Returns 0 if `s` contains no backticks.
fn longest_backtick_run(s: &str) -> usize {
    let mut max_run = 0;
    let mut current = 0;
    for &b in s.as_bytes() {
        if b == b'`' {
            current += 1;
            if current > max_run {
                max_run = current;
            }
        } else {
            current = 0;
        }
    }
    max_run
}

/// Compute the maximum width of each column in a table.
/// Results are written to `widths`, which is cleared and resized to `ncols`.
fn compute_col_widths(rows: &[TableRow], ncols: usize, widths: &mut Vec<usize>) {
    widths.clear();
    widths.resize(ncols, 3);
    for row in rows {
        for (i, cell) in row.cells.iter().enumerate() {
            if i < ncols {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }
}
