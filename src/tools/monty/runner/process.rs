//! Child-process plumbing for the supervisor.
//!
//! Everything that touches the helper's pipes or process handle lives here, so
//! the supervision logic in [`super`] can be read as a state machine over
//! frames rather than a mix of protocol and process handling.

use std::process::Stdio;
use std::time::Duration;

use monty_protocol::{
    CancelRun, HelperToParent, MAX_FRAME_BYTES, ParentToHelper, decode_frame, encode_frame,
    validate_json_text,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, Command};

use super::{HelperProgram, RunError};

/// Starts a helper with cleared environment and piped standard streams.
pub(super) async fn spawn(program: &HelperProgram) -> Result<Child, RunError> {
    Command::new(program.path())
        .kill_on_drop(true)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("RUST_BACKTRACE", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunError::Spawn)
}

/// Asks the helper to cancel, then kills and reaps it.
pub(super) async fn terminate<W: AsyncWrite + Unpin>(
    child: &mut Child,
    run_id: &str,
    stdin: &mut W,
) {
    let cancel = CancelRun {
        run_id: run_id.to_string(),
    };
    let _ = write_message(stdin, &ParentToHelper::CancelRun(cancel)).await;
    let _ = stdin.shutdown().await;
    let _ = child.start_kill();
    let _ = child.wait().await;
}

/// Gives a finished helper one second to exit, then kills and reaps it.
pub(super) async fn finish_child(child: &mut Child) {
    if tokio::time::timeout(Duration::from_secs(1), child.wait())
        .await
        .is_err()
    {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
}

/// Drains helper diagnostics without retaining or logging their contents.
pub(super) async fn drain_stderr<R: AsyncRead + Unpin>(mut stderr: R) {
    let mut buffer = [0_u8; 1024];
    while matches!(stderr.read(&mut buffer).await, Ok(read) if read > 0) {}
}

/// Reads one bounded, validated frame from the helper.
pub(super) async fn read_message<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<HelperToParent, RunError> {
    let mut prefix = [0_u8; 4];
    reader
        .read_exact(&mut prefix)
        .await
        .map_err(|_| RunError::RuntimeCrash("helper closed the protocol stream".to_string()))?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(RunError::RuntimeCrash("invalid frame length".to_string()));
    }
    let mut payload = vec![0_u8; length];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|_| RunError::RuntimeCrash("truncated helper frame".to_string()))?;
    let text = std::str::from_utf8(&payload)
        .map_err(|_| RunError::RuntimeCrash("helper frame is not UTF-8".to_string()))?;
    validate_json_text(text)
        .map_err(|_| RunError::RuntimeCrash("helper frame exceeds JSON limits".to_string()))?;
    decode_frame(&payload)
        .map_err(|_| RunError::RuntimeCrash("helper frame is not a valid message".to_string()))
}

/// Writes one frame to the helper.
pub(super) async fn write_message<W: AsyncWrite + Unpin, T: serde::Serialize>(
    writer: &mut W,
    message: &T,
) -> Result<(), RunError> {
    let framed = encode_frame(message)
        .map_err(|_| RunError::RuntimeCrash("message exceeds the frame limit".to_string()))?;
    writer
        .write_all(&framed)
        .await
        .map_err(|_| RunError::RuntimeCrash("cannot write to the helper".to_string()))
}
