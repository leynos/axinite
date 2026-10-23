"""Contracts for the pinned toolchain and the stable MSRV promise it sits beside.

The repository builds on a dated nightly, because the development standard's
parallel frontend and Cranelift backend are nightly-only, and it publishes a
stable promise, ``rust-version``. Both have to stay true, so they are held by
separate checks that agree on one derivation: the declared versions come from
the manifests, not from constants in this file.

These are the consumer-side halves of the estate's `rust-toolchain-policy`
clauses (an exact pin, a Dependabot entry, every declared MSRV exercised by CI,
no second dated nightly). The pin's age is not checked here: it moves by a
Dependabot pull request, and a test that reads the calendar would turn every
unrelated branch red the day the window closed.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import collections.abc as cabc
import copy
import re
import typing as typ

import pytest
import yaml

from _build_standard import DATED_NIGHTLY, REPOSITORY_ROOT, read_toml
from _build_standard_workflows import steps_of
from _estate import read_workflows
from _workflow_policy import WORKFLOW_DIR

NIGHTLY_IN_TEXT = re.compile(r"nightly-\d{4}-\d{2}-\d{2}")
EXACT_RELEASE = re.compile(r"^\d+\.\d+(\.\d+)?$")
SCHEDULES = {"daily", "weekly"}


def declared_msrvs(
    root_manifest: dict[str, typ.Any], members: dict[str, dict[str, typ.Any]]
) -> dict[str, str]:
    """Return each manifest's declared ``rust-version``, by package."""
    declared: dict[str, str] = {}
    package = root_manifest.get("package", {})
    if "rust-version" in package:
        declared[str(package["name"])] = str(package["rust-version"])
    for name, manifest in members.items():
        version = manifest.get("package", {}).get("rust-version")
        if version is not None:
            declared[name] = str(version)
    return declared


def setup_rust_toolchains(
    documents: cabc.Mapping[str, typ.Any],
) -> list[tuple[str, str]]:
    """Return ``(job, toolchain)`` for each setup-rust step naming a toolchain."""
    named: list[tuple[str, str]] = []
    for workflow, document in documents.items():
        for job_id, job in (document.get("jobs") or {}).items():
            for step in steps_of(job):
                if "setup-rust@" in step.uses and "toolchain" in step.inputs:
                    named.append(
                        (f"{workflow}:{job_id}", str(step.inputs["toolchain"]))
                    )
    return named


def _pin_violations(channel: str) -> list[str]:
    """RT-022: the channel is an exact release or a dated nightly."""
    if DATED_NIGHTLY.match(channel) or EXACT_RELEASE.match(channel):
        return []
    return [f"RT-022: the channel {channel!r} is not an exact pin"]


def _dependabot_violations(dependabot: dict[str, typ.Any]) -> list[str]:
    """RT-024: a rust-toolchain entry for `/` on a daily or weekly schedule."""
    entries = [
        entry
        for entry in dependabot.get("updates", [])
        if entry.get("package-ecosystem") == "rust-toolchain"
    ]
    if any(
        entry.get("directory") == "/"
        and entry.get("schedule", {}).get("interval") in SCHEDULES
        for entry in entries
    ):
        return []
    return [
        "RT-024: no rust-toolchain Dependabot entry for / on a daily or weekly schedule"
    ]


def _msrv_violations(
    msrvs: dict[str, str], documents: cabc.Mapping[str, typ.Any]
) -> list[str]:
    """RT-026: every declared MSRV is tested, and no toolchain input is stray."""
    problems: list[str] = []
    if len(set(msrvs.values())) > 1:
        problems.append(f"workspace members declare different MSRVs: {msrvs}")
    named = setup_rust_toolchains(documents)
    tested = {toolchain for _, toolchain in named}
    problems.extend(
        f"RT-026: {package} declares rust-version {version} but no job tests it "
        f"({sorted(tested)})"
        for package, version in msrvs.items()
        if version not in tested
    )
    problems.extend(
        f"{job}: setup-rust names {toolchain!r}, which is no declared MSRV, so "
        "the pin is ignored"
        for job, toolchain in named
        if toolchain not in set(msrvs.values())
    )
    return problems


def _nightly_violations(channel: str, texts: cabc.Mapping[str, str]) -> list[str]:
    """RT-027: no workflow names a dated nightly other than the pin."""
    return [
        f"RT-027: {name} names {found}, not the pin {channel}"
        for name, text in texts.items()
        for found in sorted(set(NIGHTLY_IN_TEXT.findall(text)))
        if found != channel
    ]


def _dockerignore_violations(dockerignore: str) -> list[str]:
    """The image build context must not receive the nightly standard."""
    lines = dockerignore.splitlines()
    return [
        f".dockerignore does not exclude {excluded}, so an image build would "
        "take the nightly standard"
        for excluded in (".cargo/", "rust-toolchain.toml")
        if excluded not in lines
    ]


def policy_violations(
    pin: dict[str, typ.Any],
    dependabot: dict[str, typ.Any],
    msrvs: dict[str, str],
    documents: cabc.Mapping[str, typ.Any],
    texts: cabc.Mapping[str, str],
    dockerignore: str,
) -> list[str]:
    """Judge the pin, its Dependabot entry and the MSRV evidence together."""
    channel = str(pin.get("toolchain", {}).get("channel", ""))
    return [
        *_pin_violations(channel),
        *_dependabot_violations(dependabot),
        *_msrv_violations(msrvs, documents),
        *_nightly_violations(channel, texts),
        *_dockerignore_violations(dockerignore),
    ]


@pytest.fixture(scope="module")
def estate() -> dict[str, typ.Any]:
    """Read everything the policy judges, once."""
    root = read_toml("Cargo.toml")
    members = {
        path.parent.name: read_toml(str(path.relative_to(REPOSITORY_ROOT)))
        for path in sorted((REPOSITORY_ROOT / "crates").glob("*/Cargo.toml"))
    }
    return {
        "pin": read_toml("rust-toolchain.toml"),
        "dependabot": yaml.safe_load(
            (REPOSITORY_ROOT / ".github/dependabot.yml").read_text(encoding="utf-8")
        ),
        "msrvs": declared_msrvs(root, members),
        "documents": {
            name: dict(document)
            for name, document in read_workflows(WORKFLOW_DIR).items()
        },
        "texts": {
            path.name: path.read_text(encoding="utf-8")
            for path in sorted(WORKFLOW_DIR.glob("*.y*ml"))
        },
        "dockerignore": (REPOSITORY_ROOT / ".dockerignore").read_text(encoding="utf-8"),
    }


def _judge(estate: dict[str, typ.Any]) -> list[str]:
    return policy_violations(
        estate["pin"],
        estate["dependabot"],
        estate["msrvs"],
        estate["documents"],
        estate["texts"],
        estate["dockerignore"],
    )


def test_the_repository_conforms(estate: dict[str, typ.Any]) -> None:
    """The pin, its Dependabot entry and the MSRV evidence agree."""
    assert _judge(estate) == []


def test_the_msrv_was_derived(estate: dict[str, typ.Any]) -> None:
    """The declared versions were read from manifests, and every member is covered."""
    assert len(estate["msrvs"]) >= 3, (
        f"expected the root and both members: {estate['msrvs']}"
    )
    assert len(set(estate["msrvs"].values())) == 1


def test_the_msrv_job_checks_and_never_tests(estate: dict[str, typ.Any]) -> None:
    """The estate's cost rule: `cargo check --all-targets` on the MSRV, no suite."""
    job = estate["documents"]["test.yml"]["jobs"]["msrv"]
    runs = "\n".join(step.run for step in steps_of(job))
    assert "cargo check" in runs and "--all-targets" in runs
    assert not re.search(r"\bcargo\s+(test|nextest|llvm-cov)\b|\bmake\s+test", runs)
    removal = next(
        i for i, s in enumerate(steps_of(job)) if "rm .cargo/config.toml" in s.run
    )
    check = next(i for i, s in enumerate(steps_of(job)) if "cargo check" in s.run)
    assert removal < check, (
        "the nightly-only configuration must go before the stable compile"
    )


def test_the_msrv_toolchain_is_the_declared_version(estate: dict[str, typ.Any]) -> None:
    """The one setup-rust toolchain input is the root package's rust-version."""
    named = setup_rust_toolchains(estate["documents"])
    assert [job for job, _ in named] == ["test.yml:msrv"], named
    assert {toolchain for _, toolchain in named} == set(estate["msrvs"].values())


def _mutate(
    estate: dict[str, typ.Any], change: typ.Callable[[dict[str, typ.Any]], None]
) -> list[str]:
    mutated = copy.deepcopy(estate)
    change(mutated)
    return _judge(mutated)


def _forget_the_dockerignore(estate: dict[str, typ.Any]) -> None:
    estate["dockerignore"] = "target/\n"


MUTATIONS: dict[str, tuple[typ.Callable[[dict[str, typ.Any]], None], str]] = {
    "channel-floats": (
        lambda e: e["pin"]["toolchain"].update(channel="nightly"),
        "RT-022",
    ),
    "channel-is-stable": (
        lambda e: e["pin"]["toolchain"].update(channel="stable"),
        "RT-022",
    ),
    "dependabot-entry-removed": (
        lambda e: e["dependabot"].update(
            updates=[
                u
                for u in e["dependabot"]["updates"]
                if u["package-ecosystem"] != "rust-toolchain"
            ]
        ),
        "RT-024",
    ),
    "dependabot-directory-moved": (
        lambda e: [
            u.update(directory="/x")
            for u in e["dependabot"]["updates"]
            if u["package-ecosystem"] == "rust-toolchain"
        ],
        "RT-024",
    ),
    "member-msrv-drifts": (
        lambda e: e["msrvs"].update({"ambient-fs": "1.93"}),
        "different MSRVs",
    ),
    "msrv-untested": (
        lambda e: e["msrvs"].update(
            {"axinite": "1.96", "ambient-fs": "1.96", "hyper-rustls": "1.96"}
        ),
        "RT-026",
    ),
    "second-nightly": (
        lambda e: e["texts"].update({"test.yml": "toolchain: nightly-2020-01-01\n"}),
        "RT-027",
    ),
    "setup-rust-ignores-the-pin": (
        lambda e: e["documents"]["test.yml"]["jobs"]["tests"]["steps"][
            next(
                i
                for i, s in enumerate(
                    e["documents"]["test.yml"]["jobs"]["tests"]["steps"]
                )
                if s.get("name") == "Setup Rust"
            )
        ]["with"].update(toolchain="stable"),
        "pin is ignored",
    ),
    "docker-context-takes-the-config": (_forget_the_dockerignore, ".dockerignore"),
}


@pytest.mark.parametrize("name", sorted(MUTATIONS), ids=sorted(MUTATIONS))
def test_each_mutation_is_detected(name: str, estate: dict[str, typ.Any]) -> None:
    """Weakening the policy one way at a time must be reported."""
    change, fragment = MUTATIONS[name]
    problems = _mutate(estate, change)
    assert any(fragment in problem for problem in problems), (
        f"{name}: {fragment!r} not in {problems}"
    )


def test_the_mutations_executed() -> None:
    """Prove the parametrized cases exist, so none can be discovered away."""
    assert len(MUTATIONS) == 9
