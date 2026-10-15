import { ReactNode } from "react";
import { ScrollArea } from "radix-ui";

interface AppScrollRegionProps {
  children: ReactNode;
  contentTop: number;
  fading: boolean;
}

export function AppScrollRegion({ children, contentTop, fading }: AppScrollRegionProps) {
  return (
    // Auto keeps the thumb mounted whenever content overflows. Its brightness
    // follows ordinary DOM hover styling, independently of scroll events.
    <ScrollArea.Root type="auto" asChild>
      <main
        className="app-scroll-shell w-full overflow-hidden"
        style={{ height: `calc(100% - ${contentTop}px)`, marginTop: contentTop }}
      >
        <ScrollArea.Viewport
          className="app-scroll-region h-full w-full outline-none transition-opacity duration-200 focus-visible:ring-1 focus-visible:ring-ring/40"
          tabIndex={0}
          aria-label="Scrollable page content"
          onScroll={({ currentTarget }) => {
            const fadeHeight = Math.min(Math.max(currentTarget.scrollTop, 0), 32);
            currentTarget.style.setProperty("--scroll-fade-height", `${fadeHeight}px`);
          }}
          style={{ opacity: fading ? 0 : 1 }}
        >
          {children}
        </ScrollArea.Viewport>
        <ScrollArea.Scrollbar
          className="app-scroll-track"
          orientation="vertical"
          style={{ right: 3 }}
        >
          <ScrollArea.Thumb className="app-scroll-thumb" />
        </ScrollArea.Scrollbar>
      </main>
    </ScrollArea.Root>
  );
}
