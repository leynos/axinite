import { expect, test } from "@playwright/test";

const DEPLOY_BASE = "/axinite-mockup";

test("navigates routes and only exposes complete locales", async ({ page }) => {
  await page.goto(`${DEPLOY_BASE}/chat?debug-flags=1`);

  await expect(
    page.getByRole("heading", { name: "Chat", level: 2 })
  ).toBeVisible();
  await page.getByRole("link", { name: "Memory" }).click();
  await expect(page).toHaveURL(/\/axinite-mockup\/memory$/);
  await expect(
    page.getByRole("heading", { name: "Memory", level: 2 })
  ).toBeVisible();

  await page.getByRole("link", { name: "Jobs" }).click();
  await expect(page).toHaveURL(/\/axinite-mockup\/jobs$/);
  await expect(
    page.getByRole("heading", { name: "Jobs", level: 2 })
  ).toBeVisible();

  await page.getByRole("link", { name: "Routines" }).click();
  await expect(page).toHaveURL(/\/axinite-mockup\/routines$/);
  await expect(
    page.getByRole("heading", { name: "Routines", level: 2 })
  ).toBeVisible();

  await page.getByRole("link", { name: "Extensions" }).click();
  await expect(page).toHaveURL(/\/axinite-mockup\/extensions$/);
  await expect(
    page.getByRole("heading", { name: "Extensions", level: 2 })
  ).toBeVisible();

  await page.getByRole("link", { name: "Skills" }).click();
  await expect(page).toHaveURL(/\/axinite-mockup\/skills$/);
  await expect(
    page.getByRole("heading", { name: "Skills", level: 2 })
  ).toBeVisible();

  await page.getByRole("button", { name: "Logs" }).click();
  await expect(page.getByRole("heading", { name: "Logs" })).toBeVisible();

  await page.keyboard.press("Escape");
  const languagePicker = page.getByLabel("Language");
  await expect(languagePicker).toHaveValue("en-GB");
  await expect(languagePicker.locator("option")).toHaveCount(10);

  await page.goto(`${DEPLOY_BASE}/skills?lng=fr`);
  await expect(page.locator("html")).toHaveAttribute("lang", "fr");
  await expect(
    page.getByRole("heading", { name: "Compétences", level: 2 })
  ).toBeVisible();
});
