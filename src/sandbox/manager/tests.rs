use super::*;

#[test]
fn test_exec_output_from_container_output() {
    let container = ContainerOutput {
        exit_code: 0,
        stdout: "hello".to_string(),
        stderr: String::new(),
        duration: Duration::from_secs(1),
        truncated: false,
    };

    let exec: ExecOutput = container.into();
    assert_eq!(exec.exit_code, 0);
    assert_eq!(exec.output, "hello");
}

#[test]
fn test_exec_output_combined() {
    let container = ContainerOutput {
        exit_code: 1,
        stdout: "out".to_string(),
        stderr: "err".to_string(),
        duration: Duration::from_secs(1),
        truncated: false,
    };

    let exec: ExecOutput = container.into();
    assert!(exec.output.contains("out"));
    assert!(exec.output.contains("err"));
    assert!(exec.output.contains("stderr"));
}

#[test]
fn test_builder_defaults() {
    let manager = SandboxManagerBuilder::new().build();
    assert!(manager.config.enabled); // Enabled by default (startup check disables if Docker unavailable)
}

#[test]
fn test_builder_custom() {
    let manager = SandboxManagerBuilder::new()
        .enabled(true)
        .policy(SandboxPolicy::WorkspaceWrite)
        .timeout(Duration::from_secs(60))
        .memory_limit_mb(1024)
        .image("custom:latest")
        .build();

    assert!(manager.config.enabled);
    assert_eq!(manager.config.policy, SandboxPolicy::WorkspaceWrite);
    assert_eq!(manager.config.timeout, Duration::from_secs(60));
    assert_eq!(manager.config.memory_limit_mb, 1024);
    assert_eq!(manager.config.image, "custom:latest");
}

#[tokio::test]
async fn test_direct_execution() {
    let manager = SandboxManager::new(SandboxConfig {
        enabled: true,
        policy: SandboxPolicy::FullAccess,
        ..Default::default()
    });

    let result = manager
        .execute("echo hello", Path::new("."), HashMap::new())
        .await;

    // This should work even without Docker since FullAccess runs directly
    assert!(result.is_ok());
    let output = result.expect("expected direct execution to succeed");
    assert!(output.stdout.contains("hello"));
}

#[tokio::test]
async fn test_direct_execution_truncates_large_output() {
    let manager = SandboxManager::new(SandboxConfig {
        enabled: true,
        policy: SandboxPolicy::FullAccess,
        ..Default::default()
    });

    // Generate output larger than 32KB (half of 64KB limit)
    // printf repeats a 100-char line 400 times = 40KB
    let result = manager
        .execute(
            "printf 'A%.0s' $(seq 1 40000)",
            Path::new("."),
            HashMap::new(),
        )
        .await;

    assert!(result.is_ok());
    let output = result.expect("expected large direct execution to succeed");
    assert!(output.truncated);
    assert!(output.stdout.len() <= 32 * 1024);
}
