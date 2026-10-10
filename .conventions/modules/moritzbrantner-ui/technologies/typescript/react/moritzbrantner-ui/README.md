# @moritzbrantner/ui conventions

## MORITZUI-001 — Compose applications from public component tiers

- Select public exports by semantic responsibility: stable primitives, patterns compositions, data collections, and shell chrome.
- Keep routing, server state, authorization, copy, and side effects in the app; use labs only with explicit upgrade ownership.

## MORITZUI-002 — Select one concrete theme contract

- Import exactly one concrete theme stylesheet per application surface.
- Import component-sources.css when rendering package components; use scoped themes only for intentional coexistence.

## MORITZUI-003 — Do not add counters or KPI cards by default

- Do not add cards, tiles, hero statistics, or dashboard widgets whose primary purpose is merely to display a count or total.
- This includes counts such as objects, files, scenes, matches, requests, servers, records, components, or other quantities that happen to be available.
- A number is not useful merely because it can be computed. Promote it only when it answers an active user question or materially changes a decision or workflow.
- Do not use counters as filler, as visual balance, or to make a page look more like a dashboard.
- If a count is useful only as context, place it next to the collection, filter, selection, progress state, or action it describes rather than giving it a standalone card.
- Add a prominent counter only when the product requirement explicitly asks for it or the count itself is operationally actionable. When uncertain, omit it.

## Component selection

- Use ViewHeader for page identity/actions, DescriptionList for facts, ResourceList or ItemGroup for collections, DataGrid for comparison, and StateView for loading, empty, error, or offline states.
- Use Card only for a genuinely independent bounded object; otherwise prefer spacing, headings, separators, or a shared surface.

## Product UI preferences

- Treat [`UI-PREFERENCES.md`](UI-PREFERENCES.md) as the opinionated product-design layer for agent-built interfaces.
- Product-specific requirements may override these defaults explicitly; agents must not infer an override merely from available data or existing dashboard patterns.
