import type { ReactNode } from "react";
import { ScrollArea } from "radix-ui";

/** Bounded reading area with the same quiet scrollbar as the page. */
export function ContainedScrollArea({ label, children }: { label: string; children: ReactNode }) {
  return (
    <ScrollArea.Root type="auto" className="app-scroll-shell relative overflow-hidden">
      <ScrollArea.Viewport className="contained-scroll-viewport w-full pr-4 outline-none focus-visible:ring-1 focus-visible:ring-ring"
        style={{ maxHeight: "min(52vh, 32rem)" }} tabIndex={0} aria-label={label}>
        {children}
      </ScrollArea.Viewport>
      <ScrollArea.Scrollbar className="app-scroll-track" orientation="vertical">
        <ScrollArea.Thumb className="app-scroll-thumb" />
      </ScrollArea.Scrollbar>
    </ScrollArea.Root>
  );
}
