use lookup::html::{clean_blocks, parse_html, truncate_text};

#[test]
fn test_html_parsing_and_metadata() {
    let html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>Rust Programming Language</title>
            <meta name="description" content="A language empowering everyone to build reliable and efficient software.">
        </head>
        <body>
            <nav><a href="/home">Home</a></nav>
            <header><h1>Welcome</h1></header>
            <main>
                <p>Rust is fast and memory-efficient.</p>
                <p>It has no runtime or garbage collector.</p>
                <a href="https://doc.rust-lang.org">Documentation</a>
                <a href="/install">Install Rust</a>
            </main>
            <footer><p>Copyright 2026</p></footer>
        </body>
        </html>
    "#;

    let parsed = parse_html(html, "https://www.rust-lang.org/", 5000);
    assert_eq!(parsed.title, "Rust Programming Language");
    assert_eq!(
        parsed.description,
        "A language empowering everyone to build reliable and efficient software."
    );
    assert!(parsed.content.contains("Rust is fast and memory-efficient."));
    assert!(parsed.content.contains("It has no runtime or garbage collector."));

    let doc_link = parsed.links.iter().find(|l| l.url.starts_with("https://doc.rust-lang.org"));
    assert!(doc_link.is_some());

    let install_link = parsed.links.iter().find(|l| l.url == "https://www.rust-lang.org/install");
    assert!(install_link.is_some());
}

#[test]
fn test_clean_blocks_filters_junk() {
    let blocks = vec![
        "Accept Cookies".to_string(),
        "Home".to_string(),
        "Valid main article content that contains useful information about the topic.".to_string(),
        "Valid main article content that contains useful information about the topic.".to_string(), // Duplicate
    ];
    let cleaned = clean_blocks(&blocks);
    assert_eq!(cleaned.len(), 1);
    assert!(cleaned[0].starts_with("Valid main article content"));
}

#[test]
fn test_truncate_text_boundaries() {
    let long_text = "First paragraph.\n\nSecond paragraph has some extra details.\n\nThird paragraph.";
    let truncated = truncate_text(long_text, 40);
    assert!(truncated.ends_with("[content truncated]"));
    assert!(truncated.contains("First paragraph."));
}
