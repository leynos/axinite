use crate::state::CHANNEL_NAME;

#[test]
fn test_channel_name_constant() {
    assert_eq!(CHANNEL_NAME, "telegram");
}
