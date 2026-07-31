import { render } from "solid-js/web";

import { AppProviders } from "./app/providers";
import { AppRouter } from "./app/router";
import { AuthGate } from "./components/auth-gate";
import { i18nReady } from "./lib/i18n/runtime";
import { installTestHooks } from "./lib/test-hooks";
import "./styles/index.css";

// Mount the deliberate e2e test-hook surface (window.__axinite) unconditionally
// at boot. It is installed here rather than inside the chat component so the
// object exists from first paint (before the chat route mounts); the chat
// surface then register/unregisters the concrete stream controls as it mounts.
installTestHooks();

const mountNode = document.getElementById("app");

if (!mountNode) {
  throw new Error("Missing #app mount node");
}

void i18nReady
  .catch((error) => {
    console.error("[i18n] App bootstrap locale initialization failed", error);
    return undefined;
  })
  .then(() => {
    render(
      () => (
        <AppProviders>
          <AuthGate>
            <AppRouter />
          </AuthGate>
        </AppProviders>
      ),
      mountNode
    );
  });
