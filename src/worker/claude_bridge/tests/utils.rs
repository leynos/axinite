use super::truncate;

#[test]
fn test_truncate() {
    assert_eq!(truncate("hello", 10), "hello");
    assert_eq!(truncate("hello world", 5), "hello");
    assert_eq!(truncate("", 5), "");
}
