//! Small utility tests for shared Claude bridge helpers such as `truncate`.

use super::truncate;

#[test]
fn test_truncate() {
    assert_eq!(truncate("hello", 10), "hello");
    assert_eq!(truncate("hello world", 5), "hello");
    assert_eq!(truncate("héllo", 2), "h");
    assert_eq!(truncate("", 5), "");
}
