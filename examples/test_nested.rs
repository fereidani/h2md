use h2md::convert;

fn main() {
    let open_tags = "<div>".repeat(150);
    let close_tags = "</div>".repeat(150);
    let html = format!("{open_tags}<p>content</p>{close_tags}");
    let mut out = Vec::new();
    let result = convert(html.as_bytes(), &mut out);
    println!("Result: {result:?}");
    if result.is_ok() {
        println!("Output length: {}", out.len());
        let md = String::from_utf8_lossy(&out);
        println!("First 200 chars: {}", &md[..md.len().min(200)]);
    }
}
