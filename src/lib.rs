#![deny(warnings)]

//! # h2md - HTML to Markdown Converter
//!
//! This library converts HTML to clean Markdown format.
//!
//! ## Features
//!
//! - Headings (h1-h6)
//! - Paragraphs and line breaks
//! - Inline formatting (bold, italic, strikethrough, code)
//! - Links and images
//! - Ordered and unordered lists (with nesting)
//! - Definition lists
//! - Blockquotes
//! - Code blocks with language detection
//! - Tables
//! - Horizontal rules
//!
//! ## Example
//!
//! ```rust
//! use h2md::convert;
//!
//! let html = b"<h1>Hello</h1><p>World</p>";
//! let mut out = Vec::new();
//! convert(html, &mut out)?;
//! let result = String::from_utf8(out).unwrap();
//! assert!(result.contains("# Hello"));
//! assert!(result.contains("World"));
//! # Ok::<(), h2md::Error>(())
//! ```

mod converter;
mod dom;

pub use converter::{Error, Options, convert, convert_with};
