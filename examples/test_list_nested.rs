use h2md::convert;

fn main() {
    // Create deeply nested lists (which should preserve nesting structure)
    let mut html = String::from("<ul>");
    for _ in 0..1100 {
        html.push_str("<li>");
    }
    html.push_str("content");
    for _ in 0..1100 {
        html.push_str("</li>");
    }
    html.push_str("</ul>");

    println!("HTML length: {}", html.len());

    let mut out = Vec::new();
    let result = convert(html.as_bytes(), &mut out);
    println!("Result: {:?}", result);
    if let Err(e) = &result {
        println!("Error: {}", e);
    }
}
