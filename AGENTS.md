# SOURCE UI Interaction Rules

- Interactive UI elements must show the hand cursor on hover. This includes buttons, segmented controls, clickable tabs, icon buttons, links, menu triggers, and any custom control implemented with `div`, `span`, or other non-button elements.
- Do not rely on browser defaults alone. If a custom interactive element would not naturally show the pointer cursor, style it explicitly.
- Disabled controls should not show the hand cursor. Use a non-interactive cursor state such as `not-allowed` or the platform default for disabled UI.
- When adding or refactoring shared UI primitives, preserve this pointer-on-hover rule so it stays consistent across the full app.
- Paragraph text and other multi-line explanatory copy should be constrained for readability. Target a readable measure of roughly 50 to 60 characters per line, and avoid letting paragraph-style text run wider than that unless there is a strong, explicit reason.
- When implementing this in UI, prefer text containers sized in `ch` units, such as `max-width: 60ch`, rather than relying only on page-level width constraints.
