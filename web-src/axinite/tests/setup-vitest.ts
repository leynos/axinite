/**
 * @file Setup for the unit and behaviour Vitest project (`vitest.config.ts`):
 * registers the jest-dom matchers on Vitest's `expect` and installs the
 * locale-serving `fetch` stub so no i18n backend retry outlives a test file.
 */

import "@testing-library/jest-dom/vitest";
import { installTestFetch } from "./support/i18n-test-runtime";

installTestFetch();
