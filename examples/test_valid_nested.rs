use h2md::convert;

fn main() {
    // Create valid nested HTML using tables (which should preserve nesting)
    let mut html = String::from("<table>");
    for _ in 0..1100 {
        html.push_str("<tr><td>");
    }
    html.push_str("content");
    for _ in 0..1100 {
        html.push_str("</td></tr>");
    }
    html.push_str("</table>");

    println!("HTML length: {}", html.len());

    let mut out = Vec::new();
    let result = convert(html.as_bytes(), &mut out);
    println!("Result: {result:?}");
    if let Err(e) = &result {
        println!("Error: {e}");
    }
}
