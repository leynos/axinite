"""Representative mutations of the real workflows, for the contract's own tests.

Each mutation breaks provisioning, an exclusion or the Whitaker wiring one way
in a deep copy of the parsed workflows (or appends a retired mechanism to a
workflow's text), together with the fragment the resulting violation must
contain, so that a mutation cannot pass by tripping some other judge.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ


def _steps(document: dict[str, typ.Any], job: str) -> list[dict[str, typ.Any]]:
    """Return a job's mutable steps."""
    return document["jobs"][job]["steps"]


def _index(steps: list[dict[str, typ.Any]], name: str) -> int:
    """Return the index of the named step."""
    return next(i for i, step in enumerate(steps) if step.get("name") == name)


def _remove_install(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["test.yml"], "tests")
    del steps[_index(steps, "Install build tools")]


def _install_after_the_suite(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["test.yml"], "github-tool-tests")
    install = steps.pop(_index(steps, "Install build tools"))
    steps.insert(_index(steps, "Run GitHub Tool Tests") + 1, install)


def _guard_install(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["test.yml"], "tests")
    steps[_index(steps, "Install build tools")]["if"] = "always()"


def _soften_install(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["code_style.yml"], "clippy")
    steps[_index(steps, "Install build tools")]["continue-on-error"] = True


def _soften_job(docs: dict[str, typ.Any]) -> None:
    docs["test.yml"]["jobs"]["tests"]["continue-on-error"] = True


def _new_linux_suite_job(docs: dict[str, typ.Any]) -> None:
    docs["test.yml"]["jobs"]["extra-suite"] = {
        "runs-on": "ubuntu-latest",
        "steps": [{"uses": "actions/checkout@v6"}, {"run": "make test-github-tool"}],
    }


def _new_direct_cargo_job(docs: dict[str, typ.Any]) -> None:
    docs["test.yml"]["jobs"]["extra-cargo"] = {
        "runs-on": "ubicloud-standard-4",
        "steps": [
            {"uses": "actions/checkout@v6"},
            {"run": "cargo +nightly nextest run"},
        ],
    }


def _mutation_caller_loses_install(docs: dict[str, typ.Any]) -> None:
    job = docs["mutation-testing.yml"]["jobs"]["mutation"]
    job["with"]["setup-commands"] = job["with"]["setup-commands"].replace(
        "make install-build-tools\n", ""
    )


def _mutation_caller_installs_late(docs: dict[str, typ.Any]) -> None:
    job = docs["mutation-testing.yml"]["jobs"]["mutation"]
    commands = job["with"]["setup-commands"].replace("make install-build-tools\n", "")
    job["with"]["setup-commands"] = commands.replace(
        "make build-github-tool-wasm\n",
        "make build-github-tool-wasm\nmake install-build-tools\n",
    )


def _coverage_keeps_rustflags(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["coverage.yml"], "coverage")
    del steps[_index(steps, "Generate coverage")]["env"]["RUSTFLAGS"]


def _coverage_inherits_the_frontend(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["coverage.yml"], "coverage")
    steps[_index(steps, "Generate coverage")]["env"]["RUSTFLAGS"] = "-Zthreads=8"


def _coverage_inherits_mold(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["codescene-coverage.yml"], "coverage-check")
    steps[_index(steps, "Generate coverage")]["env"]["RUSTFLAGS"] = (
        "-Clink-arg=-fuse-ld=mold"
    )


def _e2e_coverage_keeps_rustflags(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["coverage.yml"], "e2e-coverage")
    del steps[_index(steps, "Set up coverage instrumentation")]["env"]


def _release_inherits_rustflags(docs: dict[str, typ.Any]) -> None:
    del docs["release.yml"]["jobs"]["build-local-artifacts"]["env"]["RUSTFLAGS"]


def _release_installs_dev_tools(docs: dict[str, typ.Any]) -> None:
    docs["release.yml"]["jobs"]["build-wasm-extensions"]["steps"].insert(
        1, {"run": "make install-build-tools"}
    )


def _release_plz_inherits(docs: dict[str, typ.Any]) -> None:
    docs["release-plz.yml"]["jobs"]["release-plz-pr"]["env"]["RUSTFLAGS"] = (
        "-Zthreads=8"
    )


def _whitaker_pins_the_suite(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["code_style.yml"], "clippy")
    steps[_index(steps, "Install Whitaker")]["with"]["suite-version"] = "1.0.0"


def _whitaker_before_install(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["code_style.yml"], "clippy")
    gate = steps.pop(_index(steps, "Whitaker Dylint suite"))
    steps.insert(_index(steps, "Install Whitaker"), gate)


def _whitaker_gets_flags(docs: dict[str, typ.Any]) -> None:
    steps = _steps(docs["code_style.yml"], "clippy")
    steps[_index(steps, "Whitaker Dylint suite")]["env"] = {"RUSTFLAGS": "-Zthreads=8"}


#: Each mutation, with the fragment its violation must contain, so a mutation
#: cannot pass by tripping some other judge.
WORKFLOW_MUTATIONS: dict[str, tuple[typ.Callable[[dict[str, typ.Any]], None], str]] = {
    "installer-removed": (_remove_install, "test.yml:tests: compiles at step"),
    "installer-after-the-suite": (
        _install_after_the_suite,
        "github-tool-tests: make install-build-tools runs after",
    ),
    "installer-guarded-by-if": (_guard_install, "conditional or soft-failing"),
    "installer-continues-on-error": (_soften_install, "conditional or soft-failing"),
    "job-continues-on-error": (_soften_job, "the job continues on error"),
    "new-linux-make-suite-job": (_new_linux_suite_job, "extra-suite: compiles at step"),
    "new-linux-direct-cargo-job": (
        _new_direct_cargo_job,
        "extra-cargo: compiles at step",
    ),
    "mutation-caller-loses-install": (
        _mutation_caller_loses_install,
        "mutation-testing.yml:mutation: compiles",
    ),
    "mutation-caller-installs-late": (
        _mutation_caller_installs_late,
        "runs after the first compile",
    ),
    "coverage-keeps-rustflags": (
        _coverage_keeps_rustflags,
        "leaves the configuration's rustflags in force",
    ),
    "coverage-inherits-the-frontend": (
        _coverage_inherits_the_frontend,
        "into coverage",
    ),
    "coverage-inherits-mold": (_coverage_inherits_mold, "into coverage"),
    "e2e-coverage-keeps-rustflags": (
        _e2e_coverage_keeps_rustflags,
        "leaves the configuration's rustflags in force",
    ),
    "release-inherits-rustflags": (_release_inherits_rustflags, "RUSTFLAGS is unset"),
    "release-installs-dev-tools": (
        _release_installs_dev_tools,
        "installs the development build tools",
    ),
    "release-plz-inherits-the-frontend": (_release_plz_inherits, "into the release"),
    "whitaker-pins-the-suite": (_whitaker_pins_the_suite, "pins the rolling suite"),
    "whitaker-gate-before-install": (
        _whitaker_before_install,
        "runs before install-whitaker",
    ),
    "whitaker-gets-flags": (_whitaker_gets_flags, "injects build flags into Whitaker"),
}

#: Retired mechanisms, appended to a workflow's text rather than its structure.
TEXT_MUTATIONS = {
    "setup-mold-returns": "      - uses: rui314/setup-mold@v1\n",
    "stable-toolchain-action-returns": "      - uses: dtolnay/rust-toolchain@stable\n",
    "linker-env-returns": "      CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS: -C link-arg=-fuse-ld=mold\n",
    "distribution-mold-returns": "        run: sudo apt-get install -y clang mold\n",
}
