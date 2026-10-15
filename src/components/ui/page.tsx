import { useState, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";

export function PageHeader({ title, description, action }: {
  title: string;
  description?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <header className="page-heading">
      <div className="min-w-0 space-y-1.5">
        <h1>{title}</h1>
        {description && <p className="text-sm leading-6 text-muted-foreground">{description}</p>}
      </div>
      {action && <div className="shrink-0">{action}</div>}
    </header>
  );
}

export function SettingsPanel({ title, description, children, className }: {
  title: string;
  description?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("ui-panel", className)}>
      <header className="space-y-1.5">
        <h2 className="text-sm font-medium text-foreground">{title}</h2>
        {description && <p className="text-sm leading-6 text-muted-foreground">{description}</p>}
      </header>
      <div className="mt-5 space-y-5">{children}</div>
    </section>
  );
}

export function SettingRow({ title, description, htmlFor, children }: {
  title: string;
  description?: ReactNode;
  htmlFor?: string;
  children: ReactNode;
}) {
  return (
    <div className="setting-row">
      <div className="min-w-0 space-y-1">
        <label htmlFor={htmlFor} className="text-sm font-medium">{title}</label>
        {description && <p className="text-sm leading-6 text-muted-foreground">{description}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

/** Keeps optional detail out of the initial layout and mounts it only when opened. */
export function Disclosure({ title, description, children, className }: {
  title: string;
  description?: string;
  children: ReactNode;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <details className={cn("ui-disclosure", className)} open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary className="flex items-center gap-3 list-none">
        <div className="min-w-0 flex-1">
          <span className="text-sm font-medium">{title}</span>
          {description && <p className="mt-1 text-xs leading-5 text-muted-foreground">{description}</p>}
        </div>
        <ChevronDown aria-hidden="true" className="disclosure-chevron h-4 w-4 shrink-0 text-muted-foreground" />
      </summary>
      {open && <div className="mt-4">{children}</div>}
    </details>
  );
}
