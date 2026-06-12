use crate::send::{QueryParamValue, percent_encode};

#[test]
fn test_percent_encode() {
    assert_eq!(percent_encode(QueryParamValue("a-z")), "a-z");
    assert_eq!(percent_encode(QueryParamValue("a b")), "a%20b");
    assert_eq!(percent_encode(QueryParamValue("a@b")), "a%40b");
}
