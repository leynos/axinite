import { render } from "solid-js/web";

import { AppProviders } from "./app/providers";
import { AppRouter } from "./app/router";
import { i18nReady } from "./lib/i18n/runtime";
import "./styles/index.css";

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
          <AppRouter />
        </AppProviders>
      ),
      mountNode
    );
  });
