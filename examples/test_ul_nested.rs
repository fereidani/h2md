use h2md::convert;

fn main() {
    // Create deeply nested UL elements (each UL contains one LI which contains
    // another UL)
    let mut html = String::from("<ul><li>");
    for _ in 0..200 {
        html.push_str("<ul><li>");
    }
    html.push_str("content");
    for _ in 0..200 {
        html.push_str("</li></ul>");
    }
    html.push_str("</li></ul>");

    println!("HTML length: {}", html.len());

    let mut out = Vec::new();
    let result = convert(html.as_bytes(), &mut out);
    println!("Result: {:?}", result);
    if let Err(e) = &result {
        println!("Error: {}", e);
    }
}
