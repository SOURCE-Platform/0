import { LoaderCircle } from "lucide-react";

export function LoadingState({ label }: { label: string }) {
  return (
    <div role="status" aria-live="polite" className="flex min-h-64 items-center justify-center gap-3 text-sm text-muted-foreground">
      <LoaderCircle aria-hidden="true" className="size-4 animate-spin motion-reduce:animate-none" />
      <span>{label}</span>
    </div>
  );
}
