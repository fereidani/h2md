use h2md::convert;

fn main() {
    // Create truly nested HTML by nesting tags within each other
    let mut html = String::from("<div>");
    for _ in 0..50 {
        html.push_str("<div>");
    }
    html.push_str("<p>content</p>");
    for _ in 0..50 {
        html.push_str("</div>");
    }
    html.push_str("</div>");

    println!("HTML length: {}", html.len());

    let mut out = Vec::new();
    let result = convert(html.as_bytes(), &mut out);
    println!("Result: {:?}", result);
    if result.is_ok() {
        println!("Output length: {}", out.len());
        let md = String::from_utf8_lossy(&out);
        println!("Output: {}", md);
    }
}
