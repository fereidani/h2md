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

use std::{
    cell::RefCell,
    fmt,
    io::{self, Write},
};

use html5ever::Attribute;
use unicode_width::UnicodeWidthStr;

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

/// Options that control Markdown output. Construct with [`Options::default`]
/// for standard output, or toggle fields for a more compact result.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Emit compact Markdown with minimal padding:
    ///
    /// - tables are unpadded with a minimal separator row (`|a|b|`),
    /// - inline content (link text, emphasis, headings, image alt text) is
    ///   collapsed onto a single line, so a block element inside a link can
    ///   never split `[...]` across lines,
    /// - lists stay tight, with no blank line between items,
    /// - blocks are separated by at most one blank line, and the document
    ///   neither starts nor ends with blank padding.
    ///
    /// When false (the default), tables are aligned with column padding and
    /// blocks keep the surrounding blank lines.
    pub compressed: bool,
}

/// Convert HTML to Markdown with the default [`Options`], writing directly to
/// any [`Write`] target.
///
/// # Errors
///
/// Returns [`Error::Parse`] if the HTML input cannot be parsed.
/// Returns [`Error::Io`] if writing to the output fails.
pub fn convert<W: Write>(html: &[u8], out: &mut W) -> Result<(), Error> {
    convert_with(html, out, &Options::default())
}

/// Convert HTML to Markdown using `opts`, writing directly to any [`Write`]
/// target.
///
/// # Errors
///
/// Returns [`Error::Parse`] if the HTML input cannot be parsed.
/// Returns [`Error::Io`] if writing to the output fails.
pub fn convert_with<W: Write>(html: &[u8], out: &mut W, opts: &Options) -> Result<(), Error> {
    debug_assert!(!html.is_empty(), "input html must not be empty");
    let dom = RcDom::parse(html)?;
    let mut cvt = Converter {
        out,
        compressed: opts.compressed,
        redirect_stack: Vec::new(),
        in_pre: false,
        at_line_start: true,
        at_item_start: false,
        pending_space: false,
        trailing_nls: 0,
        pending_nls: 0,
        wrote_any: false,
        list_stack: Vec::new(),
        table_stack: Vec::new(),
        code_buf: String::new(),
        text_buf: String::new(),
        row_buf: String::new(),
        depth: 0,
    };
    cvt.walk(&dom.document)?;
    cvt.finalize()?;
    Ok(())
}

/// Maximum recursion depth to prevent stack overflow on malicious HTML.
const MAX_DEPTH: u32 = 200;

/// Maximum run of consecutive newlines in the output: one blank line between
/// blocks.
const MAX_BLANK_NLS: u8 = 2;

/// Markdown heading prefixes indexed by heading level (1-6); index 0 unused.
const HASHES: [&str; 7] = ["", "#", "##", "###", "####", "#####", "######"];

/// Pre-computed indent strings for list nesting depths (0-15 levels = 0-30
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
    saved_at_item_start: bool,
    saved_pending_space: bool,
    saved_trailing_nls: u8,
}

// The independent boolean flags below track distinct facets of the output
// position; a state enum would not model them cleanly, so the bool count is
// accepted here.
#[allow(clippy::struct_excessive_bools)]
struct Converter<'a, W: Write> {
    out: &'a mut W,
    compressed: bool,
    redirect_stack: Vec<RedirectState>,
    in_pre: bool,
    at_line_start: bool,
    /// True while a list item has emitted its marker but no body content yet,
    /// so a leading block child does not insert a blank line after the marker.
    at_item_start: bool,
    pending_space: bool,
    trailing_nls: u8,
    /// Newlines withheld from the output in compressed mode until further
    /// content arrives. Holding them back keeps blank padding out of the start
    /// and the end of the document. Unused in normal mode.
    pending_nls: u8,
    /// True once any content has reached the output target. Used in compressed
    /// mode to drop the withheld newlines that would otherwise lead the
    /// document.
    wrote_any: bool,
    list_stack: Vec<ListInfo>,
    table_stack: Vec<TableState>,
    code_buf: String,
    text_buf: String,
    row_buf: String,
    depth: u32,
}

struct ListInfo {
    ordered: bool,
    counter: u32,
}

struct TableState {
    rows: Vec<TableRow>,
    current_row: Vec<String>,
    in_header: bool,
    current_row_has_th: bool,
}

struct TableRow {
    cells: Vec<String>,
    is_header: bool,
}

impl<W: Write> Converter<'_, W> {
    fn finalize(&mut self) -> io::Result<()> {
        debug_assert!(
            self.redirect_stack.is_empty(),
            "finalize called with an open redirect"
        );
        if self.compressed {
            // Withheld newlines are dropped: the document ends with exactly
            // one newline and no trailing blank line.
            self.pending_nls = 0;
            if self.wrote_any {
                self.out.write_all(b"\n")?;
            }
            return Ok(());
        }
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
            saved_at_item_start: self.at_item_start,
            saved_pending_space: self.pending_space,
            saved_trailing_nls: self.trailing_nls,
        });
        self.at_line_start = true;
        self.at_item_start = false;
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
        self.at_item_start = state.saved_at_item_start;
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

    /// Render `handle`'s children as inline content.
    ///
    /// In compressed mode the result is captured and collapsed onto a single
    /// line, so a block element inside a link, a heading, or an emphasis span
    /// cannot split the construct across lines. In normal mode the children are
    /// written straight through.
    fn walk_inline_children(&mut self, handle: &Handle) -> io::Result<()> {
        if !self.compressed {
            return self.walk_children(handle);
        }
        self.enter_redirect();
        let walked = self.walk_children(handle);
        let Some(state) = self.leave_redirect() else {
            return walked;
        };
        walked?;
        let mut buf = state.buf;
        collapse_whitespace(&mut buf);
        self.raw_write(&buf)
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
            // `tbody`, `tfoot`, and unrecognized elements: render children
            // directly without adding block spacing.
            _ => self.walk_children(handle),
        }
    }

    // Block elements

    fn handle_heading(&mut self, level: usize, handle: &Handle) -> io::Result<()> {
        self.ensure_blank_line()?;
        debug_assert!((1..=6).contains(&level));
        self.emit(HASHES[level])?;
        self.emit(" ")?;
        // A heading holds inline content only; a stray block child must not
        // break the `#` line in two.
        self.walk_inline_children(handle)?;
        self.end_block()
    }

    fn handle_paragraph(&mut self, handle: &Handle) -> io::Result<()> {
        if self.table_stack.last().is_some() {
            return self.walk_children(handle);
        }
        self.ensure_blank_line()?;
        self.walk_children(handle)?;
        self.end_block()
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
        self.end_block()
    }

    fn handle_pre(&mut self, handle: &Handle) -> io::Result<()> {
        self.ensure_blank_line()?;
        let lang = extract_code_language(handle);
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
        } else if self.compressed {
            // A break that starts a line only pads the output; the line it
            // would end is already ended.
            if !self.at_line_start {
                self.emit("  \n")?;
            }
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
        // The item has emitted its marker but no body content yet; the first
        // block child must not prepend a blank line that would leave the
        // marker alone on its own line.
        self.at_item_start = true;
        self.walk_children(handle)?;
        // A block child in compressed mode has already ended the line; another
        // newline here would turn the list loose.
        if !self.compressed || self.trailing_nls == 0 {
            self.emit("\n")?;
        }
        self.at_line_start = true;
        Ok(())
    }

    #[inline]
    fn list_indent(&self) -> usize {
        self.list_stack.len().saturating_sub(1) * 2
    }

    // Tables

    fn handle_table(&mut self, handle: &Handle) -> io::Result<()> {
        if self.table_stack.is_empty() {
            // Only emit a leading blank line for a top-level table. Nested
            // tables are captured into their parent cell's redirect buffer.
            self.ensure_blank_line()?;
        }
        self.table_stack.push(TableState {
            rows: Vec::new(),
            current_row: Vec::new(),
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
        // Capture the cell's rendered content through the redirect mechanism so
        // that block children (lists, nested tables, etc.) are converted
        // normally, then flattened to a single Markdown line by normalize_cell.
        self.enter_redirect();
        self.walk_children(handle)?;
        let Some(state) = self.leave_redirect() else {
            return Ok(());
        };
        let cell_text = String::from_utf8(state.buf).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8 in table cell")
        })?;
        if let Some(table) = self.table_stack.last_mut() {
            if is_th {
                table.current_row_has_th = true;
            }
            table.current_row.push(normalize_cell(&cell_text));
        }
        Ok(())
    }

    fn finish_table(&mut self) -> io::Result<()> {
        self.pending_space = false;
        let Some(table) = self.table_stack.pop() else {
            debug_assert!(false, "finish_table called with empty stack");
            return Ok(());
        };
        if table.rows.is_empty() {
            return Ok(());
        }
        let ncols = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
        debug_assert!(ncols > 0, "table must have columns");
        let has_explicit = table.rows.iter().any(|r| r.is_header);
        if self.compressed {
            for (idx, row) in table.rows.iter().enumerate() {
                self.emit_compact_row(row, ncols)?;
                if row.is_header || (!has_explicit && idx == 0) {
                    self.emit_compact_sep(ncols)?;
                }
            }
        } else {
            let mut widths = Vec::new();
            compute_col_widths(&table.rows, ncols, &mut widths);
            for (idx, row) in table.rows.iter().enumerate() {
                self.emit_row(row, &widths, ncols)?;
                if row.is_header || (!has_explicit && idx == 0) {
                    self.emit_sep(&widths, ncols)?;
                }
            }
        }
        self.end_block()
    }

    /// Emit a single table row with no alignment padding: `|a|b|`. Empty cells
    /// become `||`. Used in compressed mode.
    fn emit_compact_row(&mut self, row: &TableRow, ncols: usize) -> io::Result<()> {
        self.row_buf.clear();
        for i in 0..ncols {
            self.row_buf.push('|');
            let cell = row.cells.get(i).map_or("", String::as_str);
            self.row_buf.push_str(cell);
        }
        self.row_buf.push('|');
        self.row_buf.push('\n');
        let line = std::mem::take(&mut self.row_buf);
        self.emit(&line)?;
        self.row_buf = line;
        Ok(())
    }

    /// Emit a minimal separator row with one dash per column: `|-|-|`.
    fn emit_compact_sep(&mut self, ncols: usize) -> io::Result<()> {
        self.row_buf.clear();
        for _ in 0..ncols {
            self.row_buf.push('|');
            self.row_buf.push('-');
        }
        self.row_buf.push('|');
        self.row_buf.push('\n');
        let line = std::mem::take(&mut self.row_buf);
        self.emit(&line)?;
        self.row_buf = line;
        Ok(())
    }

    fn emit_row(&mut self, row: &TableRow, widths: &[usize], ncols: usize) -> io::Result<()> {
        debug_assert!(
            widths.len() >= ncols,
            "widths array must have at least ncols elements"
        );
        self.row_buf.clear();
        self.row_buf.push('|');
        for (i, width) in widths.iter().enumerate().take(ncols) {
            self.row_buf.push(' ');
            let cell = row.cells.get(i).map_or("", String::as_str);
            self.row_buf.push_str(cell);
            let cell_width = UnicodeWidthStr::width(cell);
            for _ in 0..width.saturating_sub(cell_width) {
                self.row_buf.push(' ');
            }
            self.row_buf.push_str(" |");
        }
        self.row_buf.push('\n');
        let line = std::mem::take(&mut self.row_buf);
        self.emit(&line)?;
        self.row_buf = line;
        Ok(())
    }

    fn emit_sep(&mut self, widths: &[usize], ncols: usize) -> io::Result<()> {
        debug_assert!(!widths.is_empty(), "widths array must not be empty");
        self.row_buf.clear();
        self.row_buf.push('|');
        for width in widths.iter().take(ncols) {
            self.row_buf.push(' ');
            for _ in 0..*width {
                self.row_buf.push('-');
            }
            self.row_buf.push_str(" |");
        }
        self.row_buf.push('\n');
        let line = std::mem::take(&mut self.row_buf);
        self.emit(&line)?;
        self.row_buf = line;
        Ok(())
    }

    // Inline elements

    fn handle_inline(&mut self, marker: &str, handle: &Handle) -> io::Result<()> {
        let alt = match marker {
            "**" => "__",
            "*" => "_",
            _ => marker,
        };
        // Capture the rendered content once via the redirect mechanism (already
        // escaped/normalized), then pick the delimiter by scanning the buffer.
        // This walks the subtree a single time instead of once to select a
        // delimiter and again to emit.
        self.enter_redirect();
        self.walk_children(handle)?;
        let Some(state) = self.leave_redirect() else {
            return Ok(());
        };
        let mut buf = state.buf;
        if self.compressed {
            // Keep the delimiters and their content on one line, whatever the
            // element wraps.
            collapse_whitespace(&mut buf);
        }
        if buf.is_empty() {
            // An empty inline element contributes nothing.
            return Ok(());
        }
        // Choose the alternative delimiter only when a bare marker run appears
        // in the rendered content (structural markers from nested formatting)
        // and the alternative itself does not appear.
        let marker_bytes = marker.as_bytes();
        let alt_bytes = alt.as_bytes();
        let chosen = if marker != alt
            && slice_contains(&buf, marker_bytes)
            && !slice_contains(&buf, alt_bytes)
        {
            alt
        } else {
            marker
        };
        self.emit(chosen)?;
        self.raw_write(&buf)?;
        self.emit(chosen)
    }

    fn handle_code(&mut self, handle: &Handle) -> io::Result<()> {
        if self.in_pre {
            return self.walk_children(handle);
        }
        self.code_buf.clear();
        collect_text_recursive(handle, &mut self.code_buf, self.depth)?;
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
        self.walk_inline_children(handle)?;
        self.emit("](")?;
        if let Some(ref h) = href {
            self.emit_url(h)?;
        }
        self.emit(")")
    }

    fn handle_image(&mut self, attrs: &RefCell<Vec<Attribute>>) -> io::Result<()> {
        let borrowed = attrs.borrow();
        let src = borrowed.iter().find(|a| &*a.name.local == "src");
        let alt = borrowed.iter().find(|a| &*a.name.local == "alt");
        self.emit("![")?;
        if let Some(a) = alt {
            self.emit_alt(&a.value)?;
        }
        self.emit("](")?;
        if let Some(s) = src {
            self.emit_url(&s.value)?;
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
        self.write_all(data)?;
        self.at_line_start = data.last() == Some(&b'\n');
        // Buffered content can end with newlines of its own; counting them
        // keeps a following `ensure_blank_line` from stacking another blank
        // line on top.
        let nls = trailing_newlines(data);
        self.trailing_nls = if data.len() > nls as usize {
            nls
        } else {
            self.trailing_nls.saturating_add(nls).min(MAX_BLANK_NLS)
        };
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
            let needed = usize::from(MAX_BLANK_NLS).saturating_sub(self.trailing_nls as usize);
            let to_write = bytes.len().min(needed);
            if to_write > 0 {
                self.write_all(&bytes[..to_write])?;
            }
            // `to_write` is bounded by `needed` (<= MAX_BLANK_NLS), so it
            // always fits in a u8; the fallback is never used.
            let added = u8::try_from(to_write).unwrap_or(MAX_BLANK_NLS);
            self.trailing_nls = self.trailing_nls.saturating_add(added).min(MAX_BLANK_NLS);
            self.at_line_start = true;
            return Ok(());
        }
        self.write_all(bytes)?;
        // Real (non-whitespace) content ends the "just emitted a marker" state
        // where leading blank lines are suppressed.
        if self.at_item_start && bytes.iter().any(|&b| !b.is_ascii_whitespace()) {
            self.at_item_start = false;
        }
        let nls = trailing_newlines(bytes);
        if nls > 0 {
            if bytes.len() > nls as usize {
                self.trailing_nls = nls;
            } else {
                self.trailing_nls = self.trailing_nls.saturating_add(nls).min(MAX_BLANK_NLS);
            }
        } else {
            self.trailing_nls = 0;
        }
        self.at_line_start = s.ends_with('\n');
        Ok(())
    }

    fn write_one(&mut self, b: u8) -> io::Result<()> {
        let buf = [b];
        self.write_all(&buf)
    }

    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        if let Some(state) = self.redirect_stack.last_mut() {
            state.buf.extend_from_slice(data);
            return Ok(());
        }
        if self.compressed {
            return self.write_compact(data);
        }
        self.out.write_all(data)
    }

    /// Write `data` to the output target, holding back the trailing run of
    /// newlines instead of writing it immediately.
    ///
    /// Deferring the run keeps the document free of leading and trailing blank
    /// padding, and caps any run that survives at [`MAX_BLANK_NLS`], even when
    /// the run is assembled from several writes (buffered content flushed with
    /// [`Converter::raw_write`], for example). Newlines inside `data` are
    /// written unchanged, so fenced code blocks keep their blank lines.
    fn write_compact(&mut self, data: &[u8]) -> io::Result<()> {
        debug_assert!(self.compressed, "write_compact used in normal mode");
        debug_assert!(
            self.pending_nls <= MAX_BLANK_NLS,
            "pending newline run exceeds the cap"
        );
        let end = data.iter().rposition(|&b| b != b'\n').map_or(0, |i| i + 1);
        let (content, newlines) = data.split_at(end);
        if !content.is_empty() {
            if self.wrote_any {
                for _ in 0..self.pending_nls {
                    self.out.write_all(b"\n")?;
                }
            }
            self.pending_nls = 0;
            self.out.write_all(content)?;
            self.wrote_any = true;
        }
        // `newlines.len()` is bounded by the write size; saturating conversion
        // then clamping to the cap keeps the count in range for any input.
        let added = u8::try_from(newlines.len()).unwrap_or(MAX_BLANK_NLS);
        self.pending_nls = self.pending_nls.saturating_add(added).min(MAX_BLANK_NLS);
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

    /// Emit an image's alt text. In compressed mode whitespace runs collapse to
    /// a single space so the `![...]` label stays on one line; an attribute
    /// value may hold newlines of its own.
    fn emit_alt(&mut self, alt: &str) -> io::Result<()> {
        if alt.is_empty() {
            return Ok(());
        }
        if !self.compressed {
            return self.emit(alt);
        }
        self.text_buf.clear();
        let mut pending = false;
        for ch in alt.chars() {
            if ch.is_whitespace() {
                pending = !self.text_buf.is_empty();
                continue;
            }
            if pending {
                self.text_buf.push(' ');
                pending = false;
            }
            self.text_buf.push(ch);
        }
        let buf = std::mem::take(&mut self.text_buf);
        if !buf.is_empty() {
            self.emit(&buf)?;
        }
        self.text_buf = buf;
        Ok(())
    }

    /// Emit a link/image destination URL, wrapping it in angle brackets and
    /// backslash-escaping `<`, `>` and backticks inside when the URL contains
    /// characters that would otherwise break inline link syntax.
    fn emit_url(&mut self, url: &str) -> io::Result<()> {
        let needs_wrap = url
            .chars()
            .any(|c| matches!(c, '(' | ')' | '<' | '>' | '`') || c.is_whitespace());
        if !needs_wrap {
            return self.emit(url);
        }
        self.text_buf.clear();
        self.text_buf.push('<');
        for c in url.chars() {
            if matches!(c, '<' | '>' | '`') {
                self.text_buf.push('\\');
            }
            self.text_buf.push(c);
        }
        self.text_buf.push('>');
        let buf = std::mem::take(&mut self.text_buf);
        self.emit(&buf)?;
        self.text_buf = buf;
        Ok(())
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
        let text = if self.at_line_start || self.at_item_start {
            let trimmed = text.trim_start();
            if trimmed.is_empty() {
                return Ok(());
            }
            trimmed
        } else {
            text
        };
        // Coalesce the whole normalized node into a reusable buffer and emit
        // it once, escaping Markdown-significant characters as we go. The
        // first emitted character may sit at the start of an output line, in
        // which case the line-start trigger characters (# + - >) are escaped
        // too so they are not reinterpreted as block syntax.
        self.text_buf.clear();
        let mut first_on_line = self.at_line_start;
        let mut last_ws = false;
        for ch in text.chars() {
            if ch.is_whitespace() {
                last_ws = true;
            } else {
                if last_ws {
                    self.text_buf.push(' ');
                    first_on_line = false;
                }
                escape_markdown_char(&mut self.text_buf, ch, first_on_line);
                last_ws = false;
                first_on_line = false;
            }
        }
        let buf = std::mem::take(&mut self.text_buf);
        if !buf.is_empty() {
            self.emit(&buf)?;
        }
        self.text_buf = buf;
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
        // A block element that is the first body content of a list item must
        // not insert a blank line after the marker.
        if self.at_item_start {
            return Ok(());
        }
        if self.tight_block() {
            if self.trailing_nls == 0 {
                self.emit("\n")?;
            }
            self.at_line_start = true;
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

    /// Close a block element, leaving the output at the start of a line.
    fn end_block(&mut self) -> io::Result<()> {
        if self.tight_block() {
            if self.trailing_nls == 0 {
                self.emit("\n")?;
            }
        } else {
            self.emit("\n\n")?;
        }
        self.at_line_start = true;
        Ok(())
    }

    /// Returns `true` when the current block must be separated by a single
    /// newline instead of a blank line: compressed mode keeps lists tight, so
    /// blocks inside a list item stay on consecutive lines.
    #[inline]
    fn tight_block(&self) -> bool {
        self.compressed && !self.list_stack.is_empty()
    }
}

/// Look for a `<code class="language-*">` child of `handle` and return the
/// language tag for a fenced code block.
fn extract_code_language(handle: &Handle) -> Option<String> {
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

fn collect_text_recursive(handle: &Handle, out: &mut String, depth: u32) -> io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(io::Error::other(format!(
            "HTML nesting exceeds maximum depth of {MAX_DEPTH}"
        )));
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

/// Returns the number of newlines at the end of `data`, capped at
/// [`MAX_BLANK_NLS`].
fn trailing_newlines(data: &[u8]) -> u8 {
    let mut nls: u8 = 0;
    for &b in data.iter().rev().take(MAX_BLANK_NLS as usize) {
        if b != b'\n' {
            break;
        }
        nls += 1;
    }
    debug_assert!(nls <= MAX_BLANK_NLS, "count is bounded by the cap");
    nls
}

/// Collapse every run of ASCII whitespace in `buf` to a single space and drop
/// the leading and trailing runs, in place.
///
/// Operating on bytes is safe for UTF-8 input because the bytes of a multi-byte
/// sequence are all >= 0x80 and therefore never ASCII whitespace.
fn collapse_whitespace(buf: &mut Vec<u8>) {
    let mut write = 0;
    let mut pending = false;
    for read in 0..buf.len() {
        let b = buf[read];
        if b.is_ascii_whitespace() {
            // A leading run is dropped rather than turned into a space.
            pending = write > 0;
            continue;
        }
        if pending {
            buf[write] = b' ';
            write += 1;
            pending = false;
        }
        buf[write] = b;
        write += 1;
        debug_assert!(write <= read + 1, "output never overtakes the read index");
    }
    buf.truncate(write);
    debug_assert!(
        !buf.contains(&b'\n'),
        "collapsed content must be a single line"
    );
}

/// Returns `true` when `needle` occurs anywhere in `haystack`. A byte-level
/// substring search is used because the captured content may be inspected
/// before it is known to be valid UTF-8.
fn slice_contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Escape a single character for Markdown output, appending it (and any
/// required leading backslash) to `out`.
///
/// `at_line_start` indicates the character will be the first non-whitespace
/// character on its output line, so block-triggering characters (`#`, `+`,
/// `-`, `>`) are escaped as well. `<` is always escaped because a following
/// tag-like sequence would otherwise be consumed as inline HTML. Strikethrough
/// is supported, so `~` is always escaped.
fn escape_markdown_char(out: &mut String, ch: char, at_line_start: bool) {
    let needs_escape = matches!(ch, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '~')
        || (at_line_start && matches!(ch, '#' | '+' | '-' | '>'));
    if needs_escape {
        out.push('\\');
    }
    out.push(ch);
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

/// Compute the maximum display width of each column in a table.
/// Results are written to `widths`, which is cleared and resized to `ncols`.
fn compute_col_widths(rows: &[TableRow], ncols: usize, widths: &mut Vec<usize>) {
    widths.clear();
    widths.resize(ncols, 3);
    for row in rows {
        for (i, cell) in row.cells.iter().enumerate() {
            if i < ncols {
                let w = UnicodeWidthStr::width(cell.as_str());
                widths[i] = widths[i].max(w);
            }
        }
    }
}

/// Collapse a table cell's rendered content to a single Markdown line: drop
/// leading/trailing whitespace, reduce internal whitespace runs (including
/// newlines produced by block children) to single spaces, and backslash-escape
/// `|` so the cell cannot break out of its column.
fn normalize_cell(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut pending_space = false;
    for c in raw.chars() {
        if c.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        if c == '|' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
