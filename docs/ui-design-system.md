# SOURCE desktop UI

This pass covers Timeline, Agents, Profile, and Settings. Vault owns its own
interface and is deliberately excluded from the new `.app-ui` foundations.

## Foundations

- `src/styles/theme.css` is the canonical light/dark semantic palette.
  Existing token values are preserved, including those used by Vault.
- `src/styles/ui-foundations.css` defines the non-Vault type scale, control
  heights, panel spacing, focus states, and layout classes.
- `src/styles/app-layout.css` owns the page gutters shared with the header.
- The angled canvas background and scrolling alpha mask remain independent
  of panel colors. Do not replace the mask with a flat color overlay.
- Body copy is 14px with a readable measure of at most 60ch. Page titles are
  24px; section titles are 14px medium. Small annotations are 12px.
- Controls are 36px high, compact actions 32px. Panels use 20px padding,
  12px corners, a quiet border, and no decorative shadow.
- Use the semantic palette instead of adding competing theme declarations.
  Preserve the pointer cursor and visible keyboard focus on all controls.
- `src/styles/interaction-cursors.css` covers native controls, interactive ARIA
  roles, portaled menus, and custom `data-interactive="true"` controls. Labels
  and icons inherit their control's cursor. Disabled states take precedence;
  text fields retain the text cursor.

## Reusable components

`src/components/ui/page.tsx` provides:

- `PageHeader`: title, optional explanation, and optional contextual action.
- `SettingsPanel`: one named group of settings with consistent spacing.
- `SettingRow`: label and explanation opposite a control.
- `Disclosure`: native keyboard-accessible expansion for optional detail;
  children mount only while expanded.

Continue using the shared Button, Input, Switch, and Tabs primitives. Their
non-Vault presentation is scoped in the foundations stylesheet, so Vault's
existing primitive styles remain intact.

## Page decisions

- Primary navigation: Timeline, Agents, Vault, Settings. Profile is reached
  through the avatar; the empty ID page no longer occupies a primary tab.
- Timeline stays full width without a selection. Selecting a block opens
  detail below the timeline, or alongside it on very wide windows.
  Transcripts and their copy action precede technical storage accounting;
  the latter lives in Metadata. Long transcripts use a bounded reading area
  with the copy action kept above it. Overview refreshes preserve loaded text.
  Zoom and pan keep the existing surface mounted while ranges load. One nearby
  overview buffer is retained (half the visible range, capped at 30 minutes per
  side), with reuse limited to 30 seconds. Detailed text still loads on selection.
  Block widths follow captured time rather than percentage minimums. Subpixel
  markers may grow to one CSS pixel only within the gap before the next block;
  inset borders keep tiny blocks from expanding beyond their calculated bounds.
  Tracks are 24px high (previously 64px), with 16px standard blocks. Labels
  align right with a 16px gap to the grid; ruler and tracks share one column layout.
- Agents uses one divided list, consistent provider filters, and batches of
  80 conversations. All conversations remain accessible through Show more.
  No session capture, storage, remote transport, or sending behavior changes.
- Settings keeps its horizontal categories. Reset lives only in General.
- Settings header, category tabs, and cards share one centered 58rem column.
  Initial loading uses an accessible spinner that respects reduced motion.
- Timeline overview includes every capture source: input, focus, running apps,
  system, screen evidence, vision, and attention alongside audio and OCR.
  High-frequency inputs/camera samples use clock-aligned summary buckets,
  bounded to roughly 600 buckets per source for a day. Event count controls
  bar height; it does not claim exact active duration. Raw text and window
  payloads remain outside overview responses and load only for a selection.
- macOS keyboard and mouse listeners are currently placeholders. Capture
  settings mark these sources unavailable and disable their controls. Their
  tracks can represent existing historical records without claiming live capture.
- Capture prioritizes source switches and health. Optional configuration,
  meters, and diagnostics appear inside disclosures.
- Storage shows total/database/recordings/free space once. File paths and
  raw record previews are expandable; deletion retains its existing guards.
- Privacy has one persisted personal-data detection surface. The duplicate
  switches that only modified local component state were removed.
- Mobile prioritizes connection, pairing, and devices. Agent permissions
  and certificate details are available in named disclosures.

## Verification

Run `node scripts/test-timeline-zoom.mjs`,
`node scripts/test-timeline-blocks.mjs`,
`node --experimental-strip-types scripts/test-timeline-range.mjs`,
`node --experimental-strip-types scripts/test-timeline-selection.mjs`,
`npm run build`, `npm run check:file-lengths`, and `git diff --check`.
After building, `node scripts/test-interaction-cursors.mjs` checks production
cursor styles against real shared controls and custom roles in headless WebKit
(test runtime: `npx playwright install webkit`). It does not open SOURCE.
`node scripts/test-settings-layout.mjs` checks the shared Settings column and
loading state. `cargo test --manifest-path src-tauri/Cargo.toml --release --lib overview_tests` verifies overview
source coverage, event aggregation, focus gaps, and on-demand window details.
Review the native app in both themes, check expansion and tab keyboard
navigation, and verify a selected dictation block can still copy its text.
Do not exercise Vault credentials or destructive settings as part of UI QA.
