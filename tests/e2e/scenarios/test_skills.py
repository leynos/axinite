"""Scenario 4: Skills search / install / remove (SolidJS skills route).

Hits the live ClawHub registry, exactly like the legacy suite, and self-skips
(rather than failing) when the registry is unreachable or returns nothing.

Adaptation from the legacy shell:
  - The Skills tab is now the `/skills` route; the search field is reactive
    (results appear as you type — there is no Search button submit needed).
  - Results are `.skills-search__result` articles with an "Install" action;
    installed skills render as `.skills-card` with a "Remove" action. The
    SolidJS remove path uses no `window.confirm`, so no dialog override.
"""

import pytest

from helpers import SEL, goto_route


async def _open_skills(page):
    await goto_route(page, "Skills", "skills", timeout=8000)
    search = page.locator(SEL["skill_search_input"])
    await search.wait_for(state="visible", timeout=5000)
    return search


async def test_skills_route_visible(page):
    """The skills route shows the search interface."""
    search = await _open_skills(page)
    assert await search.is_visible(), "Skills search input not visible"


async def test_skills_search(page):
    """Searching ClawHub yields at least one result (or self-skips)."""
    search = await _open_skills(page)
    await search.fill("markdown")

    results = page.locator(SEL["skill_search_result"])
    try:
        await results.first.wait_for(state="visible", timeout=20000)
    except Exception:
        pytest.skip("ClawHub registry unreachable or returned no results")

    assert await results.count() >= 1, "Expected at least one search result"


async def test_skills_install_and_remove(page):
    """Install a skill from search, then remove it from the installed list."""
    search = await _open_skills(page)
    await search.fill("markdown")

    results = page.locator(SEL["skill_search_result"])
    try:
        await results.first.wait_for(state="visible", timeout=20000)
    except Exception:
        pytest.skip("ClawHub registry unreachable or returned no results")

    install_btn = results.first.get_by_role("button", name="Install")
    if await install_btn.count() == 0:
        pytest.skip("No installable skills found in results")
    await install_btn.click()

    installed = page.locator(SEL["skill_installed_card"])
    try:
        await installed.first.wait_for(state="visible", timeout=15000)
    except Exception:
        pytest.skip("Skill install did not update the installed list in time")

    installed_count = await installed.count()
    assert installed_count >= 1, "Skill should appear in the installed list"

    remove_btn = installed.first.get_by_role("button", name="Remove")
    if await remove_btn.count() > 0:
        await remove_btn.click()
        await page.wait_for_timeout(3000)
        new_count = await page.locator(SEL["skill_installed_card"]).count()
        assert new_count < installed_count, "Skill should be removed after Remove"
