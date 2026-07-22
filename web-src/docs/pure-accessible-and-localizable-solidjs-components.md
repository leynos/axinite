# Pure, Accessible, and Localizable SolidJS Components

## Introduction

This document defines a SolidJS-first component architecture for the v2a
front-end family. The goal is straightforward: components should be easy to
reason about, easy to test, accessible by default, and safe to localize without
turning UI code into a translation dump or a side-effect nest.

The recommended stack is layered on purpose:

- **Behavioural layer:** Kobalte primitives provide accessible interaction
  behaviour and WAI-ARIA aligned semantics for complex controls.
- **Presentational layer:** Tailwind CSS v4 and DaisyUI v5 provide styling,
  tokens, and semantic utility composition.
- **Data and orchestration layer:** Solid signals and stores handle local UI
  state; TanStack Query Solid handles server state; richer workflows can move
  into explicit state machines when needed.
- **Localization layer:** `i18next` remains the translation engine, with a
  Solid-compatible binding layer such as `solid-i18next` or a thin custom
  context wrapper when tighter control is preferable.

## 1) Purity in SolidJS

Solid does not use React-style component-wide re-rendering. It builds a
fine-grained reactive graph from signals, memos, resources, and effects. That
changes the performance model, but not the architectural standard: rendering
logic should still stay pure.

A Solid component should:

- derive UI from props, signals, and context,
- avoid mutating external state during render,
- keep DOM writes and subscriptions out of the render path, and
- push side effects into event handlers, `createEffect`, or dedicated data
  adapters.

### Practical rule

If a block of code can run repeatedly without changing the outside world, it
belongs in the render path. If it talks to the network, storage, timers, or
global browser APIs, move it elsewhere.

## 2) Reactivity without React-era workarounds

In Solid, performance does not come from scattering `memo`, `useCallback`, and
comparison functions around the tree. The baseline model is already granular.
The main job is to choose the right primitive:

- `createSignal` for scalar or narrow local state,
- `createStore` for nested object state,
- `createMemo` for derived values,
- `createResource` for async values tied to reactive inputs,
- context only for genuinely shared cross-cutting state.

Avoid porting React habits directly:

- do not wrap every helper in indirection just to "avoid re-renders",
- do not centralize trivial local UI state out of fear of parent updates,
- do not mutate store objects behind Solid's back.

### Example: presentational purity

```tsx
type StatusBadgeProps = {
  label: string;
  tone: "neutral" | "success" | "warning";
};

export function StatusBadge(props: StatusBadgeProps) {
  return (
    <span
      classList={{
        badge: true,
        "badge-ghost": props.tone === "neutral",
        "badge-success": props.tone === "success",
        "badge-warning": props.tone === "warning",
      }}
    >
      {props.label}
    </span>
  );
}
```

This component has no data fetching, no global writes, and no hidden mutable
dependencies. That is the standard to preserve.

## 3) Behavioural foundation: Kobalte

Kobalte is the preferred interaction layer for SolidJS because it is:

- built specifically for Solid,
- intentionally unstyled,
- aligned with WAI-ARIA authoring practices, and
- responsible for focus management, keyboard behaviour, and assistive-
  technology wiring that should not be reimplemented ad hoc.

Representative guarantees from the official docs:

- **Dialog:** modal and non-modal modes, focus trapping, Escape to close, scroll
  locking, and screen-reader announcements through title and description.
- **Tabs:** semantic tab-to-panel linking, automatic or manual activation, and
  RTL/LTR keyboard navigation.
- **Select:** listbox semantics, typeahead, focus management, and hidden native
  `<select>` integration for forms.
- **Toggle Button:** native button semantics with `data-pressed` state and
  keyboard support for `Space` and `Enter`.

### Rule of use

Use Kobalte for interaction semantics, then apply DaisyUI and Tailwind classes
for presentation. Do not bury accessibility behaviour inside one-off custom
components when a primitive already exists.

## 4) Styling: DaisyUI + Tailwind

The styling boundary should stay clear:

- DaisyUI for component roles and theme-aware tokens,
- Tailwind utilities for local layout, spacing, and stateful refinement,
- semantic project classes for repeated product-language patterns.

This produces a stable split:

- Kobalte owns behaviour,
- DaisyUI/Tailwind own appearance,
- the project owns naming.

### Example: dialog shell

```tsx
import { Dialog } from "@kobalte/core/dialog";

export function DeleteDialog() {
  return (
    <Dialog>
      <Dialog.Trigger class="btn btn-error">Delete</Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay class="fixed inset-0 bg-black/50" />
        <div class="fixed inset-0 grid place-items-center p-6">
          <Dialog.Content class="w-full max-w-md rounded-box bg-base-100 p-6 shadow-xl">
            <Dialog.Title class="text-lg font-semibold">
              Delete task
            </Dialog.Title>
            <Dialog.Description class="mt-2 text-sm text-base-content/75">
              This action cannot be undone.
            </Dialog.Description>
            <div class="mt-6 flex justify-end gap-2">
              <Dialog.CloseButton class="btn btn-ghost">
                Cancel
              </Dialog.CloseButton>
              <button class="btn btn-error">Delete</button>
            </div>
          </Dialog.Content>
        </div>
      </Dialog.Portal>
    </Dialog>
  );
}
```

## 5) State and server data

### Local UI state

Use signals for immediate interaction state:

```tsx
const [isOpen, setIsOpen] = createSignal(false);
```

Use stores when state is nested or record-shaped:

```tsx
const [filters, setFilters] = createStore({
  status: "all",
  priority: "all",
});
```

### Server state

For async server state, use TanStack Query Solid instead of hand-rolled
loading/error caches. The Solid adapter exists specifically to handle fetching,
caching, synchronization, and updates in Solid applications.

This avoids three recurring mistakes:

- duplicating request state into ad hoc signals everywhere,
- mixing transport concerns into presentational components, and
- rebuilding retry/cache invalidation logic component by component.

## 6) Localizable component boundaries

Localization should not leak everywhere. Components should receive either:

- already-resolved display strings, or
- stable translation keys plus interpolation values where the caller controls
  the locale context.

Keep the split disciplined:

- translation bundles own UI chrome and sentence structure,
- entity models own localized domain text where the entity itself is content,
- components format and present, but do not invent fallback copy.

### Practical recommendation

For SolidJS, keep `i18next` as the core engine and expose it through a
Solid-compatible binding layer. `solid-i18next` is a reasonable option when a
familiar provider-and-hook model is preferred. If the repo needs tighter
control, wrap the `i18next` instance in a small Solid context and expose only
the translation helpers the application should use.

## 7) Accessibility expectations

Every component intended for reuse should be testable through accessible queries
and should preserve these guarantees:

- correct native element choice before ARIA,
- visible and logical keyboard interaction,
- robust labelling,
- focus handling that survives dialogs, menus, and route changes,
- localized text that does not break names or descriptions.

For component tests, prefer:

- `@solidjs/testing-library`,
- `jsdom`,
- `@testing-library/user-event`, and
- `axe-core` or `jest-axe` in the Node/JSDOM accessibility lane.

## 8) Controller/view split

For non-trivial flows, keep orchestration outside the presentational component.
One workable split is:

- `useDeleteTaskController()` handles mutations, loading, and optimistic state.
- `DeleteTaskDialogView` renders Kobalte primitives and display text.
- the route or screen container wires them together.

This keeps the presentational layer easy to snapshot, inspect, and reuse.

## 9) Recommended defaults

Use these defaults unless there is a strong reason not to:

1. Build interaction components on Kobalte.
2. Use Solid signals and stores before introducing heavier state tooling.
3. Use TanStack Query Solid for server-state caching and invalidation.
4. Keep localization outside entity rendering where possible.
5. Test components through accessible roles and names, not test IDs.
6. Keep styling declarative with DaisyUI roles and Tailwind utilities.

## Sources

- Solid signals: `https://docs.solidjs.com/concepts/signals`
- Solid testing: `https://docs.solidjs.com/guides/testing`
- Kobalte introduction: `https://kobalte.dev/docs/core/overview/introduction/`
- TanStack Query Solid overview:
  `https://tanstack.com/query/v5/docs/framework/solid/overview`
- `solid-i18next` repository: `https://github.com/mbarzda/solid-i18next`
