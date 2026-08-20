use lookup::tools::torrent::{
    is_torrent_query, parse_magnet, parse_torrent, torrent_links_in_text,
};

#[test]
fn test_torrent_intent_detection() {
    assert!(is_torrent_query("download ubuntu torrent"));
    assert!(is_torrent_query("find debian magnet link"));
    assert!(is_torrent_query("get file.torrent"));
    assert!(is_torrent_query(
        "magnet:?xt=urn:btih:abcdef1234567890abcdef1234567890abcdef12"
    ));
    assert!(!is_torrent_query("how to write rust code"));
    assert!(!is_torrent_query("weather in Vancouver"));
}

#[test]
fn test_parse_magnet_hex() {
    let raw = "magnet:?xt=urn:btih:da39a3ee5e6b4b0d3255bfef95601890afd80709&dn=Ubuntu+22.04&tr=http%3A%2F%2Ftracker.example.com%2Fannounce";
    let mag = parse_magnet(raw).unwrap();
    assert_eq!(mag.info_hash, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(mag.name, "Ubuntu 22.04");
    assert_eq!(mag.trackers.len(), 1);
}

#[test]
fn test_parse_magnet_base32() {
    // 32-char Base32 hash
    let raw = "magnet:?xt=urn:btih:3G2A4XR6NNVQ2MRVX3XZKYAYSCX5QB4J&dn=Debian";
    let mag = parse_magnet(raw).unwrap();
    assert_eq!(mag.info_hash.len(), 40);
    assert_eq!(mag.name, "Debian");
}

#[test]
fn test_parse_torrent_bencode() {
    // Construct a minimal valid bencoded torrent dictionary:
    // d4:infod6:lengthi1024e4:name4:testee
    let data = b"d4:infod6:lengthi1024e4:name4:testee";
    let info = parse_torrent(data).unwrap();
    assert_eq!(info.name, "test");
    assert_eq!(info.size_bytes, 1024);
    assert_eq!(info.info_hash.len(), 40);
}

#[test]
fn test_torrent_links_in_text() {
    let text = "Here is the link: magnet:?xt=urn:btih:da39a3ee5e6b4b0d3255bfef95601890afd80709 and also https://example.com/arch.torrent! Enjoy.";
    let links = torrent_links_in_text(text);
    assert_eq!(links.len(), 2);
    assert!(links[0].starts_with("magnet:"));
    assert!(links[1].ends_with(".torrent"));
}
