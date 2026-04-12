# h2md -- HTML to Markdown Converter

[![Crates.io](https://img.shields.io/crates/v/h2md.svg?style=for-the-badge)](https://crates.io/crates/h2md)
[![Documentation](https://img.shields.io/docsrs/h2md?style=for-the-badge)](https://docs.rs/h2md)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg?style=for-the-badge)](LICENSE)

**h2md** converts HTML to clean, readable Markdown using a browser-grade HTML
parser. It handles malformed real-world HTML the same way a browser does --
gracefully -- because it uses [html5ever], the same parser engine that powers
the [Servo] browser project.

[html5ever]: https://crates.io/crates/html5ever
[Servo]: https://servo.org/

## Key Features

- **Browser-grade parser**: Uses html5ever (Servo's HTML engine) for
  standards-compliant parsing with full error recovery -- no regex hacks
- **Zero-allocation output**: Writes Markdown directly to any `Write` target;
  no intermediate string construction
- **CLI and library**: Use as a command-line tool or as a Rust library
- **Comprehensive element support**: Headings, paragraphs, inline formatting,
  links, images, lists (with nesting), blockquotes, code blocks, tables,
  horizontal rules
- **Correct edge-case handling**: Proper backtick escaping in code spans,
  alternative delimiter selection, angle-bracket wrapping for URLs with spaces
  or parentheses, `ol start` attribute support
- **Safe against malicious input**: Recursion depth bounded to 200 levels to
  prevent stack overflow on deeply nested HTML

## Installation

```
cargo add h2md
```

Or install the CLI:

```
cargo install h2md
```

## Quick Start

### Command Line

```sh
# from a file
h2md input.html -o output.md

# from stdin
curl -s https://example.com | h2md

# pipe into other tools
h2md page.html | wc -l
```

### Library

#### One-shot Conversion

```rust
use h2md::convert;

let html = b"<h1>Title</h1><p>A <strong>bold</strong> paragraph.</p>";
let mut out = Vec::new();
convert(html, &mut out)?;

let md = String::from_utf8(out)?;
assert!(md.contains("# Title"));
assert!(md.contains("**bold**"));
```

#### Stream to File

```rust
use h2md::convert;
use std::fs::File;

let html = b"<ul><li>one</li><li>two</li></ul>";
let mut file = File::create("output.md")?;
convert(html, &mut file)?;
```

## API Reference

### `convert(html: &[u8], out: &mut impl Write) -> Result<(), Error>`

Parse HTML and write Markdown directly to a `Write` target. The output ends
with a trailing newline. Returns an error if the HTML cannot be parsed or if
writing fails.

### `Error`

| Variant         | Description                  |
| --------------- | ---------------------------- |
| `Parse(String)` | HTML parsing failed          |
| `Io(io::Error)` | Writing to the output failed |

## Supported Elements

| HTML                                      | Markdown                                              |
| ----------------------------------------- | ----------------------------------------------------- |
| `<h1>` .. `<h6>`                          | `# ` .. `###### `                                     |
| `<p>`                                     | text with blank line separation                       |
| `<strong>`, `<b>`                         | `**...**` (or `__...__` if content contains `*`)      |
| `<em>`, `<i>`                             | `*...*` (or `_..._` if content contains `*`)          |
| `<del>`, `<s>`, `<strike>`                | `~~...~~`                                             |
| `<code>`                                  | `` `...` `` with automatic delimiter escaping         |
| `<a href>`                                | `[text](url)` with angle-bracket wrapping when needed |
| `<img>`                                   | `![alt](src)`                                         |
| `<ul>`, `<ol>`                            | `- ` / `1. ` with nesting support                     |
| `<blockquote>`                            | `> ` prefix with proper nesting                       |
| `<pre>`, `<pre><code class="language-*">` | fenced code block with language tag                   |
| `<table>`                                 | pipe-aligned table with header detection              |
| `<hr>`                                    | `---`                                                 |
| `<br>`                                    | two trailing spaces (newline inside `<pre>`)          |

The following elements are stripped from output: `<script>`, `<style>`,
`<noscript>`, `<head>`, `<meta>`, `<link>`, HTML comments, and doctype
declarations.

## Why html5ever

Most HTML-to-Markdown converters use regex-based extraction or lenient
tag parsers. These break on real-world HTML: missing closing tags, nested
comments, mixed-case element names, entities, malformed attributes, and all
the other chaos that browsers quietly tolerate.

html5ever implements the full [HTML5 specification] parsing algorithm. It
recovers from errors the same way browsers do, producing a consistent DOM
regardless of input quality. This means h2md produces correct output on HTML
that would break a regex-based converter -- without any special-casing.

[HTML5 specification]: https://html.spec.whatwg.org/multipage/parsing.html

## Testing

```
cargo test
```

## Contributing

Contributions are welcome! Please:

1. Run `cargo +nightly fmt` and `cargo clippy` before submitting
2. Add tests for new functionality
3. Update documentation as needed

## License

Licensed under the [MIT License](LICENSE).

---

**Author**: Khashayar Fereidani
**Repository**: [github.com/fereidani/h2md](https://github.com/fereidani/h2md)
