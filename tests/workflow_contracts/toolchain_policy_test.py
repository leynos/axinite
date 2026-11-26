"""Contracts for the pinned toolchain and the stable MSRV promise it sits beside.

The repository builds on a dated nightly because the development standard's
parallel frontend is nightly-only; Cranelift remains excluded by recorded
evidence. The repository also publishes a stable ``rust-version`` promise.
Both have to stay true, so they are held by
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

from _build_standard import DATED_NIGHTLY, REPOSITORY_ROOT, read_toml
from _build_standard_workflows import iter_jobs, steps_of
from _estate import read_workflows
from _sources import read_text
from _workflow_files import workflow_paths
from _workflow_policy import WORKFLOW_DIR
from contract_sources import is_regular_file, matching_entries
from dependabot_config import read_dependabot

NIGHTLY_IN_TEXT = re.compile(r"nightly-\d{4}-\d{2}-\d{2}")
EXACT_RELEASE = re.compile(r"^\d+\.\d+(\.\d+)?$")
SCHEDULES = {"daily", "weekly"}


def declared_msrvs(
    root_manifest: dict[str, typ.Any], members: dict[str, dict[str, typ.Any]]
) -> dict[str, str]:
    """Return each manifest's declared ``rust-version``, by package.

    Every crate under ``crates/`` declares a promise, whether or not it is a
    root-workspace member; the root package's promise is included too. Which
    promise a crate owes is decided by `workspace_msrvs` and `standalone_msrvs`
    below, from the root manifest's member list.
    """
    declared: dict[str, str] = {}
    package = root_manifest.get("package", {})
    if "rust-version" in package:
        declared[str(package["name"])] = str(package["rust-version"])
    for name, manifest in members.items():
        version = manifest.get("package", {}).get("rust-version")
        if version is not None:
            declared[name] = str(version)
    return declared


def _workspace_package_names(
    root_manifest: dict[str, typ.Any], members: cabc.Mapping[str, dict[str, typ.Any]]
) -> set[str]:
    """Return the package names the root workspace builds.

    The root package is one of them: ``members`` starts with ``"."``. Each
    other entry names a crate directory, whose manifest supplies its package
    name.
    """
    workspace = root_manifest.get("workspace", {})
    names: set[str] = set()
    root_package = root_manifest.get("package", {}).get("name")
    for entry in workspace.get("members", []):
        normalized = str(entry).removeprefix("./")
        if normalized.strip(".") == "":
            if root_package is not None:
                names.add(str(root_package))
            continue
        crate = normalized.rsplit("/", 1)[-1]
        manifest = members.get(crate, {}).get("package", {})
        if "name" in manifest:
            names.add(str(manifest["name"]))
    return names


def workspace_msrvs(
    root_manifest: dict[str, typ.Any],
    members: cabc.Mapping[str, dict[str, typ.Any]],
    msrvs: cabc.Mapping[str, str],
) -> dict[str, str]:
    """Return the promises the root workspace shares one MSRV across.

    Only root-workspace members are in this set: one ``cargo`` invocation
    compiles them, so a second version among them would be untestable.
    """
    names = _workspace_package_names(root_manifest, members)
    return {
        package: version
        for package, version in msrvs.items()
        if package in names
    }


def standalone_msrvs(
    root_manifest: dict[str, typ.Any],
    members: cabc.Mapping[str, dict[str, typ.Any]],
    msrvs: cabc.Mapping[str, str],
) -> dict[str, str]:
    """Return the promises made by first-party crates outside the workspace.

    An excluded crate is its own workspace. Monty 1.6.0 requires Rust 1.98
    while the workspace MSRV lane checks 1.95, so ``crates/monty-helper``
    cannot join the workspace and cannot pretend to 1.95 either. Each such
    crate keeps its own promise, which its own lane must build.
    """
    names = _workspace_package_names(root_manifest, members)
    return {
        package: version
        for package, version in msrvs.items()
        if package not in names
    }


def setup_rust_toolchains(
    documents: cabc.Mapping[str, typ.Any],
) -> list[tuple[str, str]]:
    """Return ``(job, toolchain)`` for each setup-rust step naming a toolchain."""
    steps = (
        (f"{workflow}:{job_id}", step)
        for workflow, job_id, job in iter_jobs(documents)
        for step in steps_of(job)
    )
    return [
        (job, str(step.inputs["toolchain"]))
        for job, step in steps
        if "setup-rust@" in step.uses and "toolchain" in step.inputs
    ]


def _pin_violations(channel: str) -> list[str]:
    """RT-022: the channel is an exact release or a dated nightly."""
    if DATED_NIGHTLY.match(channel) or EXACT_RELEASE.match(channel):
        return []
    return [f"RT-022: the channel {channel!r} is not an exact pin"]


def _dependabot_violations(dependabot: cabc.Mapping[str, typ.Any]) -> list[str]:
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
    """RT-026: the workspace shares one MSRV, and every promise is built.

    ``msrvs`` holds only root-workspace members, which must agree on one
    version and be covered by the MSRV lane. An out-of-workspace promise is
    judged by `_standalone_msrv_violations`, because no setup-rust input names
    it: its own lane builds it on the pinned nightly.
    """
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


def _standalone_msrv_violations(
    standalone: cabc.Mapping[str, str],
    workspace: cabc.Mapping[str, str],
    texts: cabc.Mapping[str, str],
) -> list[str]:
    """An excluded first-party crate keeps a promise its own lane must build.

    It may exceed the workspace MSRV, but it must declare one, and some
    workflow must build it: an out-of-workspace version no lane compiles is a
    claim nothing checks.
    """
    problems: list[str] = []
    workspace_versions = set(workspace.values())
    for package, version in standalone.items():
        if version in workspace_versions:
            continue
        if not any(package in text for text in texts.values()):
            problems.append(
                f"RT-026: {package} declares rust-version {version}, which the "
                "workspace MSRV lane does not provide, and no workflow builds it"
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


def policy_violations(estate: cabc.Mapping[str, typ.Any]) -> list[str]:
    """Judge the pin, its Dependabot entry and the MSRV evidence together.

    Parameters
    ----------
    estate
        The files the policy judges, as the `estate` fixture reads them: the
        ``pin``, the ``dependabot`` configuration, the declared ``msrvs``, the
        parsed workflow ``documents`` and their ``texts``, and the
        ``dockerignore``.
    """
    channel = str(estate["pin"].get("toolchain", {}).get("channel", ""))
    return [
        *_pin_violations(channel),
        *_dependabot_violations(estate["dependabot"]),
        *_msrv_violations(estate["workspace_msrvs"], estate["documents"]),
        *_standalone_msrv_violations(
            estate["standalone_msrvs"], estate["workspace_msrvs"], estate["texts"]
        ),
        *_nightly_violations(channel, estate["texts"]),
        *_dockerignore_violations(estate["dockerignore"]),
    ]


@pytest.fixture(scope="module")
def estate() -> dict[str, typ.Any]:
    """Read everything the policy judges, once."""
    root = read_toml("Cargo.toml")
    members = {
        path.parent.name: read_toml(str(path.relative_to(REPOSITORY_ROOT)))
        for path in matching_entries(REPOSITORY_ROOT / "crates", "*/Cargo.toml")
        if is_regular_file(path)
    }
    msrvs = declared_msrvs(root, members)
    return {
        "pin": read_toml("rust-toolchain.toml"),
        "dependabot": read_dependabot(REPOSITORY_ROOT / ".github/dependabot.yml"),
        "msrvs": msrvs,
        "workspace_msrvs": workspace_msrvs(root, members, msrvs),
        "standalone_msrvs": standalone_msrvs(root, members, msrvs),
        "documents": {
            name: dict(document)
            for name, document in read_workflows(WORKFLOW_DIR).items()
        },
        "texts": {
            path.name: read_text(path) for path in workflow_paths(WORKFLOW_DIR)
        },
        "dockerignore": read_text(REPOSITORY_ROOT / ".dockerignore"),
    }


def _judge(estate: dict[str, typ.Any]) -> list[str]:
    """Return every policy violation in one estate snapshot."""
    return policy_violations(estate)


def test_the_repository_conforms(estate: dict[str, typ.Any]) -> None:
    """The pin, its Dependabot entry and the MSRV evidence agree."""
    problems = _judge(estate)
    assert problems == [], f"toolchain policy violations: {problems}"


def test_the_msrv_was_derived(estate: dict[str, typ.Any]) -> None:
    """The declared versions were read from manifests, and members agree."""
    declared = estate["workspace_msrvs"]
    assert len(declared) >= 3, (
        f"expected the root and at least two crates: {declared}"
    )
    assert len(set(declared.values())) == 1, (
        f"workspace members must share one MSRV: {declared}"
    )
    assert estate["standalone_msrvs"], (
        "the helper is out of the workspace, so it must keep its own promise"
    )


def test_the_msrv_job_checks_and_never_tests(estate: dict[str, typ.Any]) -> None:
    """The estate's cost rule: `cargo check --all-targets` on the MSRV, no suite."""
    job = estate["documents"]["test.yml"]["jobs"]["msrv"]
    runs = "\n".join(step.run for step in steps_of(job))
    assert "cargo check" in runs, f"MSRV job has no cargo check command: {runs}"
    assert "--all-targets" in runs, f"MSRV job skips targets: {runs}"
    runs_tests = re.search(
        r"\bcargo\s+(test|nextest|llvm-cov)\b|\bmake\s+test", runs
    )
    assert runs_tests is None, f"MSRV job must check without running suites: {runs}"
    steps = steps_of(job)
    removal = next(
        (i for i, step in enumerate(steps) if "rm .cargo/config.toml" in step.run),
        None,
    )
    check = next(
        (i for i, step in enumerate(steps) if "cargo check" in step.run), None
    )
    assert removal is not None, "MSRV job must remove the nightly Cargo config"
    assert check is not None, "MSRV job must run its stable cargo check"
    assert removal < check, (
        f"nightly Cargo config must be removed before cargo check: {steps}"
    )


def test_the_msrv_toolchain_is_the_declared_version(estate: dict[str, typ.Any]) -> None:
    """The one setup-rust toolchain input is the root package's rust-version."""
    named = setup_rust_toolchains(estate["documents"])
    assert [job for job, _ in named] == ["test.yml:msrv"], (
        f"only the MSRV job may select stable explicitly: {named}"
    )
    tested = {toolchain for _, toolchain in named}
    declared = set(estate["workspace_msrvs"].values())
    assert tested == declared, (
        f"toolchains {tested} do not cover declared MSRVs {declared}"
    )


def _mutate(
    estate: dict[str, typ.Any], change: cabc.Callable[[dict[str, typ.Any]], None]
) -> list[str]:
    """Judge a changed deep copy without altering the shared fixture."""
    mutated = copy.deepcopy(estate)
    change(mutated)
    return _judge(mutated)


def _forget_the_dockerignore(estate: dict[str, typ.Any]) -> None:
    """Drop the nightly config exclusions from a mutated image context."""
    estate["dockerignore"] = "target/\n"


MUTATIONS: dict[str, tuple[cabc.Callable[[dict[str, typ.Any]], None], str]] = {
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
        lambda e: e["workspace_msrvs"].update({"ambient-fs": "1.93"}),
        "different MSRVs",
    ),
    "msrv-untested": (
        lambda e: e["workspace_msrvs"].update(
            dict.fromkeys(e["workspace_msrvs"], "1.96")
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
    "standalone-promise-without-a-lane": (
        lambda e: e["texts"].update(
            {name: "" for name in e["texts"]}
        ),
        "no workflow builds it",
    ),
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
    assert len(MUTATIONS) == 10
