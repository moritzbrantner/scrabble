# Product UI preferences

This document records opinionated product-design preferences for agent-built interfaces.

These preferences are stronger than generic dashboard conventions or aesthetic defaults. Apply them across demos, labs, GitHub Pages applications, internal tools, and product surfaces unless a product-specific requirement explicitly overrides them.

Do not infer additional likes or dislikes from the existence of a section below. Unfilled sections are placeholders for future recorded preferences.

## General decision rule

Every prominent UI element must earn its visual weight by helping the user understand something, decide something, or do something.

Data being easy to compute is not a reason to display it. A familiar dashboard pattern is not a reason to use it. Visual empty space is not a problem that needs to be filled with metrics.

When an element has no clear user-facing consequence if removed, prefer removing it.

## Anti-pattern: decorative counter and KPI cards

### Rule

Do not create standalone cards, tiles, hero statistics, or dashboard widgets whose main content is merely a count or total unless the count itself is required.

Examples of patterns to avoid by default:

- `24 objects`
- `8 files`
- `12 scenes`
- `6 matches`
- `3 servers`
- `142 requests`
- `17 components`
- any similar count promoted into a large card simply because the value is available

This applies even when such cards are common in admin dashboards, analytics templates, generated demos, or design-system examples.

### Why this pattern is undesirable

Decorative counters often:

- consume high-value visual space without adding proportional information;
- create a generic or artificial "dashboard" appearance;
- imply that a quantity is important merely because it is measurable;
- distract from the actual objects, results, controls, relationships, or explanations the user came to inspect;
- repeat information that is already obvious from a nearby collection or status;
- encourage agents to fill layouts instead of designing around the user's task.

The goal is not to eliminate numbers. The goal is to avoid promoting weak information into visually dominant UI.

### Preferred alternatives

If the count is not useful, omit it entirely.

If the count is useful only as context, keep it close to the thing it describes. Suitable placements include:

- next to a collection heading;
- in a filter or search result summary;
- next to a selection state such as `3 selected`;
- inside progress or queue feedback;
- next to an action whose behavior depends on the quantity;
- in explanatory text when the number materially helps interpretation.

Prefer showing the actual objects, results, relationships, state, or controls over showing a card that merely counts them.

Prefer a chart or visualization only when it reveals structure such as change, distribution, comparison, correlation, or composition. A chart is not a replacement decoration for a useless counter.

### Exceptions

A prominent count may be appropriate when at least one of these is true:

1. The product requirement explicitly asks for the count.
2. The user must notice the count to make a decision or take an action.
3. The value represents actionable operational state, such as pending work, failures, a quota, a limit, or a threshold that changes behavior.
4. The count is itself the subject being analyzed rather than incidental metadata.

Even in these cases, a standalone card is not automatically the right presentation. Use the least visually dominant representation that still serves the task.

### Agent check before adding a counter

Before adding a count to an interface, answer these questions:

- Did the requirement actually ask for this number?
- What decision or action changes because the user sees it?
- Is the number meaningful in isolation?
- Would placing the number beside its related content be clearer than creating a card?
- If this element disappeared, would the user lose important capability or understanding?

If the answers do not establish a concrete purpose, omit the counter.

## Cards and surfaces

Reserved for future recorded preferences about card usage, grouping, borders, containers, and visual surfaces.

## Dashboard composition

Reserved for future recorded preferences about dashboard-like layouts and information hierarchy.

## Information density and whitespace

Reserved for future recorded preferences about density, spacing, and how much information should appear at once.

## Charts and visualizations

Reserved for future recorded preferences about charts, graphs, diagrams, and data visualization.

## Navigation and page structure

Reserved for future recorded preferences about navigation, tabs, sidebars, page hierarchy, and discoverability.

## Forms and controls

Reserved for future recorded preferences about forms, buttons, inputs, toggles, menus, and interaction patterns.

## Status, badges, and metadata

Reserved for future recorded preferences about badges, labels, chips, state indicators, timestamps, and secondary metadata.

## Motion and transitions

Reserved for future recorded preferences about animation, transition, and motion behavior.

## Copy and labeling

Reserved for future recorded preferences about UI wording, headings, labels, helper text, and explanatory copy.

## Responsive and mobile behavior

Reserved for future recorded preferences about responsive layout and mobile interaction.
