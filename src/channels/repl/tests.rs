//! Tests for the REPL channel behavior and message handling.

use futures::StreamExt;

use super::*;

#[tokio::test]
async fn single_message_mode_sends_message_then_quit() {
    let repl = ReplChannel::with_message("hi".to_string());
    let mut stream = repl.start().await.expect("repl start should succeed");

    let first = stream.next().await.expect("first message missing");
    assert_eq!(first.channel, "repl");
    assert_eq!(first.content, "hi");

    let second = stream.next().await.expect("quit message missing");
    assert_eq!(second.channel, "repl");
    assert_eq!(second.content, "/quit");

    assert!(
        stream.next().await.is_none(),
        "stream should end after /quit"
    );
}
