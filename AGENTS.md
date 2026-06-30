# SOURCE UI Interaction Rules

- Interactive UI elements must show the hand cursor on hover. This includes buttons, segmented controls, clickable tabs, icon buttons, links, menu triggers, and any custom control implemented with `div`, `span`, or other non-button elements.
- Do not rely on browser defaults alone. If a custom interactive element would not naturally show the pointer cursor, style it explicitly.
- Disabled controls should not show the hand cursor. Use a non-interactive cursor state such as `not-allowed` or the platform default for disabled UI.
- When adding or refactoring shared UI primitives, preserve this pointer-on-hover rule so it stays consistent across the full app.
- Paragraph text and other multi-line explanatory copy should be constrained for readability. Target a readable measure of roughly 50 to 60 characters per line, and avoid letting paragraph-style text run wider than that unless there is a strong, explicit reason.
- When implementing this in UI, prefer text containers sized in `ch` units, such as `max-width: 60ch`, rather than relying only on page-level width constraints.

# SOURCE Codebase Modularity Rules

- No source file should exceed 350 lines of code. Treat 350 lines as a hard cap, not a suggestion.
- If a feature starts pushing a file toward that limit, stop and split it into focused modules before adding more logic.
- Large features must be organized as module directories with small sibling files, not as one monolithic file with internal sections.
- Prefer separation by responsibility, such as `types`, `service`, `queries`, `capture`, `indexing`, or `ui sections`, rather than splitting arbitrarily by line count alone.
- Do not introduce new files above the limit, and do not leave existing oversized files in place after touching them substantially. Refactor them as part of the work.
- When editing an oversized legacy file, reduce its size during the same task unless there is a documented blocker.
- Keep shared entry points thin. Files such as `lib.rs`, large React screens, or top-level feature modules should delegate to smaller modules instead of containing full implementations.
- Avoid “temporary” mega-files. If code is too complex to modularize quickly, that is a sign to simplify the design before continuing.
- Run the repo file-length audit when doing structural work so oversized files are visible immediately instead of being discovered later.
