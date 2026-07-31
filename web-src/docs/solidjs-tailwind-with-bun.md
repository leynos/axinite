# SolidJS + Tailwind with Bun 1.3.0

> **Note:** This repository currently publishes a static prototype. This guide
> exists for Bun-centric SolidJS experiments and for the eventual SPA
> implementation path.

This is a pragmatic walkthrough for building and serving a small SolidJS +
Tailwind app using Bun. The recommended path is to let Bun manage packages and
scripts while Vite handles the Solid compilation pipeline.

## 0) Prerequisites

- Bun installed and available on `PATH`
- A current browser for local preview

Verify Bun:

```bash
bun --version
```

## 1) Scaffold a SolidJS project

Solid's official quick start supports `bun create solid`. For a Tailwind-ready
TypeScript setup, choose the `ts-tailwindcss` template when prompted.

```bash
mkdir my-app && cd my-app
bun create solid
```

During the prompts:

1. Choose a project name.
2. Select a non-SolidStart app unless server rendering is explicitly required.
3. Choose `ts-tailwindcss` for a Bun + Vite + Tailwind starting point.

For projects started directly from Vite, Bun's Vite guide also supports this
flow:

```bash
bun create vite my-app
cd my-app
bun install
```

When prompted by Vite, choose the `solid-ts` template.

## 2) Run the dev server

Use the project's existing Vite script through Bun:

```bash
bun run dev
```

This setup provides:

- Vite's Solid-aware module pipeline,
- fast rebuilds during development, and
- a simpler path to production builds than relying on Bun's older framework-
  specific shortcuts.

## 3) Minimal SolidJS + Tailwind shape

**`index.html`**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Bun + SolidJS + Tailwind</title>
  </head>
  <body class="min-h-screen bg-slate-950 text-slate-100">
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

**`src/index.css`**

```css
@import "tailwindcss";
```

**`src/main.tsx`**

```tsx
import { render } from "solid-js/web";
import "./index.css";

function App() {
  return (
    <main class="grid min-h-screen place-items-center p-6">
      <section class="text-center">
        <h1 class="text-3xl font-bold tracking-tight">Hello, Bun</h1>
        <p class="mt-2 text-slate-300">
          SolidJS + Tailwind served through Bun scripts and Vite.
        </p>
        <button class="mt-6 rounded-xl bg-emerald-500 px-4 py-2 font-medium text-emerald-950 hover:bg-emerald-400">
          Click me
        </button>
      </section>
    </main>
  );
}

render(() => <App />, document.getElementById("root")!);
```

The important shift from the old React version is simple:

- no `react-dom/client`,
- no `createRoot`,
- no `className`,
- and no React Fast Refresh terminology.

Solid uses `render` from `solid-js/web`, JSX attributes such as `class`, and
fine-grained reactivity through signals rather than component-wide re-renders.

## 4) Add state the Solid way

For local interactive state, prefer signals:

```tsx
import { createSignal } from "solid-js";

function Counter() {
  const [count, setCount] = createSignal(0);

  return (
    <button class="btn" onClick={() => setCount(count() + 1)}>
      Count: {count()}
    </button>
  );
}
```

Use stores for nested object state, and reserve heavier client-state libraries
for cases where the state graph genuinely outgrows signals and context.

## 5) Build for production

Use the Vite build through Bun:

```bash
bun run build
```

Or invoke Vite directly through Bun:

```bash
bunx --bun vite build
```

The output is a static `dist/` directory suitable for GitHub Pages, Netlify,
Cloudflare Pages, S3, or any other static host.

## 6) Preview the production build

```bash
bun run preview
```

If the template does not ship a preview script yet, add one:

```json
{
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview"
  }
}
```

## 7) Recommended next steps for this repo family

For the Axinite/Wildside/Corbusier direction, the next additions should be:

- Tailwind CSS v4 plus DaisyUI v5 for the design layer,
- Kobalte for accessible, unstyled interactive primitives,
- TanStack Router for Solid for SPA routing,
- `@solidjs/testing-library` plus jsdom for component tests, and
- TanStack Query Solid where server-state caching is needed.

## Sources

- Solid quick start: `https://docs.solidjs.com/quick-start`
- Bun with Vite: `https://bun.com/docs/guides/ecosystem/vite`
