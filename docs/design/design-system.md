# Design system notes (M1)

- Tokens live in `apps/desktop/src/styles/globals.css` as CSS variables in OKLCH, exposed to
  Tailwind v4 via `@theme inline`. Components use semantic names (`bg-card`, `text-muted-foreground`),
  not raw palette values.
- Two themes: dark (default) and light, plus a `system` preference. The root `.dark` class is
  applied by `useAppliedTheme`.
- UI scale is persisted separately from world state and applied through the `--app-scale`
  CSS variable; range 80–140 %.
- shadcn/ui primitives are vendored under `src/components/ui`, styled with `class-variance-authority`
  and `tailwind-merge`. New primitives must follow the same pattern and use Radix for behavior.
- Density: 13 px base text, 28 px controls, 8 px scrollbars. The 3D viewport keeps the remaining
  space; panels scroll independently and never grow the document.
- Accessibility: visible focus ring token, aria labels on icon-only buttons, keyboard-reachable
  dialogs and tabs, `sr-only` text for icon-only actions.

## Layout contract

```text
toolbar (40px)
├── world tree (224px) │ viewport (flex) │ inspector (256px)
bottom panel (176px)
status bar (24px)
```

The layout is CSS grid/flex only; the future Three.js canvas is an imperative child of the
viewport region and never a React-managed block tree.
