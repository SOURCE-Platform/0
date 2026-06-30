import { AlertCircle } from "lucide-react";
import { BlockDetailPanel } from "@/components/desktop-context-workspace/BlockDetailPanel";
import { DeviceContextTimelineSection } from "@/components/desktop-context-workspace/DeviceContextTimelineSection";
import { TimelineContentTabs } from "@/components/desktop-context-workspace/TimelineContentTabs";
import { useDesktopContextWorkspace } from "@/components/desktop-context-workspace/useDesktopContextWorkspace";

export default function DesktopContextWorkspace({ displayId }: { displayId: number | null }) {
  const controller = useDesktopContextWorkspace(displayId);

  return (
    <div className="space-y-6">
      {controller.actionError ? (
        <div className="flex items-start gap-2 rounded-xl border border-red-300 bg-red-50 px-4 py-3 text-sm text-red-800 dark:border-red-900 dark:bg-red-950/60 dark:text-red-200">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          {controller.actionError}
        </div>
      ) : null}

      {controller.status?.warnings?.length ? (
        <div className="rounded-xl border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-900 dark:bg-amber-950/60 dark:text-amber-100">
          {controller.status.warnings.join(" ")}
        </div>
      ) : null}

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
        <div className="space-y-4">
          <DeviceContextTimelineSection controller={controller} />
          <TimelineContentTabs controller={controller} />
        </div>
        <BlockDetailPanel controller={controller} />
      </div>
    </div>
  );
}
