# An Architectural Blueprint for High-Velocity, Accessibility-First Testing

## I. Strategic Framework: A Multi-Layered Approach to Accessible Testing

Achieving an **accessibility-first** testing strategy at high velocity requires
a test architecture that is both **fast** and **thorough**. Accessibility must
be embedded into everyday development without creating a slow feedback loop. The
proposed framework adopts a multi-layered approach: a rapid inner loop for
component tests and a comprehensive outer loop for end-to-end (E2E) validation.

This architecture reflects recent tooling decisions. The framework leverages the
speed of the **Bun** runtime for most unit and integration tests, while a
parallel **Node.js** harness compensates for Bun's accessibility-scan
limitations. **Playwright** serves as the outer test loop that covers the full
user experience. Each layer has a distinct role, ensuring that accessibility is
verified at every stage without compromising developer productivity.

### 1.1 Deconstructing the Happy-DOM Deadlock: Bun vs. `axe-core`

Initial experiments exposed a fundamental deadlock between Bun's default DOM
simulation and the industry-standard accessibility engine, **`axe-core`**. Bun’s
test runner is engineered for extreme speed and recommends using **Happy DOM**
for simulating the browser environment. **Happy DOM** is a lightweight, fast
implementation of browser APIs – but its quest for speed comes at a cost. It
diverges from web standards in subtle ways that clash with `axe-core`.

The most critical incompatibility lies in Happy DOM’s handling of the
**`Node.isConnected`** property. This DOM API indicates if a node is attached to
the document. `axe-core` relies on modifying `isConnected` during its DOM
traversal. In Happy DOM, `isConnected` is implemented as a read-only property
(and not fully standard), causing `axe-core` to throw runtime errors when it
attempts to set it. In practice, this means any accessibility scan using
`axe-core` fails outright under Happy DOM.[^1] Community reports and library
documentation confirm this issue: the maintainers of `vitest-axe` (a Vitest/Jest
integration for axe) explicitly warn that their matcher is **incompatible** with
Happy DOM environments.

Unfortunately, Bun’s test runner still does not provide **JSDOM** as a native,
drop-in test environment. As of March 2026, Bun’s documentation continues to
recommend **Happy DOM** for DOM testing, while Bun v1.1.41 and later have made
JSDOM more reliable through `node:vm` compatibility fixes.[^2] That improves
the manual setup story, but JSDOM still has to be installed and configured
explicitly rather than enabled as a built-in replacement for Happy DOM, and a
future Bun release may change that status. JSDOM would solve the
`isConnected` issue, but Bun cannot use it as a drop-in replacement at this
time. This presents a catch-22:

- Bun’s recommended path for DOM tests is Happy DOM (for speed).

- `axe-core` cannot operate under Happy DOM due to fundamental API mismatch.

- The only viable alternative (JSDOM) requires manual setup instead of Bun’s
  native DOM-test path.

In summary, **Bun alone cannot run component-level accessibility scans** today.
This impasse is not a matter of configuration or minor bug; it is a fundamental
limitation of the current Bun + Happy DOM pairing. The strategy must therefore
retain Bun’s performance benefits _and_ enable `axe-core` scans through other
means. The solution is a **hybrid testing approach**: run most tests in Bun for
speed, but outsource accessibility-specific tests to a Node.js environment that
supports JSDOM.

### 1.2 A Hybrid Solution: Node.js + JSDOM for A11y Scans

To resolve the deadlock, the framework introduces a **parallel Node.js test
harness** dedicated to accessibility checks. Rather than abandoning Bun
entirely, the `axe-core` scans are isolated into their own test suite running on
Node.js. In practice, this means writing accessibility tests in separate files
such as `*.a11y.test.tsx` and executing them with a Node-based runner that uses
JSDOM. All other unit and integration tests continue to run with `bun test`
using Happy DOM as usual.

Under this hybrid model, **Bun** remains the primary test engine for the vast
majority of tests, and **Node+JSDOM** is invoked as needed for the specific
cases that require `axe-core`. This dual setup provides the “best of both
worlds”:

- **Fast feedback for logic and UI structure:** Bun’s test runner with Happy DOM
  provides near-instant execution for tests that verify component behaviour,
  state management, and basic rendering structure. Immediate feedback stays in
  the inner loop without waiting on a heavy browser simulation.

- **Standards-compliant DOM for accessibility rules:** A Node environment can
  host JSDOM, a more complete DOM implementation that `axe-core` supports. By
  running `axe` in this context, semantic and structural accessibility issues
  that Bun/Happy DOM would miss or error on can be caught.

To keep the Node-based tests efficient, the framework leverages **tsgo**
(TypeScript’s new fast compiler) to compile and launch these tests. A typical
setup could use a lightweight command such as `npx tsgo` to transpile the
`*.a11y.test.tsx` files on the fly and execute them in Node. This avoids the
overhead of a full Jest or Vitest context for the accessibility suite. These
a11y tests function as a specialized batch of scripts: they set up JSDOM, render
components, run `axe-core`, and report results.

Crucially, the separation of test files by naming convention means the two
suites can run independently. A typical workflow might include two NPM scripts:
one for the **fast tests** (`npm run test:unit` using Bun) and one for the
**a11y tests** (`npm run test:a11y` using Node). In continuous integration,
these can run in parallel or in sequence, but they remain logically isolated.
This prevents any slowdown of the primary suite; if accessibility scans are a
bit slower due to JSDOM’s overhead, they do not make the core test run slower.
It also provides flexibility: rapid prototyping can rely on the Bun tests alone,
with axe checks run when needed or in a pre-commit hook.

By introducing a Node+JSDOM harness for `axe-core`, the accessibility-first
strategy is unblocked without sacrificing performance. Bun remains at the center
of testing, while a reliable secondary path performs **automated accessibility
audits** at the component level.

### 1.3 Acknowledging Limits: What JSDOM **Can’t** Catch

Even with JSDOM enabling `axe-core` scans, it’s important to set realistic
expectations. **No simulated DOM environment (Happy DOM or JSDOM) can replace a
real browser for certain accessibility validations.** JSDOM is purely a DOM
parser and engine; it does **not** perform visual rendering, apply CSS layout,
or run the browser’s accessibility tree computations. This means some
accessibility rules are beyond its scope.

The official Axe documentation notes "limited support for JSDOM" and advises
disabling rules that are known to yield false results in a headless DOM.[^3] The
most prominent example is the **colour contrast** rule. Verifying colour contrast
requires computing rendered text colours against background pixels – something
impossible without an actual rendering engine (JSDOM has no concept of pixels or
CSS cascade in effect). Any `axe-core` rule that depends on actual rendering or
CSS will fail or produce irrelevant results under JSDOM. Besides
`color-contrast`, other rules in this category include:

- **Target size** (minimum touch target dimensions)

- **Scrollable region focusable** (requires knowing if overflow renders
  scrollbars)

- **Visual focus indicators** (CSS outline or highlight on focusable elements)

- Etc.

The framework addresses this by **disabling such rules in the component-layer
tests** and deferring their verification to the Playwright E2E layer. In
practice, the Node-based axe test harness loads `axe-core` with a configuration
that turns off rules like `color-contrast` and any others that rely on styles or
layout. For example, `color-contrast` is disabled globally in these tests and
the documentation explicitly records that **contrast must be checked in the
browser**. Similarly, a rule like `scrollable-region-focusable` is unreliable
without actual CSS overflow calculations; JSDOM scans should ignore it and a
keyboard navigation test should cover it in the E2E suite.

Understanding these limits reinforces the need for the **outer test loop**. The
inner loop (Bun + Node/axe tests) catches semantic issues such as missing ARIA
labels, improper roles, and missing alt text, but it **cannot fully guarantee**
things like proper colour contrast, focus order on actual UI, or dynamic content
announcements. **Real browser testing with Playwright** covers those gaps. This
separation of concerns prevents a false sense of security from the fast tests
and makes it clear which checks occur later in the pipeline.

With the strategic foundation laid, each layer of the architecture can now be
examined in turn: the fast feedback inner loop powered by Bun (augmented with
Node-based axe scans), and the comprehensive outer loop powered by Playwright.

## II. The Inner Loop: Fast, Accessible Component Testing with Bun & JSDOM

The “inner loop” is where developers spend most of their time. It encompasses
the rapid unit and component tests run during active development, often on every
file save or commit. The goal for this layer is to **make accessibility a
built-in aspect of these fast tests**. Accessibility checks should happen
automatically and with minimal overhead whenever a component’s logic or
rendering is tested.

To achieve this, the inner loop is structured into two synergistic parts:

- **2.1 Bun Test Runner + Happy DOM for Core Tests** – ultra-fast execution of
  component tests for logic, state, and basic rendering using Bun’s native test
  framework.

- **2.2 Node + JSDOM Axe Tests for Accessibility** – focused tests that
  specifically scan components with `axe-core` to catch semantic issues, running
  in a Node environment in parallel to the Bun tests.

- **2.3 Accessible-First Test Practices** – a set of conventions for writing
  tests (in both parts above) that ensure accessible selectors are used and
  assertions focus on meaningful output, effectively doubling as accessibility
  verification.

Together, these ensure that by the time a component leaves the inner loop, it is
not only functionally correct but also **accessible by design**.

### 2.1 Ultra-Fast Execution with Bun and Happy DOM

Bun’s built-in test runner serves as the default for all standard tests. The
`bun test` command is configured with Happy DOM as the global DOM
(`--preload ./tests/setup-happy-dom.ts` in the NPM script) so that each test
file can create `document` and `window` as if running in a browser. The payoff
is speed: Bun’s runtime and Happy DOM allow most tests to run in
**milliseconds**, keeping the feedback loop extremely tight.

These Bun-driven tests cover items such as:

- **Component Rendering and Props:** Does the component render the right output
  given certain props or state?

- **State Transitions:** Does the component update correctly when a user action
  is simulated or a context value changes?

- **Basic Accessibility attributes:** If a component is supposed to have certain
  ARIA attributes or roles, are they present in the rendered output?

For example, suppose a `<ThemeProvider>` context and the desktop `<AppShell>`
layout are under test. A Bun test might verify that when the theme context is
applied, the DOM gets a `data-theme` attribute on the `<html>` element:

```tsx
import { describe, it, expect } from "bun:test";
import { render } from "@solidjs/testing-library";

// ... import the ThemeProvider and a test component

describe("ThemeProvider integration", () => {
  it("applies the default theme to <html> and <body>", () => {
    const { unmount } = render(() => (
      <ThemeProvider>
        <span id="probe" />
      </ThemeProvider>
    ));

    const probe = document.querySelector("#probe");
    expect(probe).toBeTruthy();
    // Check that the theme data attribute was applied globally
    expect(document.documentElement.getAttribute("data-theme")).toBe(
      "wildside-night",
    );
    expect(document.body.getAttribute("data-theme")).toBe("wildside-night");

    unmount();
  });
});
```

In this snippet, the Bun test quickly renders a Solid component into a fake DOM
and makes assertions. The usage of `document.querySelector` and standard DOM
APIs is possible thanks to Happy DOM. Tests like this run blazingly fast (no
real browser, no JSDOM overhead) and can be run hundreds of times a minute
during development.

**Happy DOM Caveat:** As discussed, Happy DOM isn’t fully standards-compliant.
Its usage should be **limited to tests that do not require full fidelity**. That
typically means avoiding deep CSS or canvas-related testing here. For most Solid
component output and event simulation, Happy DOM is sufficient and incredibly
fast. Whenever a limitation appears, such as an API not implemented or a
discrepancy, it should be documented. In many cases, simple polyfills or slight
test adjustments can work around minor differences.

### 2.2 Automated Accessibility Scans via Node + JSDOM

To complement Bun’s lightning-fast tests, a parallel suite of
**accessibility-focused tests** runs in Node. These tests are responsible for
running `axe-core` against components. They ensure that, from a semantic
standpoint, components meet WCAG guidelines before integration into pages.

**Test Structure and Naming:** These files are suffixed with `.a11y.test.tsx`
(for example, `Button.a11y.test.tsx`, `Modal.a11y.test.tsx`) to clearly
distinguish them. This naming serves two purposes: it signals their purpose, and
it allows targeted execution in test scripts. Package scripts can be configured
so that `npm run test:a11y` only picks up files matching `*.a11y.test.tsx`. In
CI, this makes it trivial to run the accessibility suite separately from, and
possibly in parallel with, the main Bun test suite.

**JSDOM Setup:** Each a11y test file uses **JSDOM** as its DOM environment.
JSDOM can either be initialized manually within the test, or supplied by a Node
test runner that supports it globally. One lightweight approach is a small
harness script that, for each `.a11y.test.tsx` file:

- Spins up a JSDOM instance (e.g. using
  `new JSDOM('<!DOCTYPE html><html><body></body></html>')` and setting global
  `window` and `document`).

- Mounts the Solid component into that JSDOM document (likely using
  `@solidjs/testing-library`’s `render`).

- Runs `axe-core` on the JSDOM’s DOM tree.

- Asserts that there are no violations.

This can be automated using familiar libraries. Solid's own testing guidance
centres on **Vitest**, **jsdom**, `@solidjs/testing-library`,
`@testing-library/user-event`, and `@testing-library/jest-dom`. In this
architecture, the Node accessibility lane mirrors that tool shape while focusing
specifically on `axe-core`. The following illustrates a typical pattern, written
in a Jest/Vitest style similar to what a custom harness would do:

```tsx
// Button.a11y.test.tsx
import { render, screen } from "@solidjs/testing-library";
import { axe, toHaveNoViolations } from "jest-axe";
import Button from "./Button";

// Extend expect for the axe matcher (if using jest-axe)
expect.extend(toHaveNoViolations);

test("Button has no accessibility violations and is properly labelled", async () => {
  const { container } = render(<Button>Click Me</Button>);

  const results = await axe(container);
  expect(results).toHaveNoViolations(); // Custom matcher asserts zero axe issues

  // Also verify accessible name and role are correct as a sanity check:
  const btn = screen.getByRole("button", { name: /click me/i });
  expect(btn).toBeInTheDocument();
});
```

In this example, when run in a Node+JSDOM environment, the `Button` component is
rendered off-screen and scanned. The test will fail if, for instance, the
`<Button>` component is missing an accessible name or has ARIA attributes
misused. By including a Testing Library query
(`getByRole('button', { name: /click me/i })`), the test verifies not only that
the component is _free of axe-detectable violations_ but also that it adheres to
expected accessibility APIs.

This pattern applies to all interactive components. A more complex example is a
**Modal** component which might be hidden or shown based on props:

```tsx
// Modal.a11y.test.tsx
import { render } from "@solidjs/testing-library";
import { axe } from "jest-axe";
import Modal from "./Modal";

describe("Modal component accessibility", () => {
  test("has no violations when closed", async () => {
    const { container } = render(<Modal open={false} title="Sample Modal" />);
    const results = await axe(container);
    expect(results).toHaveNoViolations();
    // Even though the modal is closed (perhaps rendered off-screen or not at all),
    // no stray violations such as missing landmarks should remain.
  });

  test("has no violations when open", async () => {
    const { container } = render(
      <Modal open={true} title="Sample Modal">
        <p>Content</p>
      </Modal>,
    );
    // Temporarily disable a page-level rule that is irrelevant in isolation:
    const results = await axe(container, {
      rules: {
        region: { enabled: false }, // disable landmark rule since modal isn't wrapped in <main> here
      },
    });
    expect(results).toHaveNoViolations();
  });
});
```

The second test demonstrates **contextual rule configuration**: the `region`
rule is not applicable when testing a Modal in isolation, so it is disabled.
This ability to tweak axe’s rules per test helps avoid false positives and keeps
the test signal clean. Every axe violation reported in this suite should
correspond to a genuine issue that a user might encounter.

**Execution and Performance:** Running these Node/JSDOM tests is slower than the
Bun tests, because JSDOM startup and axe analysis take time, but they are still
relatively fast. A simple component scan might take on the order of tens of
milliseconds to a couple hundred milliseconds. Performance concerns are
mitigated by running these tests in parallel and by keeping each test focused on
a single component or state to avoid heavy DOMs. In CI, further parallelization
is possible by splitting the accessibility tests across multiple machines if
needed.

The key outcome of this inner-loop accessibility suite is **immediate feedback
on semantic issues**. If a component is missing an `aria-label` on a button, a
test fails immediately. That is far better than catching the issue days later in
a manual audit or in a slow E2E test. It turns accessibility into a daily
concern rather than a QA afterthought.

### 2.3 Accessible-First Test Practices: Enforcing Good Habits

Having the right tools is only half the battle. The framework also enforces
**strict conventions in how tests are written**, ensuring that the test code
itself encourages accessible implementation. Two key practices are:

- **Use of Accessible Queries:** Tests must interact with the rendered DOM the
  same way a user or assistive technology would, by using accessible labels,
  roles, and text content. Selecting elements by obscure hooks or internal IDs
  is forbidden whenever a semantic alternative exists. In practice, this means
  favoring Testing Library queries like `getByRole`, `getByLabelText`, or
  `getByText` over queries like `querySelector('[data-testid="..."]')`. If tests
  cannot find an element by a meaningful label or role, that is a red flag that
  the component might not be accessible.

_Policy:_ The use of `data-testid` (or similar testing-only attributes) is
considered a last resort. In code review, any test that uses a test ID must
justify why a role or label could not be used instead. Often, the remedy is to
improve the component (e.g. add an `aria-label` or proper text) so that a more
user-centric query becomes possible. This policy creates a virtuous cycle: it
pushes developers to build components with accessibility in mind, and it makes
the tests more robust by tying them to user-visible strings and roles.

_Automation:_ Biome loads the `tools/grit/rule-testing-*.grit` patterns, which
emit the `test/no-testid-selectors`, `test/no-queryselector`,
`test/prefer-byrole-for-actions`, and `test/no-raw-dom-lookup` diagnostics for
test files. These rules error on any `*ByTestId` lookup (or raw
`querySelector*`), warn when `getByText` is clicked, and therefore keep tests
aligned with the role-first guidance from Testing Library and Playwright. If a
semantic selector is genuinely impossible, add an `ACCESSIBILITY:` justification
next to a targeted `// biome-ignore lint/test/no-testid-selectors` suppression
so reviewers can see the trade-off.

_Example:_ In the `HeaderBar` component test, instead of selecting a theme
button by an ID or class, query it by its accessible name:

```tsx
const dayThemeButton = screen.getByRole("button", {
  name: "Day",
});
expect(dayThemeButton).toBeInTheDocument();
```

This example relies on the visible label on the theme button. If that label were
missing or changed, the test would fail, which is desirable because the
accessible contract of the UI would have changed. Either way, the test asserts
something a screen reader user would care about: that there is a button with
that label.

#### 2.3.1 Exemplars awaiting refactor

There are still a handful of high-visibility suites leaning on test IDs and DOM
structure hooks. They are valuable targets when socializing the upcoming lint
rule:

- `tests/routes.stage1.test.tsx` (quick-map and offline manager flows) uses
  selectors such as `[data-testid='quick-walk-stops-panel']` and
  `offline-delete-button`. The fix is to surface accessible names – for example,
  ensure the stops tab owns an `aria-labelledby`, label the delete buttons
  “Delete {map name}”, and then switch the tests to
  `getByRole('tabpanel', { name: /stops/i })` or
  `getAllByRole('button', { name: /delete .* offline map/i })`.
- `tests/header-bar.test.tsx` should continue asserting theme state through the
  real controls, for example by checking the active button’s `aria-pressed`
  state rather than a DOM-only hook.
- `tests/quick-map-fragment.test.tsx` queries map panels via
  `[data-testid='quick-walk-map-container']`. Once the container has a sensible
  label (for example `aria-label="Quick walk map"`), the test can use
  `getByRole('region', { name: /quick walk map/i })` and remain resilient.

Documenting these specific call sites gives reviewers a short list of “fix me
next” examples and shows what the accessible alternative should look like.

- **Assertions on Accessibility Outcomes:** In addition to low-level assertions
  such as “this state toggled” or “this element exists”, tests should add
  assertions specifically for accessibility expectations. For instance, after
  clicking a button that opens a modal, a test might assert that focus moved to
  the modal dialog element (using `expect(dialog).toHaveFocus()` or checking
  `document.activeElement`). Or after toggling a theme, a test can assert that
  the `<html lang>` or `data-theme` attributes are correctly set. Writing these
  into tests ensures that accessibility features such as focus management, ARIA
  attributes, and language attributes are not accidentally broken.

To enforce these practices, the framework integrates ESLint rules and testing
guidelines:

- **ESLint plugins** such as Testing Library’s `prefer-user-event` and
  `prefer-accessible-queries` should be enabled to warn if a test uses
  `getByTestId` when a role query is available, or if it calls low-level DOM
  methods instead of simulating real user events.

- In pull request reviews, the team should flag any test code that does not
  adhere to accessible-first querying. Over time this becomes second nature.

By combining tooling (axe scans, proper libraries) with conventions (only use
accessible queries, assert on ARIA/focus behaviours), the inner loop becomes a
strong quality gate. A component that is functionally perfect but accessibly
flawed cannot pass unnoticed.

## III. The Outer Loop: Comprehensive E2E Validation with Playwright

While the inner loop verifies individual components in isolation, the **outer
loop** ensures that the application works as a cohesive, accessible whole.
**Playwright** runs end-to-end tests in real browsers. The outer loop addresses
everything that a simulated environment cannot: actual rendering, styling, user
interactions across components, and integration of features like routing and
dynamic content. It is the final safety net and a place to conduct more
intensive audits, including visual and internationalization checks.

Playwright was chosen for its robust multi-browser support (Chromium, Firefox,
WebKit), its powerful automation APIs, and built-in test runner that integrates
assertions, parallelization, and rich reporting. This section outlines how
Playwright supports various accessibility-focused E2E tasks:

- **3.1 In-Browser Axe Scans:** Running `axe-core` in a **real browser** context
  to catch issues like colour contrast, focus order, and other things JSDOM can’t
  detect.

- **3.2 Advanced Interaction Tests:** Simulating keyboard navigation, verifying
  focus management (e.g., modals trapping focus, return focus on close), and
  other workflow-oriented checks.

- **3.3 Accessibility Tree Snapshots:** Capturing and diffing the browser’s
  accessibility tree to detect unintended semantic changes over time.

- **3.4 Visual Regression across Viewports/Themes:** Taking screenshots of
  pages/components in various layouts to catch visual bugs that could affect
  usability or clarity.

- **3.5 Localization and Language Verification:** Testing the app under
  different locale settings to ensure language-specific attributes and content
  appear correctly.

This battery of tests runs less frequently (e.g., on pull request or nightly
builds) due to being heavier, but it is exhaustive in ensuring real-world
accessibility compliance.

### 3.1 Strategic Axe Scans in the Browser

`axe-core` is integrated into Playwright tests via the `@axe-core/playwright`
utility. This allows the axe script to be injected into pages and the rendered
output to be analyzed for violations. The **advantage** is that, in a real
browser, axe can evaluate everything, including CSS, canvas, and the actual
computed tree.

However, the suite must be kept fast. Running a full axe scan after every action
would turn E2E tests into a slog. Instead, accessibility scans are treated as
**targeted assertions** at critical checkpoints of a user flow:

- **After initial page load:** The fully loaded page is often scanned to ensure
  no glaring issues exist on the baseline UI.

- **After major UI transitions:** For example, after opening a modal, after
  navigating to a new page, after triggering a form validation that reveals
  errors, etc. These are points where new content appears or state significantly
  changes, warranting a re-check.

- **Scope the scan to changed regions:** Using `AxeBuilder.include(selector)`,
  analysis can be limited to specific parts of the DOM. For instance, after
  opening a modal, scanning _only_ the modal dialog element saves time and
  focuses results.

**Example – Modal Workflow:** Consider a test of an e-commerce flow where
clicking "Add to Cart" opens a dialog. The test can be written as follows:

```ts
import { test, expect } from "@playwright/test";
import { AxeBuilder } from "@axe-core/playwright";

test.describe("Add to Cart Modal Flow", () => {
  test("main page has no axe violations on load", async ({ page }) => {
    await page.goto("/products");
    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  test("modal dialog is accessible after opening", async ({ page }) => {
    await page.goto("/products");
    // Trigger the modal
    await page.getByRole("button", { name: "Add to Cart" }).click();
    // Ensure modal is visible
    const modal = page.locator("#add-to-cart-modal");
    await expect(modal).toBeVisible();
    // Run axe only inside the modal
    const modalResults = await new AxeBuilder({ page })
      .include("#add-to-cart-modal")
      .analyze();
    expect(modalResults.violations).toEqual([]);
  });
});
```

In this snippet, the first test scans the full page and expects zero violations,
meaning the base page structure is sound. The second test specifically clicks
the "Add to Cart" button and then waits for the modal. The scan is limited to
the modal content. If the modal were missing an `aria-label` on its header or
had a form control without a label, `axe-core` would catch it here.

By **scoping axe scans and using them sparingly**, performance impact is kept
under control. Each scan might take a second or two on a large page, so scans
should occur only where they yield new information. The principle is: **treat
accessibility scans as assertions, not as a blanket afterthought**. This keeps
the E2E tests fast enough for CI while still covering critical scenarios.

### 3.2 Interactive Behaviour and Focus Management

Automated accessibility testing must extend beyond static analysis. Many
accessibility issues are only apparent when users actually interact with the UI.
With Playwright’s control of the browser, those interactions can be simulated
and the application’s response can be validated. Two major focus areas are
**keyboard navigation** and **focus handling**.

#### 3.2.1 Keyboard Navigation Flow Tests

A core WCAG principle is that **all functionality should be operable via
keyboard**. E2E tests should mimic a user pressing `Tab`, `Shift+Tab`, `Enter`,
and `Escape` to navigate and activate components.

For example, consider verifying the navigation menu and search input in a
header:

```ts
test("Home page header is fully keyboard-navigable", async ({ page }) => {
  await page.goto("/");

  // Press Tab repeatedly and check focus moves in the expected order
  await page.keyboard.press("Tab");
  let focused = await page.evaluate(() => document.activeElement?.textContent);
  expect(focused).toMatch(/Products/i); // focus on "Products" link

  await page.keyboard.press("Tab");
  focused = await page.evaluate(() =>
    document.activeElement?.getAttribute("aria-label"),
  );
  expect(focused).toBe("Search"); // now search box is focused (identified by its aria-label)

  await page.keyboard.press("Tab");
  focused = await page.evaluate(() => document.activeElement?.textContent);
  expect(focused).toMatch(/My Account/i); // now "My Account" button is focused

  // Reverse navigation
  await page.keyboard.down("Shift");
  await page.keyboard.press("Tab");
  await page.keyboard.up("Shift");
  focused = await page.evaluate(() =>
    document.activeElement?.getAttribute("aria-label"),
  );
  expect(focused).toBe("Search");
});
```

This script tabs through the interactive elements of the header and uses
`document.activeElement` to check which element is currently focused. It
verifies that focus cycles through **Products link → Search input → My Account
button** in order, and that it can go backwards as well. If an element is not
reachable via Tab, the test will catch it.

Similar tests apply to menus, dialogs, and other composite components:

- Ensuring that pressing **Enter** on a focused button triggers the expected
  action (Playwright’s `page.keyboard.press('Enter')` can be used).

- Ensuring **Escape** closes dialogs or dropdowns when focused inside them.

- Verifying there are no **keyboard traps**: focus should never get stuck in a
  loop or lost off-screen. For modals, focus should remain trapped within the
  modal while it is open, but when the modal closes, focus should return to a
  sensible place.

These tests provide confidence that a sighted keyboard user or a visually
impaired user using keyboard plus screen reader can navigate and operate the
application fully.

#### 3.2.2 Focus Handling in Dialogs and Overlays

Building on keyboard tests, **modal dialogs, popovers, and other overlays**
should be checked specifically for proper focus management:

- When a modal opens, the first interactive element inside it should receive
  focus (or the modal container itself, if it has `tabindex="-1"` to catch
  focus).

- Tab should wrap within the modal (since modals typically trap focus until
  closed).

- When the modal closes, focus should restore to the element that opened it (or
  another logical location).

Playwright makes it straightforward to test these conditions:

```ts
// Assume the "Add to Cart" modal from earlier tests
await page.getByRole("button", { name: "Add to Cart" }).click();
const modalDialog = page.locator("#add-to-cart-modal");
await expect(modalDialog).toBeVisible();

// The first focusable element in modal should be focused.
const firstFocusable = modalDialog.locator("button").first();
await expect(firstFocusable).toBeFocused();

// Press Tab repeatedly within modal and ensure focus stays in modal
for (let i = 0; i < 5; i++) {
  await page.keyboard.press("Tab");
  const activeId = await page.evaluate(
    () => document.activeElement?.closest("#add-to-cart-modal")?.id,
  );
  expect(activeId).toBe("add-to-cart-modal");
}

// Close modal (press Escape or click close button)
await page.keyboard.press("Escape");
await expect(modalDialog).toBeHidden();

// After closing, focus should return to the "Add to Cart" button
const triggerButton = page.getByRole("button", { name: "Add to Cart" });
await expect(triggerButton).toBeFocused();
```

This sequence rigorously checks focus containment and restoration. If any step
fails, it indicates a bug in the focus management logic, perhaps a missing call
to `.focus()` or incorrect `tabindex` attributes.

Such tests ensure **operability** – one of the four principles of accessibility
(POUR: Perceivable, Operable, Understandable, Robust). They catch issues that
static analysis never could (for example, `axe-core` may not report that focus
was lost when a dialog closed).

### 3.3 Accessibility Tree Snapshots for Semantic Regression

In a complex UI, it is possible to inadvertently change or break semantic
structure while refactoring, even if everything “looks” fine visually. To guard
against this, the framework uses **accessibility tree snapshots** in Playwright.
The accessibility tree is what assistive technologies actually interact with a
computed structure derived from the DOM, listing elements with roles, names, and
properties as a screen reader would perceive them.

Playwright provides an API `page.accessibility.snapshot()` that returns this
tree. The adopted pattern is to take **snapshot files** (in a serialized format
like JSON or YAML) of key components or pages, and then use a custom matcher to
compare future runs against the baseline. This is analogous to visual snapshot
testing, but for semantics.

A custom Expect matcher (e.g., `toMatchAriaSnapshot()`) simplifies this. The
first time a test runs, it generates a snapshot file (for example,
`product-card.spec.ts-snapshots/product-card-accessibility.yml`). Subsequent
runs diff the current accessibility tree against that file and fail the test if
there is any difference, except for whitelisted changes.

**Example – Product Card Component:**

```ts
test("ProductCard maintains accessible structure", async ({ page }) => {
  await page.goto("/products/widget-pro");
  const card = page.locator(".product-card");
  // Expect the accessibility tree of the product card to match the stored baseline
  await expect(card).toMatchAriaSnapshot();
});
```

On first run, `toMatchAriaSnapshot()` will save something like:

```yaml
role: "group"
name: "Widget Pro"
children:
  - role: "image"
    name: "Widget Pro product image"
  - role: "heading"
    name: "Widget Pro"
    level: 2
  - role: "paragraph"
    name: "$19.99"
  - role: "button"
    name: "Add to Cart"
```

(This is a conceptual illustration of what the accessibility snapshot might
contain.)

If a developer accidentally changed the `h2` in `ProductCard` to an `h3` or
removed an aria-label on the image, the next test run would produce a different
snapshot and cause a failure. This kind of regression test is **highly sensitive
to meaningful changes** but **robust to cosmetic ones**. Unlike raw HTML
snapshots (which would break on any minor markup change), the accessibility
snapshot ignores irrelevant container `<div>`s or styling hooks and focuses
purely on roles, names, and structure. If an extra `<div>` is added for layout
but doesn’t affect roles or names, the snapshot remains the same. But if, say, a
heading level changes or a label is lost, the difference is caught immediately.

This technique helps ensure that refactors or component iterations do not
silently alter the accessible experience. It is especially useful for ensuring
things like:

- Headings hierarchy remains correct (no suddenly skipping from h2 to h4, etc.).

- Buttons and links retain their names.

- Images keep their alt text or accessible name.

- Regions and landmarks (like nav, main, footer) remain in place.

Accessibility snapshot tests should be incorporated for all major composite
components and pages. They serve as a guardrail for semantic consistency.

### 3.4 Visual Regression Testing across Breakpoints and Themes

Accessibility is not just about screen readers and keyboard navigation; visual
presentation matters too. Issues like text getting cut off, colour contrast in
different themes, or layout breakage on small screens can dramatically affect
usability. To catch these, the framework includes **visual regression tests** in
the Playwright suite.

Using Playwright’s screenshot capabilities, snapshots of pages or components can
be generated under various conditions and compared to baselines. Key dimensions
include:

- **Different screen sizes (responsive breakpoints):** Layouts should be tested
  on a small mobile viewport (e.g. 375×667) vs a desktop viewport (e.g.
  1280×800). This ensures the responsive design does not introduce inaccessible
  overflow or content hiding on small screens.

- **Light and Dark themes:** Since the application supports theme switching
  (e.g., `wildside-day` vs `wildside-night` themes), each mode should be
  captured in screenshots. This helps verify colour contrast in each theme and
  catches theme-specific asset issues.

- **Critical pages/components:** Focus should remain on pages like the home
  dashboard, forms, and any component with complex styling (e.g., data
  visualization or maps in this context) for visual snapshots.

**Example – Responsive Header Snapshot:**

```ts
test.describe("Header visual regression", () => {
  // Test at two breakpoints
  const viewports = {
    mobile: { width: 360, height: 740 },
    desktop: { width: 1280, height: 800 },
  };
  for (const [label, size] of Object.entries(viewports)) {
    test(`header looks correct in ${label} view`, async ({ page }) => {
      await page.setViewportSize(size);
      await page.goto("/"); // go to homepage which includes the header
      // If theme defaults to dark, ensure it’s set for consistency
      await expect(page).toHaveScreenshot(`header-${label}.png`);
    });
  }
});
```

The first run will save `header-mobile.png` and `header-desktop.png`. Subsequent
runs will compare screenshots pixel-by-pixel. If a CSS change accidentally
pushes the header’s menu off-screen on mobile, the diff will flag it. The usual
challenges of visual testing remain, such as ensuring consistent fonts and
ignoring dynamic content, but Playwright includes mechanisms for that.

For theme variations, a test might explicitly toggle the theme:

```ts
test("Settings panel visual regression in dark and light mode", async ({
  page,
}) => {
  await page.goto("/settings");
  // Dark theme (default)
  await expect(page.locator("#settings-panel")).toHaveScreenshot(
    "settings-dark.png",
  );
  // Toggle theme button
  await page.getByRole("button", { name: "Switch to light theme" }).click();
  await expect(page.locator("#settings-panel")).toHaveScreenshot(
    "settings-light.png",
  );
});
```

Baseline images are now available for both dark and light modes of the settings
panel. If some text becomes illegible in light mode, the screenshot difference
will reveal it. This effectively acts as an automated **contrast check** across
themes, complementing what axe does.

Visual regression tests are run sparingly because they can be resource-intensive
and occasionally flaky. Flakiness is mitigated by:

- Freezing dynamic data (e.g., use test accounts or stubbed dates so that
  today’s date or random data does not spoil the snapshot).

- Using tolerances or masking for known nondeterministic regions if needed.

- Running on consistent infrastructure (same browser versions, etc., via
  Playwright’s Docker or the cloud workers).

The payoff is worthwhile: the UI can be verified to look as intended and remain
usable at a glance. Many accessibility issues manifest visually, so these tests
catch problems that neither axe nor unit tests would.

### 3.5 Localization and Internationalization Checks

Finally, accessibility extends to supporting users from different locales and
languages. An often overlooked aspect is ensuring that the application handles
localization correctly, both in terms of content and technical attributes like
the page’s language. E2E tests validate internationalization (i18n) features:

- **Locale-specific content loading:** If the application is translated or
  supports multiple languages, switching to a locale should yield the correct
  text.

- **Language attributes:** The `<html lang="">` attribute should be set
  correctly whenever the locale changes, and any region-specific sub-tags should
  be handled if needed. The `lang` attribute is critical for screen readers to
  switch voice profiles.

- **Directionality:** If right-to-left (RTL) languages are supported, tests
  should verify that the `dir="rtl"` attribute is applied and that layout flips
  appropriately.

Playwright allows different locales to be simulated by launching contexts with a
specific locale. For example, `browser.newContext({ locale: 'fr-FR' })` would
make `navigator.language` report French. Alternatively, if the application has a
UI control for language, that control can be used.

**Example – Language Toggle:**

```ts
test("app supports English and Spanish locales", async ({ page }) => {
  await page.goto("/");
  // Default is English
  expect(await page.locator("html").getAttribute("lang")).toBe("en");
  await expect(page.getByRole("heading", { name: "Welcome" })).toBeVisible();

  // Switch locale via UI control (assuming a button or menu)
  await page.getByRole("button", { name: "Español" }).click();
  // Now the page should be in Spanish
  expect(await page.locator("html").getAttribute("lang")).toBe("es");
  await expect(page.getByRole("heading", { name: "Bienvenido" })).toBeVisible();
});
```

This test verifies two things: the `<html lang>` attribute changes from "en" to
"es", and the content of a heading changes from "Welcome" to "Bienvenido". If
either fails, it indicates a localization issue.

For more thorough coverage, an entire suite can run in a non-default locale
using Playwright’s test config. For instance, some critical E2E tests can be
duplicated under a fixture where the context is launched with
`{ locale: 'es-ES' }`. That way, the locale toggle is not the only path under
test; the Spanish version of various flows can also be checked for axe
violations and parity with English behaviour.

By treating localization as a first-class aspect of testing, the
**understandability** aspect of accessibility is safeguarded for all supported
audiences. There is also a practical benefit: missing translation keys or
formatting issues are caught early.

## IV. Performance Strategy and CI Integration

Having a comprehensive test suite is valuable, but it must run efficiently to be
viable in a fast-paced development setting. The strategy layers the tests so
that the cheapest tests **fail fast** and the heavier tests are deferred to
later, possibly parallelized, stages. These layers are also integrated into the
Continuous Integration pipeline so that accessibility is continuously enforced.

### 4.1 Layered Test Execution for Speed

The test architecture naturally splits into tiers that can execute separately:

- **Layer 1: Bun Unit/Integration Tests** – Extremely fast, and suitable for
  every commit or even every file change in watch mode. These catch basic
  regressions immediately. If any of these fail, there is no need to proceed
  further until fixed.

- **Layer 2: Node/JSDOM Axe Tests** – Slightly slower, but still relatively
  quick. They can run in parallel to Layer 1. In a local development flow, they
  may not run on every save, but they should run before pushing code. In CI,
  they run on each PR. If an accessibility rule fails here, the build fails for
  serious issues or at least flags the problem for fixing.

- **Layer 3: Playwright E2E Tests** – The slowest layer, involving real browsers
  and possibly multiple iterations (various viewports, etc.). This layer runs
  less frequently, typically on pull request validation and nightly full runs.
  Within this layer, Playwright can be configured to shard tests across multiple
  workers. Even though each test is heavy, the suite can still complete in a
  reasonable time.

In CI, parallel jobs provide the best throughput:

- The **Bun tests** run in one job. Given Bun’s speed, this job might finish in,
  say, 30 seconds.

- The **axe a11y tests** run simultaneously in another job. This might also take
  on the order of seconds to a minute, depending on how many components are
  scanned.

- The **Playwright E2E tests** run in parallel in another job. This could take a
  few minutes, especially if running on three browsers or doing visual
  comparisons, but Playwright can often keep it around 2-5 minutes by
  parallelizing internally.

By splitting these, the overall CI time is roughly the maximum of these, not the
sum. The **longest** will usually be the Playwright job. If that job takes four
minutes and the others finish in one minute, the total CI test time remains
roughly four minutes. This is acceptable for a PR gate in exchange for the
thorough coverage provided.

GitHub Actions, or an equivalent CI system, should reflect this structure:

- Job “Test: Bun” – runs `bun test` on all non-a11y tests.

- Job “Test: Accessibility (Node)” – runs the Node script for axe tests.

- Job “Test: E2E” – runs `npx playwright test` (possibly with some flags for
  trace or report).

- An overall workflow depends on all three finishing. Fail-fast can be set for
  certain jobs if desired.

This way, a quick failure such as a unit test or basic axe rule failure does not
require waiting for the whole E2E suite to complete. Immediate feedback is
available on what broke.

Additionally, if test runtime becomes a concern, those tests can be partitioned
further. For example, axe tests can run in parallel shards. At the current
scope, that likely is not necessary.

### 4.2 CI Reporting and Failure Triage

Integrating into CI also means handling results smartly. Any **accessibility
violation of high severity** should be treated as a build failure. For instance:

- If axe reports a **critical or serious** violation in either the component
  tests or E2E scans, the CI job fails. This prevents merging code that would
  introduce major accessibility regressions.

- **Minor** or **moderate** issues may be allowed to pass but logged, depending
  on the chosen strictness level, with an automated task created to fix them. In
  practice, the inner loop should catch most issues early, so few if any
  moderate issues should remain by the time E2E runs.

Playwright and the Node test harness can output machine-readable results (such
as JSON). A post-processing step in CI can parse the axe results and summarize
any violations. The framework also generates **HTML reports** for easier
debugging:

- The Node axe tests can produce an HTML report of all violations, with links to
  relevant DOM nodes and suggestions (using the output from `axe-core`).

- Playwright, by default, can produce an HTML report that includes screenshots,
  failing test traces, and related diagnostics. Enable it and upload it as a CI
  artifact on failure.

The resulting workflow is straightforward: if a test fails, especially an
accessibility one, the report artifact can be downloaded to inspect the exact
failure. A Playwright trace may show that focus did not move as expected after
clicking a button, whilst an axe report may highlight a missing form label.

Finally, the process should maintain a **culture of accessibility ownership**.
Failing tests are not made green by updating the tests alone; the expectation is
to _fix the underlying issue_. Because the tests are designed to catch real
problems, the correct response to a failure is usually to correct the component
or page (for example, add the missing `aria-label`, adjust the colour contrast
in CSS, or fix the focus logic in JavaScript).

Severity tagging helps manage this process:

- Tests or checks can be annotated with severity levels (for example, using
  axe’s impact ratings). A CI parser can distinguish them and, if needed, post a
  PR comment such as: “Accessibility issue detected: **low contrast on button
  text** (critical). This must be resolved before merge.”

- For less critical issues that slip through, backlog tickets can be created
  automatically. Ideally, the gating ensures that everything important is caught
  and fixed in the same development cycle.

## V. Synthesis and Implementation Roadmap

The proposed testing framework delivers a **holistic, layered approach** to
accessibility testing without sacrificing development speed. Fast
component-level checks combine with full browser validation so that every new UI
element is both **rapidly verified** and **truly accessible** in practice. Below
is a summary of the two major layers and their roles:

| Dimension       | Component Layer (Bun + Node/JSDOM)                                                                                                                                                                                               | E2E Layer (Playwright)                                                                                                                                                                                                                                    |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Primary Tools   | Bun test runner (Happy DOM) for units; Node + JSDOM for axe scans                                                                                                                                                                | Playwright test runner (Chromium, Firefox, WebKit) with Axe                                                                                                                                                                                               |
| Environment     | Simulated DOM (Happy DOM for speed; JSDOM for standards)                                                                                                                                                                         | Real browser environment (headless or headed as needed)                                                                                                                                                                                                   |
| Scope of Tests  | Isolated components and small integration pieces in memory                                                                                                                                                                       | Full pages and user flows in the deployed app context                                                                                                                                                                                                     |
| Execution Speed | Very fast (milliseconds per test file under Bun; a bit more for axe)                                                                                                                                                             | Slower (seconds per test, overall minutes per suite)                                                                                                                                                                                                      |
| Defects Caught  | **Structural/Semantic issues:** missing labels, incorrect roles, improper ARIA, form associations, and basic functional bugs in components. _(Visual/style issues generally are not caught here due to Happy DOM/JSDOM limits.)_ | **Visual & Interactive issues:** colour-contrast failures, element focus order, keyboard traps, missing focus outlines, responsive layout breakages, incorrect `lang` attributes, integration issues, and end-to-end flows such as modals and navigation. |
| When to Run     | On every code change or commit (developer inner loop); each PR as a quick check                                                                                                                                                  | On each PR merge request (CI gating) and nightly full runs; also useful locally before major releases                                                                                                                                                     |
| CI Role         | Fast feedback that fails the build quickly if a core test or axe rule fails, preventing bad code early                                                                                                                           | Final quality gate that ensures the merged product is accessible in reality and produces review artifacts such as screenshots and traces                                                                                                                  |

This model ensures **accessibility is woven into every stage**: immediate IDE or
terminal feedback catches obvious issues, and CI catches anything that requires
a real browser to detect.

### 5.1 Implementation Plan

To adopt this framework, a phased rollout is advisable:

#### Phase 1: Setup Bun and Basic Testing (Day 0-1)

- **Install/Verify Bun** in the project. Ensure the `bun:test` script is working
  with Happy DOM. Add a `tests/setup-happy-dom.ts` to configure any globals or
  polyfills (for example, setting `document.doctype` if needed, or configuring
  `window.fetch` if used in tests).

- **Establish Test Structure:** Decide on a directory convention (e.g., all
  tests in `tests/` folder, mirroring src structure). Ensure naming for a11y
  tests is in place (e.g., `*.a11y.test.tsx`).

- **Sample Bun Tests:** Write a couple of simple Bun tests for existing
  components to validate that the framework is running and fast. For example,
  test a context provider or a presentational component. This lays groundwork
  and confidence in Bun’s runner.

#### Phase 2: Integrate Node A11y Testing (Day 1-2)

- **Add Dependencies:** Install `axe-core` (or `jest-axe`),
  `@solidjs/testing-library`, `@testing-library/user-event`, and `jsdom` as
  devDependencies for the Node tests. Optionally, add `tsgo` or configure `tsc`
  for quick compilation of tests.

- **Node Test Script:** Create an NPM script, e.g.,
  `"test:a11y": "node scripts/run-a11y-tests.js"`. This script can discover
  `*.a11y.test.tsx` files (via glob), compile them (using tsgo or esbuild), and
  execute them. As an interim, one could use a tool like Mocha or Vitest in Node
  just for these tests – but a lightweight custom runner might suffice.

- **First Axe Test:** Write an a11y test for a simple component (like a
  `<Button>` or a form input) and deliberately introduce a violation (e.g.,
  remove an aria-label) to see that it catches it. Then fix the violation and
  see tests pass. This validates the Node axe pipeline.

- **Disable Incompatible Rules:** Configure the axe wrapper to disable
  `color-contrast` and other problematic rules globally for JSDOM. This can be
  done in a setup portion of the Node tests (for example, using `axe.configure`
  to turn off certain checks). Document these disabled rules clearly,
  referencing that Playwright will cover them.

#### Phase 3: Expand Coverage in Inner Loop (Week 1 ongoing)

- **Write Tests for Key Components:** Start adding tests (both Bun and a11y) for
  all crucial components in the codebase. Prioritize components that are reused
  often or have complex accessibility considerations (dialogs, menus, inputs,
  etc.). Leverage the patterns illustrated (render, axe scan, role/name
  assertions).

- **Enforce Query Convention:** Introduce the ESLint rules for Testing Library
  to ensure no `getByTestId` selectors slip in. Add this to the CI lint step.
  Communicate the guideline clearly: test as a user, not via implementation
  details.

- **Team Training:** Host a short workshop or provide documentation on how to
  write these new tests. Show examples of good patterns. This helps scale the
  effort as multiple developers contribute tests.

#### Phase 4: Playwright Setup (Week 2)

- **Install Playwright and Config:** Add Playwright test runner
  (`@playwright/test`) and run `npx playwright install` to get browser binaries.
  Create `playwright.config.ts` with proper settings: use a base URL (if running
  against a dev server or Storybook), configure timeouts, and set test directory
  (e.g., `tests/e2e`). Also enable trace on failure and screenshot on failure
  for debugging.

- **Axe in Playwright:** Import `@axe-core/playwright` in a test to ensure it
  works. Write a basic test that navigates to a page and runs an axe scan. This
  validates that the environment can do in-browser scans (and helps fine-tune
  any needed delays or wait-fors to stabilize the page).

- **Visual Baseline:** Write a simple visual test for a static page and generate
  the baseline screenshot. Commit this baseline image to the repo because
  reference snapshots are generally tracked over time. Ensure that small pixel
  differences do not cause noise by adjusting thresholds or screenshot options
  if needed (such as disabling antialiasing via CSS in test).

- **Accessibility Snapshots:** Implement the `toMatchAriaSnapshot` matcher. This
  might involve calling `page.accessibility.snapshot()` and doing a deep
  comparison with a stored object. Existing example implementations from the
  Playwright community can also be used if available. Start by snapshotting a
  simple component or page to test the flow (store file, compare on next run).

#### Phase 5: Full E2E Test Development (Week 2-3)

- **Implement Scenario Tests:** Begin writing tests for critical user flows:
  e.g., “user can navigate through main menu with keyboard,” “opening and
  closing modals manages focus correctly,” “form error messages are announced or
  present in DOM,” etc. Use the patterns from 3.2.1 and 3.2.2. These tests might
  require building out some test fixture data or using a staging environment if
  working with real backend – ensure they are deterministic.

- **Implement Visual & I18n Tests:** Add tests for visual snapshots of key pages
  in both themes, and for switching language as described. Mark these as either
  part of the main Playwright suite or a separate suite if they should run less
  frequently (Playwright can tag tests, for example with an `@visual` tag, to
  filter runs).

- **Parallelize where possible:** Playwright parallelizes by default; confirm
  that tests are isolated (avoid tests interfering with each other by using
  fresh page and context instances for each run, which Playwright does by
  default per test). Use test fixtures for different viewport sizes or locales
  to run those in parallel threads.

#### Phase 6: CI Integration (Week 3)

- **Set up CI jobs:** Modify the pipeline to include the three jobs (Bun tests,
  Node a11y, Playwright). Use caching where possible (cache `node_modules` or
  Playwright browsers for speed). If using GitHub Actions, ensure runners have
  the necessary dependencies (or use the official Playwright action).

- **Artifact & Reporting:** Configure each job to output results. For Bun/Node
  tests, a simple console output might suffice, but JUnit or HTML reports can
  also be emitted using tools like `bun test --reporter junit` or a custom
  format. For Playwright, enable the HTML report and artifacts (videos, traces
  on failure). Upload these in CI (Actions artifacts).

- **Failure Criteria:** Decide the threshold for failing. Likely any failing
  test fails the job. For axe violations, the suite naturally fails if any
  violations remain because the tests assert zero findings. An allow-list
  mechanism may be incorporated if there are legacy known issues (though fixing
  them remains preferable).

- **Notification:** Update PR template or guidelines to mention that a PR will
  not be merged if accessibility tests fail. Possibly integrate a status check
  that specifically surfaces “Accessibility Checks” vs “Unit Tests” so it’s
  clear where an issue lies.

#### Phase 7: Ongoing Maintenance and Coverage (Week 4+)

- **Backfill Tests:** Gradually increase coverage by adding tests for remaining
  components and pages. The goal is not 100% coverage for coverage’s sake, but
  rather coverage of all **accessibility aspects** of each element. Use the a11y
  test suite as a checklist for components – if a component has an interactive
  role, ensure it has at least one axe test verifying its basics.

- **Update Snapshots Intentionally:** When a legitimate UI change occurs, update
  the corresponding accessibility and visual snapshots. Treat these updates as
  code changes requiring review and double-check that the new output is correct.

- **Monitor Flakiness:** Keep an eye on E2E test stability. If a test is flaky
  (randomly failing), investigate and fix it promptly, as flaky tests can erode
  team trust in the suite. Playwright’s trace viewer can help debug intermittent
  failures. It might be a timing issue or a test that needs a slight adjustment
  (like waiting for an animation to finish).

- **Continuous Improvement:** As new accessibility best practices or tools
  emerge, integrate them. For example, if a library for detecting screen reader
  announcements becomes available, add tests for live region updates. If Bun
  adds support for JSDOM in the future, the Node harness could be simplified and
  axe could run directly in Bun, dropping the extra complexity. Remain adaptable
  to improve the framework.

By following this roadmap, the testing regimen grows incrementally and robustly.
Each phase delivers tangible benefits (immediate bug catching, new coverage)
without overwhelming the team. Within a few weeks, accessibility testing moves
from an abstract concern to an everyday part of development, with contributors
supported by quick feedback and protected by comprehensive end-to-end checks.

### 5.2 Outcomes and Benefits

In conclusion, this accessibility-first testing architecture embeds the mantra
“**shift left** on accessibility” into the development lifecycle. Fast Bun tests
and Node axe checks mean issues are caught at the earliest possible moment.
Playwright E2E ensures that nothing slips through the cracks when it comes to
real user experience. Periodic manual audits or the diligence of individual
developers remembering every ARIA rule are no longer the only safety net; the
system provides continuous coverage.

**Key benefits:**

- **Higher Quality UIs:** Fewer accessibility bugs reach production, resulting
  in an inclusive product by default.

- **Developer Efficiency:** Quick inner loop tests prevent context-switching;
  developers fix issues while the code is fresh in mind. The outer loop, while
  slower, is automated and saves QA effort by catching issues that would be hard
  to find manually.

- **Documentation by Tests:** The tests themselves serve as living documentation
  of accessibility expectations. New team members can read tests to understand
  how components should behave (for example, “Modal should return focus to
  opener on close – see test X”). This knowledge sharing is invaluable.

- **Compliance and Confidence:** Conformance to standards (such as WCAG 2.2 AA)
  can be asserted with confidence because automated checks cover many criteria
  (contrast, focus order, semantics, etc.). This mitigates risk from a
  legal/compliance standpoint and broadens user reach.

By modernizing the original design to use **Bun for speed** and a **Node assist
for axe**, the core philosophy is preserved: accessibility testing should
enhance, not hinder, development velocity. Every layer of testing reinforces the
others, and together they create a safety net where accessibility regressions
simply cannot survive unnoticed. This is the essence of high-velocity,
accessibility-first development – moving fast **without breaking things** for
users.

## Footnotes

[^1]:
    Happy DOM issue tracker – documented incompatibility of `Node.isConnected`
    implementation with axe-core’s expectations.

[^2]:
    Bun documentation and Bun v1.1.41 release notes – Bun continues to
    recommend Happy DOM for DOM tests, while v1.1.41 improved `node:vm`
    compatibility so `jsdom` works more reliably when it is installed and
    configured manually.

[^3]:
    Deque `axe-core` documentation – notes on JSDOM support and rules like
    `color-contrast` being inapplicable in headless DOM.
