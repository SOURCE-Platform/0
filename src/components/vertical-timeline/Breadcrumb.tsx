import { ChevronRight } from "lucide-react";
import { ActivitySegment } from "@/data/mockTimeline";
import { TYPE_DISPLAY } from "@/components/vertical-timeline/constants";
import { cn } from "@/lib/utils";

interface BreadcrumbProps {
  stack: ActivitySegment[];
  onNavigate: (level: number) => void;
}

export function Breadcrumb({ stack, onNavigate }: BreadcrumbProps) {
  return (
    <div className="flex min-w-0 items-center gap-1 text-sm">
      <button
        onClick={() => onNavigate(0)}
        className="shrink-0 text-muted-foreground transition-colors hover:text-foreground"
      >
        Week
      </button>
      {stack.map((segment, index) => (
        <span key={segment.id} className="flex min-w-0 items-center gap-1">
          <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          <button
            onClick={() => onNavigate(index + 1)}
            className={cn(
              "truncate transition-colors",
              index === stack.length - 1
                ? "font-medium text-foreground"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {segment.label ?? TYPE_DISPLAY[segment.type]}
          </button>
        </span>
      ))}
    </div>
  );
}
