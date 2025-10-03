# 2025-10-03 - Theme Switching Implementation

**Problem:** The Observer app needed a working light/dark theme toggle system. Initial implementation had multiple issues: theme wouldn't switch visually despite state changes, text remained black in dark mode, and there was a mismatch between Tailwind v3 config and Tailwind v4 CSS.

**Root Cause:**
1. Initial CSS used `@media (prefers-color-scheme: dark)` which responded to macOS system preference instead of the `.dark` class toggle
2. Tailwind v4's `@theme` directive doesn't support being nested inside selectors like `.dark { @theme { ... } }`
3. Old `tailwind.config.js` (v3 format) was conflicting with Tailwind v4's CSS-based configuration
4. CSS variables were defined in wrong format - tried RGB format when HSL format was expected
5. UI components (headings, buttons) lacked explicit `text-foreground` classes and defaulted to black

**Solution:**
1. Removed old `tailwind.config.js` to eliminate v3/v4 conflicts
2. Implemented proper Tailwind v4 CSS structure with `@layer base` for CSS variables
3. Created two-layer theming system:
   - CSS variables in `:root` and `.dark` classes that define color values
   - `@theme` directive that maps these variables to Tailwind's color system
4. Used HSL color format matching Shadcn UI conventions
5. Added explicit `text-foreground` classes to all headings and outline button variants

**Files Modified:**
- `/src/index.css` - Complete rewrite with proper Tailwind v4 theme structure
- `/src/App.tsx` - Added `text-foreground` to Observer heading
- `/src/components/ConsentManager.tsx` - Added `text-foreground` to main heading
- `/src/components/Settings.tsx` - Added `text-foreground` to main heading
- `/src/components/ui/button.tsx` - Added `text-foreground` to outline variant
- `/tailwind.config.js` - Removed (conflicted with v4)
- `/postcss.config.js` - Already properly configured for Tailwind v4

**Outcome:** Theme toggle now works correctly - clicking the moon/sun icon successfully switches between light and dark modes with all text properly visible in both themes. The implementation uses Tailwind v4's native CSS-based configuration for a clean, maintainable theming system.
