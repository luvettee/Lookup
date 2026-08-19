use lookup::budget::enforce_output_budget;
use serde_json::json;

#[test]
fn test_budget_small_payload() {
    let payload = json!({
        "status": "ok",
        "results": [{"title": "Test", "url": "https://example.com"}]
    });
    let budgeted = enforce_output_budget(payload.clone(), Some(1000));
    assert_eq!(payload, budgeted);
}

#[test]
fn test_budget_shrinks_content() {
    let large_string = "A".repeat(5000);
    let payload = json!({
        "title": "Short Title",
        "content": large_string,
        "results": [{"url": "https://example.com", "snippet": "B".repeat(2000)}]
    });

    let budgeted = enforce_output_budget(payload, Some(500));
    let serialized = serde_json::to_string(&budgeted).unwrap();
    assert!(serialized.len() <= 500);
}
