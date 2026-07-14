//! Safety policy for shell execution: blocked/dangerous command patterns,
//! auto-approval exclusions, safe environment variables, and command
//! injection/obfuscation detection.

use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Duration;

/// Maximum output size before truncation (64KB).
pub(super) const MAX_OUTPUT_SIZE: usize = 64 * 1024;

/// Default command timeout.
pub(super) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Commands that are always blocked for safety.
pub(super) static BLOCKED_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "rm -rf /",
        "rm -rf /*",
        ":(){ :|:& };:", // Fork bomb
        "dd if=/dev/zero",
        "mkfs",
        "chmod -R 777 /",
        "> /dev/sda",
        "curl | sh",
        "wget | sh",
        "curl | bash",
        "wget | bash",
    ])
});

/// Patterns that indicate potentially dangerous commands.
pub(super) static DANGEROUS_PATTERNS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "sudo ",
        "doas ",
        " | sh",
        " | bash",
        " | zsh",
        "eval ",
        "$(curl",
        "$(wget",
        "/etc/passwd",
        "/etc/shadow",
        "~/.ssh",
        ".bash_history",
        "id_rsa",
    ]
});

/// Patterns that should NEVER be auto-approved, even if the user chose "always approve"
/// for the shell tool. These require explicit per-invocation approval because they are
/// destructive or security-sensitive.
static NEVER_AUTO_APPROVE_PATTERNS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "rm -rf",
        "rm -fr",
        "chmod -r 777",
        "chmod 777",
        "chown -r",
        "shutdown",
        "reboot",
        "poweroff",
        "init 0",
        "init 6",
        "iptables",
        "nft ",
        "useradd",
        "userdel",
        "passwd",
        "visudo",
        "crontab",
        "systemctl disable",
        "launchctl unload",
        "kill -9",
        "killall",
        "pkill",
        "docker rm",
        "docker rmi",
        "docker system prune",
        "git push --force",
        "git push -f",
        "git reset --hard",
        "git clean -f",
        "DROP TABLE",
        "DROP DATABASE",
        "TRUNCATE",
        "DELETE FROM",
    ]
});

/// Environment variables safe to forward to child processes.
///
/// When executing commands directly (no sandbox), we scrub the environment to
/// prevent API keys and secrets from leaking through `env`, `printenv`, or child
/// process inheritance (CWE-200). Only these well-known OS/toolchain variables
/// are forwarded.
pub(super) const SAFE_ENV_VARS: &[&str] = &[
    // Core OS
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "COLORTERM",
    // Locale
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    // Working directory (many tools depend on this)
    "PWD",
    // Temp directories
    "TMPDIR",
    "TMP",
    "TEMP",
    // XDG (Linux desktop/config paths)
    "XDG_RUNTIME_DIR",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    // Rust toolchain
    "CARGO_HOME",
    "RUSTUP_HOME",
    // Node.js
    "NODE_PATH",
    "NPM_CONFIG_PREFIX",
    // Editor (for git commit, etc.)
    "EDITOR",
    "VISUAL",
    // Windows (no-ops on Unix, but needed if we ever run on Windows)
    "SystemRoot",
    "SYSTEMROOT",
    "ComSpec",
    "PATHEXT",
    "APPDATA",
    "LOCALAPPDATA",
    "USERPROFILE",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "WINDIR",
];

/// Whether a lower-cased command contains an always-blocked pattern.
pub(super) fn matches_blocked_command(lower: &str) -> bool {
    BLOCKED_COMMANDS
        .iter()
        .any(|blocked| lower.contains(blocked))
}

/// Whether a lower-cased command contains a potentially dangerous pattern.
pub(super) fn matches_dangerous_pattern(lower: &str) -> bool {
    DANGEROUS_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

/// Check whether a shell command contains patterns that must never be auto-approved.
///
/// Even when the user has chosen "always approve" for the shell tool, these commands
/// require explicit per-invocation approval because they are destructive.
pub fn requires_explicit_approval(command: &str) -> bool {
    let lower = command.to_lowercase();
    NEVER_AUTO_APPROVE_PATTERNS
        .iter()
        .any(|p| lower.contains(&p.to_lowercase()))
}

/// Detect command injection and obfuscation attempts.
///
/// Catches patterns that indicate a prompt-injected LLM trying to exfiltrate
/// data or hide malicious intent through encoding. Returns a human-readable
/// reason if a pattern is detected.
///
/// These checks complement the existing BLOCKED_COMMANDS and DANGEROUS_PATTERNS
/// lists by catching obfuscation that simple substring matching would miss.
pub fn detect_command_injection(cmd: &str) -> Option<&'static str> {
    // Null bytes can bypass string matching in downstream tools
    if cmd.bytes().any(|b| b == 0) {
        return Some("null byte in command");
    }

    let lower = cmd.to_lowercase();
    INJECTION_CHECKS
        .iter()
        .find(|(is_match, _)| is_match(&lower))
        .map(|(_, reason)| *reason)
}

/// An injection/obfuscation predicate over the lower-cased command.
type InjectionCheck = (fn(&str) -> bool, &'static str);

/// Injection/obfuscation predicates paired with the reason reported when
/// they match. Each predicate receives the lower-cased command.
const INJECTION_CHECKS: &[InjectionCheck] = &[
    (is_base64_decode_to_shell, "base64 decode piped to shell"),
    (
        is_encoded_escape_to_shell,
        "encoded escape sequences piped to shell",
    ),
    (is_binary_decode_to_shell, "binary decode piped to shell"),
    (
        is_dns_exfiltration,
        "potential DNS exfiltration via command substitution",
    ),
    (is_netcat_piping, "netcat with data piping"),
    (is_curl_file_post, "curl posting file contents"),
    (is_wget_file_post, "wget posting file contents"),
    (
        is_string_reversal_to_shell,
        "string reversal piped to shell",
    ),
];

/// Base64 decode piped to shell execution (obfuscation of arbitrary commands).
fn is_base64_decode_to_shell(lower: &str) -> bool {
    let decodes = lower.contains("base64 -d") || lower.contains("base64 --decode");
    decodes && contains_shell_pipe(lower)
}

/// printf/echo with hex or octal escapes piped to shell.
fn is_encoded_escape_to_shell(lower: &str) -> bool {
    let echo_escape = lower.contains("echo -e") || lower.contains("echo $'");
    let printer = lower.contains("printf") || echo_escape;
    let escapes = lower.contains("\\x") || lower.contains("\\0");
    let encoded_print = printer && escapes;
    encoded_print && contains_shell_pipe(lower)
}

/// xxd/od reverse (hex dump to binary) piped to shell.
///
/// Uses has_command_token for "od" to avoid matching words like "method", "period".
fn is_binary_decode_to_shell(lower: &str) -> bool {
    let decoder = lower.contains("xxd -r") || has_command_token(lower, "od ");
    decoder && contains_shell_pipe(lower)
}

/// DNS exfiltration: dig/nslookup/host with command substitution.
///
/// Uses has_command_token to avoid false positives on words containing
/// "host" (e.g., "ghost", "--host") or "dig" as substrings.
fn is_dns_exfiltration(lower: &str) -> bool {
    let dig_or_nslookup = has_command_token(lower, "dig ") || has_command_token(lower, "nslookup ");
    let dns_lookup = dig_or_nslookup || has_command_token(lower, "host ");
    dns_lookup && has_command_substitution(lower)
}

/// Netcat with data piping (exfiltration channel).
///
/// Uses has_command_token to avoid false positives on words containing
/// "nc" as a substring (e.g., "sync", "once", "fence").
fn is_netcat_piping(lower: &str) -> bool {
    let nc_or_ncat = has_command_token(lower, "nc ") || has_command_token(lower, "ncat ");
    let netcat = nc_or_ncat || has_command_token(lower, "netcat ");
    let piping = lower.contains('|') || lower.contains('<');
    netcat && piping
}

/// curl posting file contents to a remote server.
///
/// Includes both "-d @file" (with space) and "-d@file" (without space)
/// since curl accepts both forms.
fn is_curl_file_post(lower: &str) -> bool {
    let data_at = lower.contains("-d @") || lower.contains("-d@");
    let data_flag = data_at || lower.contains("--data @");
    let upload = lower.contains("--data-binary @") || lower.contains("--upload-file");
    let posts_file = data_flag || upload;
    lower.contains("curl") && posts_file
}

/// wget posting file contents to a remote server.
fn is_wget_file_post(lower: &str) -> bool {
    lower.contains("wget") && lower.contains("--post-file")
}

/// Chained obfuscation: rev used to reconstruct hidden commands piped to shell.
fn is_string_reversal_to_shell(lower: &str) -> bool {
    let reversed = lower.contains("| rev") || lower.contains("|rev");
    reversed && contains_shell_pipe(lower)
}

/// Check if a command string contains a pipe to a shell interpreter.
///
/// Uses word boundary checking so "| shell" or "| shift" don't false-positive
/// against "| sh".
pub(super) fn contains_shell_pipe(lower: &str) -> bool {
    has_pipe_to(lower, "sh")
        || has_pipe_to(lower, "bash")
        || has_pipe_to(lower, "zsh")
        || has_pipe_to(lower, "dash")
        || has_pipe_to(lower, "/bin/sh")
        || has_pipe_to(lower, "/bin/bash")
}

/// Check if the command pipes to a specific interpreter, with word boundary
/// validation so "| shift" doesn't match "| sh".
fn has_pipe_to(lower: &str, shell: &str) -> bool {
    for prefix in ["| ", "|"] {
        let pattern = format!("{prefix}{shell}");
        for (i, _) in lower.match_indices(&pattern) {
            let end = i + pattern.len();
            if end >= lower.len()
                || matches!(
                    lower.as_bytes()[end],
                    b' ' | b'\t' | b'\n' | b';' | b'|' | b'&' | b')'
                )
            {
                return true;
            }
        }
    }
    false
}

/// Check if a command string contains shell command substitution (`$(...)` or backticks).
fn has_command_substitution(s: &str) -> bool {
    s.contains("$(") || s.contains('`')
}

/// Check if `token` appears as a standalone command in `lower` (not as a substring
/// of another word).
///
/// A token is "standalone" if it appears at the start of the string or is preceded
/// by whitespace or a shell separator (`|`, `;`, `&`, `(`).
///
/// This prevents false positives like "sync " matching "nc " or "ghost " matching
/// "host ".
pub(super) fn has_command_token(lower: &str, token: &str) -> bool {
    for (i, _) in lower.match_indices(token) {
        if i == 0 {
            return true;
        }
        let before = lower.as_bytes()[i - 1];
        if matches!(before, b' ' | b'\t' | b'|' | b';' | b'&' | b'\n' | b'(') {
            return true;
        }
    }
    false
}
