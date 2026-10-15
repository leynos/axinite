"""Prevent accepted schema-helper pass fixtures from leaving the UI test."""

from pathlib import Path

import pytest
from _workflow_policy import REPOSITORY_ROOT
from schema_helper_fixtures import read_fixture_inventory

EXPECTED_FIXTURE_MODULES = {
    "param_name_call": "param_name_call.rs",
    "str_call": "str_call.rs",
}


def test_schema_helper_ui_registers_and_executes_every_pass_fixture() -> None:
    """Both existing public call-site fixtures remain ordinary UI test code."""
    inventory = read_fixture_inventory(REPOSITORY_ROOT / "tests/schema_helpers_ui")

    assert inventory.discovered == frozenset(EXPECTED_FIXTURE_MODULES.values())
    assert dict(inventory.registrations) == EXPECTED_FIXTURE_MODULES
    assert frozenset(inventory.invocations) == frozenset(EXPECTED_FIXTURE_MODULES)
    assert not inventory.unregistered
    assert not inventory.missing_files
    assert not inventory.not_executed
    assert not inventory.duplicate_registrations
    assert not inventory.duplicate_invocations
    assert not inventory.unknown_invocations
    assert not inventory.invalid_modules
    assert not inventory.outside_includes
    assert inventory.has_ui_test
    assert not inventory.uses_trybuild


def _write_harness(directory: Path, ui_calls: tuple[str, ...]) -> None:
    """Write the minimal accepted harness shape used by mutation cases."""
    pass_directory = directory / "pass"
    pass_directory.mkdir(parents=True)
    for fixture in EXPECTED_FIXTURE_MODULES.values():
        (pass_directory / fixture).write_text("fn main() {}\n", encoding="utf-8")
    modules = "\n".join(
        f'mod {module} {{\n'
        f'    include!("pass/{fixture}");\n'
        "    pub(super) fn run() {\n"
        "        main();\n"
        "    }\n"
        "}"
        for module, fixture in EXPECTED_FIXTURE_MODULES.items()
    )
    calls = "\n".join(f"    {module}::run();" for module in ui_calls)
    (directory / "main.rs").write_text(
        f"{modules}\n\n#[test]\nfn ui() {{\n{calls}\n}}\n",
        encoding="utf-8",
    )


def _insert_ui_statement(directory: Path, statement: str) -> None:
    """Insert one source statement or comment into the generated ``ui`` body."""
    harness = directory / "main.rs"
    source = harness.read_text(encoding="utf-8")
    opening = "fn ui() {\n"
    assert source.count(opening) == 1, "fixture harness must have one ui test"
    harness.write_text(
        source.replace(opening, f"{opening}    {statement}\n", 1),
        encoding="utf-8",
    )


def test_added_pass_fixture_is_reported_when_not_registered(tmp_path: Path) -> None:
    """A new direct child of ``pass`` cannot silently leave test coverage."""
    _write_harness(tmp_path, tuple(EXPECTED_FIXTURE_MODULES))
    (tmp_path / "pass" / "added_call.rs").write_text(
        "fn main() {}\n", encoding="utf-8"
    )

    inventory = read_fixture_inventory(tmp_path)

    assert inventory.unregistered == frozenset({"added_call.rs"})


def test_removed_ui_call_is_reported_as_an_unexecuted_fixture(tmp_path: Path) -> None:
    """Removing one wrapper call cannot make its fixture pass unnoticed."""
    _write_harness(tmp_path, ("param_name_call",))

    inventory = read_fixture_inventory(tmp_path)

    assert inventory.not_executed == frozenset({"str_call.rs"})


@pytest.mark.parametrize(
    "comment",
    [
        "// str_call::run();",
        "/* str_call::run(); */",
        "/* outer /* str_call::run(); */ still commented */",
    ],
    ids=["line-comment", "block-comment", "nested-block-comment"],
)
def test_commented_ui_call_is_not_an_execution(tmp_path: Path, comment: str) -> None:
    """Line and block comments cannot stand in for a live wrapper call."""
    _write_harness(tmp_path, ("param_name_call",))
    _insert_ui_statement(tmp_path, comment)

    inventory = read_fixture_inventory(tmp_path)

    assert inventory.not_executed == frozenset({"str_call.rs"})


def test_quoted_ui_call_is_not_an_execution(tmp_path: Path) -> None:
    """A call-like string literal cannot satisfy fixture execution coverage."""
    _write_harness(tmp_path, ("param_name_call",))
    _insert_ui_statement(tmp_path, 'let _quoted = "str_call::run();";')

    inventory = read_fixture_inventory(tmp_path)

    assert inventory.not_executed == frozenset({"str_call.rs"})


@pytest.mark.parametrize(
    "statement",
    [
        'let _quoted = "//";',
        'let _quoted = "/*";',
        r'let _quoted = "escaped \" // str_call::run();";',
    ],
    ids=["line-marker", "block-marker", "escaped-quote"],
)
def test_quoted_comment_markers_preserve_live_fixture_calls(
    tmp_path: Path, statement: str
) -> None:
    """Comment markers and fake calls inside strings leave real calls intact."""
    _write_harness(tmp_path, tuple(EXPECTED_FIXTURE_MODULES))
    _insert_ui_statement(tmp_path, statement)

    inventory = read_fixture_inventory(tmp_path)

    assert inventory.invocations == tuple(EXPECTED_FIXTURE_MODULES)
    assert not inventory.not_executed
    assert not inventory.duplicate_invocations


def test_line_comment_at_eof_preserves_fixture_inventory(tmp_path: Path) -> None:
    """A final line comment without a newline cannot damage earlier calls."""
    _write_harness(tmp_path, tuple(EXPECTED_FIXTURE_MODULES))
    harness = tmp_path / "main.rs"
    harness.write_text(
        f'{harness.read_text(encoding="utf-8")}// str_call::run();',
        encoding="utf-8",
    )

    inventory = read_fixture_inventory(tmp_path)

    assert inventory.invocations == tuple(EXPECTED_FIXTURE_MODULES)
    assert not inventory.not_executed
    assert not inventory.duplicate_invocations


def test_commented_wrapper_is_not_a_fixture_registration(tmp_path: Path) -> None:
    """A block-commented wrapper cannot register its included fixture."""
    _write_harness(tmp_path, tuple(EXPECTED_FIXTURE_MODULES))
    harness = tmp_path / "main.rs"
    source = harness.read_text(encoding="utf-8")
    wrapper = "    pub(super) fn run() {\n        main();\n    }"
    assert source.count(wrapper) == len(EXPECTED_FIXTURE_MODULES)
    source = source.replace(wrapper, "    /* pub(super) fn run() { main(); } */", 1)
    harness.write_text(source, encoding="utf-8")

    inventory = read_fixture_inventory(tmp_path)

    assert "param_name_call" in inventory.invalid_modules
    assert "param_name_call.rs" in inventory.unregistered
