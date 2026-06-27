import VerticalTimeline from "./VerticalTimeline";
import DesktopContextWorkspace from "./DesktopContextWorkspace";

export const DISPLAY_KEY = "selected-display-id";

export default function TimelinePage({ mockDataMode }: { mockDataMode: boolean }) {
  const rawDisplayId = localStorage.getItem(DISPLAY_KEY);
  const displayId = rawDisplayId ? Number.parseInt(rawDisplayId, 10) : null;

  return (
    <div className="w-full">
      {mockDataMode ? (
        <VerticalTimeline />
      ) : (
        <DesktopContextWorkspace displayId={Number.isNaN(displayId ?? NaN) ? null : displayId} />
      )}
    </div>
  );
}
