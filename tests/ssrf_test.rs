use lookup::net::ssrf::{is_global_ip, validate_url};
use std::net::IpAddr;

#[test]
fn test_private_ip_detection() {
    let loopback: IpAddr = "127.0.0.1".parse().unwrap();
    assert!(!is_global_ip(&loopback));

    let loopback_v6: IpAddr = "::1".parse().unwrap();
    assert!(!is_global_ip(&loopback_v6));

    let priv1: IpAddr = "10.0.0.5".parse().unwrap();
    assert!(!is_global_ip(&priv1));

    let priv2: IpAddr = "192.168.1.1".parse().unwrap();
    assert!(!is_global_ip(&priv2));

    let priv3: IpAddr = "172.16.0.1".parse().unwrap();
    assert!(!is_global_ip(&priv3));

    let link_local: IpAddr = "169.254.169.254".parse().unwrap();
    assert!(!is_global_ip(&link_local));

    let public_dns: IpAddr = "8.8.8.8".parse().unwrap();
    assert!(is_global_ip(&public_dns));

    let cloudflare: IpAddr = "1.1.1.1".parse().unwrap();
    assert!(is_global_ip(&cloudflare));
}

#[test]
fn test_validate_url_blocking() {
    assert!(validate_url("http://localhost:8080", false).is_err());
    assert!(validate_url("http://127.0.0.1/admin", false).is_err());
    assert!(validate_url("http://192.168.1.1/", false).is_err());
    assert!(validate_url("http://169.254.169.254/latest/meta-data/", false).is_err());
    assert!(validate_url("ftp://example.com/file", false).is_err());
    assert!(validate_url("http://user:pass@example.com", false).is_err());
    assert!(validate_url("https://example.com", false).is_ok());
}
