---
paths:
  - "SovEmu.Web/src/**/*.{ts,tsx}"
---

# React and TypeScript style (SovEmu.Web)

Code style for the web client, taken from Sovereign's `react.md` and `react-typescript.md` and
fitted to this app, which has no design-system catalogue, no form library and code-based routes.
Where the code is laid out is [web.md](web.md).

## Components

- Functional components and hooks only. The one class is `ErrorBoundary`
  (`src/components/ErrorBoundary.tsx`), because an error boundary cannot be a function; every
  screen is wrapped in it (`router.tsx`), keyed by its URL, and it reports what it catches as
  `SCREEN_ERROR`.
- Extract any component whose visibility is toggled into a named component.
- Extract involved state or event logic into a custom hook (`use*.ts` beside its component, as
  `useGameInUrl.ts`), or into a plain class or function in `src/lib/` when it needs no React.
  Components stay focused on rendering.
- Any component cleanly extractable into its own unit is extracted. It can live in the same file
  until a second use arises.
- Reuse what `src/components/ui/` has before writing new markup (see [web.md](web.md)).

## Effects

`useEffect` is for synchronising with something **outside** React: a timer, a subscription, a
global listener, a WebRTC session, the gamepad loop. It is not a place to compute state, and every
one added is a thing that can run at the wrong time.

- **Never fetch into state from an effect.** Reads go through TanStack Query (`q` in
  `src/lib/data.ts`), which writes the staleness guard once: a response that arrives after its
  inputs changed must never be rendered.
- **Derive, do not synchronise.** If a value can be computed during render from props and state,
  compute it. An effect that only calls `setState` from other state is a re-render for nothing.
- **A user action is an event, not an effect.** Submitting, opening a dialog, confirming and
  starting a game belong in the handler that caused them.
- **List every dependency, or restructure until you can.** Biome's `useExhaustiveDependencies`
  warns; treat the warning as a bug, and never silence it. When a value must be read but must not
  re-trigger (a caller's inline callback, a flag a listener checks), hold it in a ref and let the
  effect depend on what genuinely triggers it.
- **Every subscription returns a cleanup** that undoes exactly what the effect did: a screen
  unmounts on every navigation, and a leaked listener keeps steering the next screen.
- One effect per concern. Two unrelated jobs sharing a dependency list each run when only the
  other's input changed.

## State

- Keep state minimal and colocated at its point of use; derive rather than duplicate.
- Share a value many components need through React Context (as `useToast`), not prop drilling.
- Avoid needless re-renders, but measure before reaching for `memo`/`useMemo`/`useCallback`.

## Forms

Forms are native `<form>`s read with `FormData` in `onSubmit`; there is no form library.

- Validate that required fields are non-empty with `required` and a submit button that cannot
  submit until they are; never an `onBlur` "required" check, which fires when pad focus merely
  passes through an empty field.
- Give inputs their real `type` (`url`, `number`, `password`) and `autoComplete`.
- Show the server's error message (the `ApiError`) in the form, beside what it is about.

## Interactive elements and accessibility

- Every clickable element is a `<button>`, an `<a href>` or a Radix control, never a clickable
  `<div>`, so the pad, the keyboard and screen readers all reach it. Buttons and links get
  `cursor-pointer` (Tailwind v4 no longer sets it).
- An icon-only button has an `aria-label` (and a tooltip where its purpose is not obvious).
- Add ARIA roles, labels and key handling wherever a Radix primitive does not supply them; the
  end-to-end tests find elements by role and label, so missing ones show up there.

## Types

- A component's props, and any other object argument, are a named `type` (`type GameCardProps =
  { … }`) above it, not an inline object type in the signature; a generic component's are generic
  too (`type SelectProps<T extends string>`).
- Types used in one file are declared at its top; types shared across files are exported from the
  module that owns the concept (`lib/controllers.ts`, `lib/api.ts`). There is no `types/` folder.
  API types come from gql.tada (`ResultOf<typeof Doc>`), never re-declared by hand.
- Compose types with `Omit<T, K>` / `Pick<T, K>` / `ComponentProps<…> &` rather than defining new
  ones that drift.
- No TypeScript `enum`: use string-literal unions (`type PillTone = "neutral" | …`) and gql.tada's
  `Enum<"Name">`.
- Type-only imports use `import type` (or an inline `type` specifier); `verbatimModuleSyntax` is
  on, so `tsc` fails without it.
- `strict` is on. Prefer narrowing to `!`; Biome allows a non-null assertion, but only where the
  code just made it true (`ref.current!` inside a handler that only runs after mount).

## Formatting

Biome owns it (`biome.jsonc`: 2-space indent, 120 columns, double quotes): run `npm run format`,
never hand-format against it. A `biome-ignore` always carries its reason.
