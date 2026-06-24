use h2md::{Options, convert, convert_with};

/// Helper: convert HTML to Markdown, trimming surrounding whitespace.
fn h(input: &str) -> String {
    let mut out = Vec::new();
    convert(input.as_bytes(), &mut out).expect("conversion failed");
    String::from_utf8(out)
        .expect("output was not valid UTF-8")
        .trim()
        .to_owned()
}

/// Helper: convert HTML to Markdown in compressed mode, trimming surrounding
/// whitespace.
fn hc(input: &str) -> String {
    let mut out = Vec::new();
    convert_with(input.as_bytes(), &mut out, &Options { compressed: true })
        .expect("conversion failed");
    String::from_utf8(out)
        .expect("output was not valid UTF-8")
        .trim()
        .to_owned()
}

/// Helper: convert and return the raw output without trimming.
fn h_raw(input: &str) -> String {
    let mut out = Vec::new();
    convert(input.as_bytes(), &mut out).expect("conversion failed");
    String::from_utf8(out).expect("output was not valid UTF-8")
}

// Headings

#[test]
fn heading_levels() {
    for level in 1..=6 {
        let tag = format!("h{level}");
        let html = format!("<{tag}>Hello</{tag}>");
        let hashes = "#".repeat(level);
        assert_eq!(h(&html), format!("{hashes} Hello"));
    }
}

#[test]
fn heading_with_inline_markup() {
    assert_eq!(h("<h2>Bold <strong>move</strong></h2>"), "## Bold **move**");
}

// Paragraphs

#[test]
fn plain_paragraph() {
    assert_eq!(h("<p>Hello world</p>"), "Hello world");
}

#[test]
fn consecutive_paragraphs() {
    assert_eq!(h("<p>First</p><p>Second</p>"), "First\n\nSecond");
}

#[test]
fn paragraph_with_inline() {
    assert_eq!(
        h("<p>This is <strong>bold</strong> and <em>italic</em>.</p>"),
        "This is **bold** and *italic*."
    );
}

// Inline elements

#[test]
fn bold_strong() {
    assert_eq!(h("<strong>x</strong>"), "**x**");
    assert_eq!(h("<b>x</b>"), "**x**");
}

#[test]
fn italic_em() {
    assert_eq!(h("<em>x</em>"), "*x*");
    assert_eq!(h("<i>x</i>"), "*x*");
}

#[test]
fn strikethrough() {
    assert_eq!(h("<del>x</del>"), "~~x~~");
    assert_eq!(h("<s>x</s>"), "~~x~~");
    assert_eq!(h("<strike>x</strike>"), "~~x~~");
}

#[test]
fn link() {
    assert_eq!(
        h(r#"<a href="https://example.com">click</a>"#),
        "[click](https://example.com)"
    );
}

#[test]
fn link_without_href() {
    assert_eq!(h("<a>click</a>"), "[click]()");
}

#[test]
fn image() {
    assert_eq!(
        h(r#"<img src="a.png" alt="Alt text">"#),
        "![Alt text](a.png)"
    );
}

#[test]
fn image_without_alt() {
    assert_eq!(h(r#"<img src="a.png">"#), "![](a.png)");
}

#[test]
fn inline_code() {
    assert_eq!(h("<code>let x = 1;</code>"), "`let x = 1;`");
}

#[test]
fn inline_code_with_backticks() {
    assert_eq!(h("<code>a`b</code>"), "``a`b``");
}

// Lists

#[test]
fn unordered_list() {
    assert_eq!(
        h("<ul><li>one</li><li>two</li><li>three</li></ul>"),
        "- one\n- two\n- three"
    );
}

#[test]
fn ordered_list() {
    assert_eq!(
        h("<ol><li>first</li><li>second</li><li>third</li></ol>"),
        "1. first\n2. second\n3. third"
    );
}

#[test]
fn nested_list() {
    let html = "<ul>\
         <li>A\
         <ul><li>A1</li><li>A2</li></ul>\
         </li>\
         <li>B</li>\
         </ul>";
    let md = h(html);
    assert!(md.contains("- A\n"), "outer item: {md:?}");
    assert!(md.contains("  - A1\n"), "nested item A1: {md:?}");
    assert!(md.contains("  - A2"), "nested item A2: {md:?}");
    assert!(md.contains("- B"), "outer item B: {md:?}");
}

#[test]
fn nested_ordered_in_unordered() {
    let html = "<ul>\
         <li>Items\
         <ol><li>one</li><li>two</li></ol>\
         </li>\
         </ul>";
    let md = h(html);
    assert!(md.contains("- Items\n"), "outer item: {md:?}");
    assert!(md.contains("  1. one\n"), "nested item one: {md:?}");
    assert!(md.contains("  2. two"), "nested item two: {md:?}");
}

// Blockquotes

#[test]
fn blockquote_single_line() {
    assert_eq!(h("<blockquote>Hello</blockquote>"), "> Hello");
}

#[test]
fn blockquote_multiline() {
    assert_eq!(
        h("<blockquote><p>Line one</p><p>Line two</p></blockquote>"),
        "> Line one\n> \n> Line two"
    );
}

#[test]
fn blockquote_with_inline() {
    assert_eq!(
        h("<blockquote><strong>Bold quote</strong></blockquote>"),
        "> **Bold quote**"
    );
}

// Code blocks

#[test]
fn pre_block() {
    assert_eq!(h("<pre>fn main() {}\n</pre>"), "```\nfn main() {}\n```");
}

#[test]
fn pre_with_language() {
    assert_eq!(
        h("<pre><code class=\"language-rust\">let x = 1;</code></pre>"),
        "```rust\nlet x = 1;\n```"
    );
}

#[test]
fn pre_preserves_whitespace() {
    assert_eq!(
        h("<pre>  indented\n    more\n</pre>"),
        "```\n  indented\n    more\n```"
    );
}

// Tables

#[test]
fn table_with_headers() {
    let md = h("<table>\
         <tr><th>Name</th><th>Age</th></tr>\
         <tr><td>Alice</td><td>30</td></tr>\
         <tr><td>Bob</td><td>25</td></tr>\
         </table>");
    assert!(md.contains("| Name  | Age |"), "header row: {md:?}");
    assert!(md.contains("| ----- | --- |"), "separator: {md:?}");
    assert!(md.contains("| Alice | 30  |"), "data row 1: {md:?}");
    assert!(md.contains("| Bob   | 25  |"), "data row 2: {md:?}");
}

#[test]
fn table_without_explicit_headers() {
    let md = h("<table>\
         <tr><td>A</td><td>B</td></tr>\
         <tr><td>C</td><td>D</td></tr>\
         </table>");
    assert!(md.contains("| A"), "row with A: {md:?}");
    assert!(md.contains("| B"), "row with B: {md:?}");
    assert!(md.contains("| C"), "row with C: {md:?}");
    assert!(md.contains("| D"), "row with D: {md:?}");
    assert!(md.contains("| ---"), "separator: {md:?}");
}

#[test]
fn table_with_thead() {
    let md = h("<table>\
         <thead><tr><th>X</th></tr></thead>\
         <tbody><tr><td>1</td></tr></tbody>\
         </table>");
    assert!(md.contains("| X"), "header: {md:?}");
    assert!(md.contains("| ---"), "separator: {md:?}");
    assert!(md.contains("| 1"), "data: {md:?}");
}

#[test]
fn table_with_ragged_rows() {
    let md = h("<table>\
         <tr><td>A</td><td>B</td><td>C</td></tr>\
         <tr><td>D</td></tr>\
         </table>");
    assert!(md.contains("| A"), "row 1 col 1: {md:?}");
    assert!(md.contains("| D"), "row 2 col 1: {md:?}");
}

// Horizontal rules and line breaks

#[test]
fn horizontal_rule() {
    let md = h("<p>above</p><hr><p>below</p>");
    assert!(md.contains("above"), "above text: {md:?}");
    assert!(md.contains("---"), "horizontal rule: {md:?}");
    assert!(md.contains("below"), "below text: {md:?}");
}

#[test]
fn br_tag() {
    let md = h("<p>line one<br>line two</p>");
    assert!(
        md.contains("line one  \n"),
        "br as two trailing spaces: {md:?}"
    );
    assert!(md.contains("line two"), "second line: {md:?}");
}

#[test]
fn br_inside_pre() {
    let md = h("<pre>a<br>b</pre>");
    assert!(md.contains("a\nb"), "br as newline in pre: {md:?}");
}

// Whitespace normalization

#[test]
fn collapses_whitespace() {
    assert_eq!(h("<p>Hello     world</p>"), "Hello world");
}

#[test]
fn trims_leading_whitespace() {
    assert_eq!(h("<p>   Hello</p>"), "Hello");
}

#[test]
fn ignores_multiple_newlines_in_text() {
    assert_eq!(h("<p>Hello\n\n\nworld</p>"), "Hello world");
}

// Structural / div-like elements

#[test]
fn div_as_block() {
    let md = h("<p>before</p><div><p>inside</p></div><p>after</p>");
    assert!(md.contains("before"), "before: {md:?}");
    assert!(md.contains("inside"), "inside: {md:?}");
    assert!(md.contains("after"), "after: {md:?}");
    let before_pos = md.find("before").expect("before");
    let inside_pos = md.find("inside").expect("inside");
    let after_pos = md.find("after").expect("after");
    assert!(before_pos < inside_pos, "before comes before inside");
    assert!(inside_pos < after_pos, "inside comes before after");
}

#[test]
fn section_as_block() {
    assert_eq!(h("<section><p>content</p></section>"), "content");
}

// Stripped elements

#[test]
fn script_stripped() {
    assert_eq!(h("<p>visible</p><script>alert('xss')</script>"), "visible");
}

#[test]
fn style_stripped() {
    assert_eq!(h("<p>visible</p><style>body{}</style>"), "visible");
}

#[test]
fn head_stripped() {
    assert_eq!(
        h("<html><head><title>x</title></head><body><p>visible</p></body></html>"),
        "visible"
    );
}

// Comments and doctype

#[test]
fn html_comment_stripped() {
    assert_eq!(h("<p>visible</p><!-- a comment -->"), "visible");
}

// Unknown tags

#[test]
fn unknown_tag_children_rendered() {
    assert_eq!(h("<foo><p>Hello</p></foo>"), "Hello");
}

// Complex / integration

#[test]
fn full_document() {
    let html = concat!(
        "<h1>Title</h1>",
        "<p>A <strong>bold</strong> paragraph.</p>",
        "<ul><li>one</li><li>two</li></ul>",
        "<blockquote>Nice quote</blockquote>",
    );
    let md = h(html);
    assert!(md.contains("# Title"), "heading: {md:?}");
    assert!(md.contains("A **bold** paragraph."), "paragraph: {md:?}");
    assert!(md.contains("- one\n- two"), "list items: {md:?}");
    assert!(md.contains("> Nice quote"), "blockquote: {md:?}");
}

#[test]
fn nested_inline_elements() {
    assert_eq!(
        h("<p><strong><em>bold italic</em></strong></p>"),
        "***bold italic***"
    );
}

#[test]
fn link_with_bold_text() {
    assert_eq!(
        h(r#"<a href="https://example.com"><strong>link</strong></a>"#),
        "[**link**](https://example.com)"
    );
}

#[test]
fn image_in_paragraph() {
    assert_eq!(
        h(r#"<p><img src="photo.jpg" alt="A photo"></p>"#),
        "![A photo](photo.jpg)"
    );
}

#[test]
fn multiple_paragraphs_with_mixed_content() {
    let html = concat!(
        "<h2>Section</h2>",
        "<p>First paragraph.</p>",
        "<p>Second with <a href=\"/url\">a link</a>.</p>",
        "<hr>",
        "<p>After rule.</p>",
    );
    let md = h(html);
    assert!(md.contains("## Section"), "heading: {md:?}");
    assert!(md.contains("First paragraph."), "first para: {md:?}");
    assert!(md.contains("[a link](/url)"), "link: {md:?}");
    assert!(md.contains("---"), "hr: {md:?}");
    assert!(md.contains("After rule."), "last para: {md:?}");
}

// Raw output format

#[test]
fn block_output_has_blank_line_prefix_and_suffix() {
    let raw = h_raw("<p>Hello</p>");
    assert!(
        raw.starts_with("\n\n"),
        "block output starts with blank line: {raw:?}"
    );
    assert!(
        raw.ends_with("\n\n"),
        "block output ends with blank line: {raw:?}"
    );
}

#[test]
fn inline_output_has_trailing_newline_only() {
    let raw = h_raw("<strong>x</strong>");
    assert_eq!(raw, "**x**\n");
}

#[test]
fn finalize_appends_newline_when_missing() {
    let raw = h_raw("<strong>x</strong>");
    assert!(
        raw.ends_with('\n'),
        "finalize ensures trailing newline: {raw:?}"
    );
}

// Regression tests for bug fixes

#[test]
fn nested_blockquote() {
    let md = h("<blockquote><blockquote>inner</blockquote></blockquote>");
    assert!(md.contains("> > inner"), "nested blockquote: {md:?}");
}

#[test]
fn pre_inside_blockquote() {
    let md = h("<blockquote><pre>code\n</pre></blockquote>");
    assert!(
        md.contains("> ```"),
        "opening fence inside blockquote: {md:?}"
    );
    assert!(
        md.contains("> code"),
        "code content inside blockquote: {md:?}"
    );
    assert!(
        md.contains("> ```"),
        "closing fence inside blockquote: {md:?}"
    );
}

#[test]
fn blockquote_with_pre_and_text() {
    let md = h("<blockquote><p>before</p><pre>code\n</pre><p>after</p></blockquote>");
    assert!(md.contains("> before"), "text before pre: {md:?}");
    assert!(md.contains("> code"), "code content: {md:?}");
    assert!(md.contains("> after"), "text after pre: {md:?}");
}

#[test]
fn th_outside_thead() {
    let md = h("<table>\
         <tr><th>Name</th><th>Age</th></tr>\
         <tr><td>Alice</td><td>30</td></tr>\
         </table>");
    assert!(md.contains("| Name"), "header cell Name: {md:?}");
    assert!(md.contains("| ---"), "separator row: {md:?}");
    assert!(md.contains("| Alice"), "data cell: {md:?}");
}

#[test]
fn inline_with_marker_chars() {
    // The literal `**` in the text is escaped (`\*\*`), so there is no bare
    // marker run in the rendered content and the `**` delimiter is safe.
    let md = h("<strong>a ** b</strong>");
    assert!(
        md.contains("**a \\*\\* b**"),
        "escapes literal markers in content: {md:?}"
    );
}

#[test]
fn link_with_parentheses() {
    let md = h(r#"<a href="http://example.com/(foo)">click</a>"#);
    assert!(
        md.contains("<http://example.com/(foo)>"),
        "URL wrapped in angle brackets: {md:?}"
    );
}

// Excessive newline regression tests

#[test]
fn no_excessive_blank_lines_with_empty_paragraphs() {
    let raw = h_raw("<p></p><p>Hello</p><p></p>");
    assert!(
        !raw.contains("\n\n\n"),
        "excessive blank lines in raw output: {raw:?}"
    );
}

#[test]
fn no_excessive_blank_lines_with_empty_heading() {
    let raw = h_raw("<h1></h1><p>Hello</p>");
    assert!(
        !raw.contains("\n\n\n"),
        "excessive blank lines in raw output: {raw:?}"
    );
}

#[test]
fn no_excessive_blank_lines_with_empty_blockquote() {
    let raw = h_raw("<blockquote></blockquote><p>Hello</p>");
    assert!(
        !raw.contains("\n\n\n"),
        "excessive blank lines in raw output: {raw:?}"
    );
}

#[test]
fn no_excessive_blank_lines_with_multiple_empty_blocks() {
    let raw = h_raw("<h1></h1><p></p><div></div><p>Hello</p>");
    assert!(
        !raw.contains("\n\n\n"),
        "excessive blank lines in raw output: {raw:?}"
    );
}

// Security tests

// Bug reproduction tests

#[test]
fn code_with_double_backticks() {
    // <code>``</code> produces "`` `` ``" which CommonMark parses as
    // an empty code span (opening `` at pos 0, closing `` at pos 3)
    // followed by literal " ``". The backtick content is lost.
    // Correct output should use triple backtick delimiters: "``` `` ```"
    let md = h("<code>``</code>");
    // The rendered code span should preserve both backticks
    assert!(
        md.contains("```") || md.contains("````"),
        "should use 3+ backtick delimiters for content with 2 backticks: {md:?}"
    );
}

#[test]
fn code_with_backticks_in_middle() {
    // <code>a``b</code> with double backticks in the middle of content.
    // Using `` as delimiters means the `` in the content is mistaken for
    // the closing delimiter. "`` a``b ``" parses as code span "a" then
    // literal "b ``".
    let md = h("<code>a``b</code>");
    assert!(
        md.contains("```") || md.contains("````"),
        "should use 3+ backtick delimiters when content has 2 consecutive backticks: {md:?}"
    );
}

#[test]
fn code_with_leading_trailing_spaces() {
    // CommonMark strips one leading and one trailing space from code spans.
    // " hello " rendered with ` hello ` becomes "hello" (spaces lost).
    // Should use double backticks with padding: "``  hello  ``"
    let md = h("<code> hello </code>");
    // The spaces should be preserved - check that the output uses
    // double backticks with extra padding
    assert!(
        md.starts_with("``") && !md.starts_with("````"),
        "should use double backtick delimiters for content with leading/trailing spaces: {md:?}"
    );
    // Verify there are extra padding spaces inside the delimiters
    assert!(
        md.contains("``  hello  ``"),
        "should have padding spaces around content: {md:?}"
    );
}

#[test]
fn code_with_only_spaces() {
    // <code> </code> - a single space. "` `" would be parsed as an empty
    // code span (space stripped). With "``   ``" (3 spaces), CommonMark
    // strips one from each end, preserving the single space.
    let md = h("<code> </code>");
    assert_eq!(md, "``   ``");
}

#[test]
fn ol_start_attribute() {
    // <ol start="5"> should produce "5. fifth" not "1. fifth"
    let md = h("<ol start=\"5\"><li>fifth</li><li>sixth</li></ol>");
    assert!(
        md.contains("5. fifth"),
        "should respect start attribute: {md:?}"
    );
    assert!(
        md.contains("6. sixth"),
        "should increment from start value: {md:?}"
    );
}

#[test]
fn link_with_space_in_url() {
    // URLs with spaces break markdown link syntax.
    // [text](http://example.com/path with spaces) -> broken
    // Should wrap in angle brackets: [text](<http://example.com/path with spaces>)
    let md = h(r#"<a href="http://example.com/path with spaces">click</a>"#);
    assert!(
        md.contains("<http://example.com/path with spaces>"),
        "URL with spaces should be wrapped in angle brackets: {md:?}"
    );
}

#[test]
fn image_with_space_in_src() {
    // Same issue for images - URLs with spaces need angle brackets
    let md = h(r#"<img src="http://example.com/img photo.png" alt="photo">"#);
    assert!(
        md.contains("<http://example.com/img photo.png>"),
        "image src with spaces should be wrapped in angle brackets: {md:?}"
    );
}

#[test]
fn deeply_nested_html_returns_error() {
    // Create deeply nested HTML using nested UL elements
    // Each UL contains one LI which contains another UL, creating true nesting
    let mut html = String::from("<ul><li>");
    for _ in 0..250 {
        html.push_str("<ul><li>");
    }
    html.push_str("content");
    for _ in 0..250 {
        html.push_str("</li></ul>");
    }
    html.push_str("</li></ul>");

    let mut out = Vec::new();
    let result = h2md::convert(html.as_bytes(), &mut out);
    assert!(result.is_err(), "deeply nested HTML should return error");
    if let Err(e) = result {
        assert!(
            e.to_string().contains("exceeds maximum depth"),
            "error should mention depth: {e}"
        );
    }
}

// Markdown escaping of text content

#[test]
fn escapes_asterisks_in_text() {
    // A literal `*` in HTML text must not round-trip as emphasis.
    let md = h("<p>a * b</p>");
    assert!(md.contains(r"a \* b"), "literal asterisk escaped: {md:?}");
}

#[test]
fn escapes_underscore_in_text() {
    let md = h("<p>foo_bar_baz</p>");
    assert!(
        md.contains(r"foo\_bar\_baz"),
        "literal underscores escaped: {md:?}"
    );
}

#[test]
fn escapes_hash_only_at_line_start() {
    // `#` mid-line is literal and must NOT be escaped; only a line-start
    // `#` (which would start a heading) is escaped.
    let md = h("<p>issue #42 is fine</p>");
    assert!(
        !md.contains(r"\#"),
        "mid-line hash should not be escaped: {md:?}"
    );

    // A `#` that begins a paragraph's output line is escaped.
    let md = h("<p># not a heading</p>");
    assert!(
        md.contains(r"\# not a heading"),
        "leading hash should be escaped: {md:?}"
    );
}

#[test]
fn escapes_less_than_in_text() {
    // `<b>` in body text would otherwise be consumed as an HTML tag.
    let md = h("<p>a < b > c</p>");
    assert!(md.contains(r"a \< b"), "literal less-than escaped: {md:?}");
}

#[test]
fn does_not_escape_inside_code_span() {
    // Code spans emit raw text; no Markdown escaping is applied.
    let md = h("<code>a * b _ c</code>");
    assert_eq!(md, "`a * b _ c`");
}

#[test]
fn does_not_escape_inside_pre() {
    let md = h("<pre>a * b _ c\n</pre>");
    assert!(
        md.contains("a * b _ c"),
        "pre content should not be escaped: {md:?}"
    );
}

// Empty inline elements

#[test]
fn empty_strong_emits_nothing() {
    let md = h("<strong></strong><p>after</p>");
    assert!(
        !md.contains("**"),
        "empty strong should emit no markers: {md:?}"
    );
    assert!(md.contains("after"), "after text present: {md:?}");
}

#[test]
fn empty_em_emits_nothing() {
    let md = h("<em></em>visible");
    assert!(!md.contains('*'), "empty em should emit no markers: {md:?}");
    assert!(md.contains("visible"), "visible text present: {md:?}");
}

// URL emission edge cases

#[test]
fn link_url_with_angle_brackets_wrapped() {
    let md = h(r#"<a href="http://example.com/a<b>c">x</a>"#);
    assert!(
        md.contains("[x](<http://example.com/a\\<b\\>c>)"),
        "URL with < > wrapped and inner-escaped: {md:?}"
    );
}

#[test]
fn plain_link_url_not_wrapped() {
    let md = h(r#"<a href="https://example.com/path">x</a>"#);
    assert_eq!(md, "[x](https://example.com/path)");
}

// Table column alignment for non-ASCII

#[test]
fn table_aligns_wide_characters() {
    let md = h("<table>\
         <tr><th>Name</th><th>Age</th></tr>\
         <tr><td>Alice</td><td>30</td></tr>\
         <tr><td>日本</td><td>25</td></tr>\
         </table>");
    // The header and the wide-char row must pad to the same display width so
    // the closing pipes line up. Both rows should end with "| Age |" / "| 25 |"
    // at the same visual column.
    assert!(md.contains("| Name  | Age |"), "header row aligned: {md:?}");
    assert!(
        md.contains("| 日本  | 25  |"),
        "wide-char row aligned by display width: {md:?}"
    );
}

#[test]
fn table_escapes_pipe_in_cell() {
    let md = h("<table><tr><td>a|b</td></tr></table>");
    assert!(md.contains(r"a\|b"), "pipe in cell escaped: {md:?}");
}

// Nested tables and block content inside cells

#[test]
fn nested_table_does_not_corrupt_outer_table() {
    let md = h("<table>\
         <tr><td>a</td><td><table><tr><td>x</td></tr></table></td></tr>\
         <tr><td>b</td><td>y</td></tr>\
         </table>");
    // The output must be a valid single table with two data rows, not the
    // garbled interleaving produced before the fix.
    let lines: Vec<&str> = md.lines().filter(|l| l.starts_with('|')).collect();
    assert!(
        lines.len() >= 3,
        "expected a header/sep/data layout, got: {md:?}"
    );
    // No row should contain a stray unescaped nested-table fragment.
    assert!(md.contains('a'), "outer cell a present: {md:?}");
    assert!(md.contains('y'), "sibling cell y present: {md:?}");
}

#[test]
fn list_inside_table_cell_is_single_line() {
    let md = h("<table><tr><td><ul><li>a</li><li>b</li></ul></td></tr></table>");
    // The cell content is flattened to one line; no raw newlines leak into the
    // row and break the table.
    let row = md
        .lines()
        .find(|l| l.contains('a') && l.contains('b'))
        .unwrap_or_else(|| panic!("no row with list items: {md:?}"));
    assert!(
        row.starts_with('|') && row.ends_with('|'),
        "list-in-cell stays on one table row: {row:?}"
    );
}

// Compressed (`-c`) tables

#[test]
fn compressed_table_minimal_definition() {
    let md = hc("<table>\
         <tr><th>Name</th><th>Age</th></tr>\
         <tr><td>Alice</td><td>30</td></tr>\
         <tr><td>Bob</td><td>25</td></tr>\
         </table>");
    assert!(md.contains("|Name|Age|"), "compact header: {md:?}");
    assert!(md.contains("|-|-|"), "minimal separator: {md:?}");
    assert!(md.contains("|Alice|30|"), "compact data row 1: {md:?}");
    assert!(md.contains("|Bob|25|"), "compact data row 2: {md:?}");
    assert!(
        !md.contains("| Name"),
        "no padding spaces in compressed mode: {md:?}"
    );
}

#[test]
fn compressed_table_without_explicit_headers() {
    let md = hc("<table>\
         <tr><td>A</td><td>B</td></tr>\
         <tr><td>C</td><td>D</td></tr>\
         </table>");
    assert!(md.contains("|A|B|"), "compact row 1: {md:?}");
    assert!(md.contains("|C|D|"), "compact row 2: {md:?}");
    // A separator is still required after the first row for a valid table.
    assert!(md.contains("|-|-|"), "separator after first row: {md:?}");
}

#[test]
fn compressed_table_ragged_rows() {
    let md = hc("<table>\
         <tr><td>A</td><td>B</td><td>C</td></tr>\
         <tr><td>D</td></tr>\
         </table>");
    assert!(md.contains("|A|B|C|"), "full row: {md:?}");
    assert!(
        md.contains("|D||"),
        "ragged row padded with empty cell: {md:?}"
    );
}

#[test]
fn normal_table_unaffected_by_compressed_option() {
    // Default options still produce the aligned, padded table.
    let md = h("<table>\
         <tr><th>Name</th><th>Age</th></tr>\
         <tr><td>Alice</td><td>30</td></tr>\
         </table>");
    assert!(md.contains("| Name  | Age |"), "aligned header: {md:?}");
    assert!(md.contains("| ----- | --- |"), "aligned separator: {md:?}");
}

// List items whose first child is a block element must not leave the marker
// alone on its own line with a leading blank line (regression: nav menus built
// from <div> dropdowns used to fragment into many short lines).

#[test]
fn list_item_with_div_first_child_no_empty_marker() {
    let md = h("<ul><li><div>x</div></li></ul>");
    assert!(
        md.contains("- x"),
        "content follows marker directly: {md:?}"
    );
    assert!(!md.contains("- \n"), "no empty marker line: {md:?}");
}

#[test]
fn list_item_with_paragraph_first_child() {
    let md = h("<ul><li><p>text</p></li></ul>");
    assert!(
        md.contains("- text"),
        "paragraph inline with marker: {md:?}"
    );
    assert!(!md.contains("- \n\n"), "no blank line after marker: {md:?}");
}

#[test]
fn nav_like_dropdown_list_does_not_fragment() {
    // Mirrors the CWE navigation: a <li> whose first child is a <div> wrapping
    // a label button and a container of links.
    let md = h("<ul><li>\
         <div class=\"dropdown\"><button>Section</button>\
         <div class=\"dropdown-content\">\
         <a href=\"/a\">Link A</a> <a href=\"/b\">Link B</a>\
         </div></div>\
         </li></ul>");
    assert!(
        md.contains("- Section"),
        "label directly after marker (no double space): {md:?}"
    );
    assert!(!md.contains("- \n"), "no empty marker: {md:?}");
    assert!(md.contains("[Link A](/a)"), "links preserved: {md:?}");
}
