use h2md::convert;

fn main() {
    // Create truly nested HTML by nesting tags within each other
    let mut html = String::from("<div>");
    for _ in 0..200 {
        html.push_str("<div>");
    }
    html.push_str("<p>content</p>");
    for _ in 0..200 {
        html.push_str("</div>");
    }
    html.push_str("</div>");

    let mut out = Vec::new();
    let result = convert(html.as_bytes(), &mut out);
    println!("Result: {:?}", result);
    if let Err(e) = result {
        println!("Error: {}", e);
    }
}
