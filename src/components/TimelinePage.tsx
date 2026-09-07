import DesktopContextWorkspace from "./DesktopContextWorkspace";

export const DISPLAY_KEY = "selected-display-id";

export default function TimelinePage() {
  const rawDisplayId = localStorage.getItem(DISPLAY_KEY);
  const displayId = rawDisplayId ? Number.parseInt(rawDisplayId, 10) : null;

  return (
    <div className="w-full">
      <DesktopContextWorkspace displayId={Number.isNaN(displayId ?? NaN) ? null : displayId} />
    </div>
  );
}
