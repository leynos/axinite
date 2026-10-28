"""A sandbox in which the build-tools scripts see only what a test gives them.

``make check-build-tools`` is a prerequisite of every build, test, lint and
typecheck target, so any contract that *runs* one of those targets (rather than
printing it with ``--dry-run``) reaches the check. The sandbox makes that
check pass or fail on purpose: ``PATH`` holds symlinks to the few utilities the
scripts need plus stub ``mold``, ``clang`` and ``rustup`` executables, and the
pins are fixtures the scripts already honour through their environment
variables.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import platform
import shutil
import subprocess
import typing as typ
from pathlib import Path

CHANNEL = "nightly-2099-01-01"
HOST_ARCH = platform.machine().replace("amd64", "x86_64").replace("arm64", "aarch64")
HOST_TRIPLE = f"{HOST_ARCH}-unknown-linux-gnu"
COMPONENTS = ("rustfmt", "clippy", "llvm-tools-preview")
UTILITIES = (
    "awk",
    "bash",
    "cat",
    "curl",
    "dirname",
    "env",
    "grep",
    "head",
    "mkdir",
    "mktemp",
    "rm",
    "sed",
    "sha256sum",
    "tar",
    "tr",
    "uname",
    "gzip",
    "printf",
)
TOOLCHAIN_FILE = f'''[toolchain]
channel = "{CHANNEL}"
components = [
    "rustfmt",
    "clippy",  # a comment, not a component: "nothing"
    "llvm-tools-preview",
]
targets = ["wasm32-wasip2"]
'''
RUSTUP_STUB = r"""#!/usr/bin/env bash
printf '%s\n' "$*" >>"$SANDBOX_LOG"
case "$1 $2" in
  "toolchain list") printf '%s\n' "$STUB_TOOLCHAINS" ;;
  "show ")
    printf 'Default host: %s\n' "$STUB_HOST_TRIPLE"
    exit "${STUB_RUSTUP_SHOW_STATUS:-0}"
    ;;
  "component list") printf '%s\n' "$STUB_COMPONENTS" ;;
  "target list") printf '%s\n' "$STUB_TARGETS" ;;
  "toolchain install") exit "${STUB_INSTALL_STATUS:-0}" ;;
esac
"""


def fake_mold(version: str) -> str:
    """Return the text of a stub ``mold`` that reports ``version``."""
    return (
        f'#!/usr/bin/env bash\necho "mold {version} (fake; compatible with GNU ld)"\n'
    )


class Sandbox(typ.NamedTuple):
    """Everything a script run needs, rooted in one temporary directory."""

    root: Path
    bin: Path
    log: Path

    def env(self, **extra: str) -> dict[str, str]:
        """Return the environment for a script run, without a host ``PATH``."""
        base = {
            "PATH": str(self.bin),
            "HOME": str(self.root / "home"),
            "SANDBOX_LOG": str(self.log),
            "MOLD_VERSION_FILE": str(self.root / "VERSION"),
            "MOLD_SHA256SUMS_FILE": str(self.root / "SHA256SUMS"),
            "RUST_TOOLCHAIN_FILE": str(self.root / "rust-toolchain.toml"),
            "BUILD_TOOLS_PREFIX": str(self.root / "prefix"),
            "STUB_TOOLCHAINS": f"stable-{HOST_TRIPLE}\n{CHANNEL}-{HOST_TRIPLE}",
            "STUB_HOST_TRIPLE": HOST_TRIPLE,
            "STUB_COMPONENTS": "\n".join(
                f"{c.removesuffix('-preview')}-{HOST_TRIPLE}" for c in COMPONENTS
            ),
            "STUB_TARGETS": f"{HOST_TRIPLE}\nwasm32-wasip2",
        }
        base.update(extra)
        return base

    def stub(self, name: str, body: str) -> None:
        """Install an executable stub into the sandbox ``PATH``."""
        path = self.bin / name
        path.write_text(body, encoding="utf-8")
        path.chmod(0o755)

    def run(self, script: Path, **extra: str) -> subprocess.CompletedProcess[str]:
        """Run a script and return its result without raising."""
        return subprocess.run(  # noqa: S603 - repository script in a sandbox
            [str(script)],
            env=self.env(**extra),
            cwd=self.root,
            capture_output=True,
            text=True,
            check=False,
        )


def make_sandbox(root: Path) -> Sandbox:
    """Build a sandbox in ``root`` where every tool is present and correct."""
    bin_dir = root / "bin"
    bin_dir.mkdir(parents=True)
    for name in UTILITIES:
        found = shutil.which(name)
        if found:
            (bin_dir / name).symlink_to(found)
    box = Sandbox(root, bin_dir, root / "calls.log")
    (root / "VERSION").write_text("2.41.0\n", encoding="utf-8")
    (root / "rust-toolchain.toml").write_text(TOOLCHAIN_FILE, encoding="utf-8")
    box.stub("mold", fake_mold("2.41.0"))
    box.stub("clang", "#!/usr/bin/env bash\necho clang\n")
    box.stub("rustup", RUSTUP_STUB)
    return box
