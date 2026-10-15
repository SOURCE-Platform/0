import { DropdownMenu } from "radix-ui";
import { UserRound } from "lucide-react";
import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";

export type AppView = "timeline" | "agents" | "vault" | "settings" | "id";

const TABS = [
  { value: "timeline", label: "Timeline" },
  { value: "agents", label: "Agents" },
  { value: "vault", label: "Vault" },
  { value: "settings", label: "Settings" },
];

export function AppHeader({ top, view, onNavigate }: {
  top: number;
  view: AppView;
  onNavigate: (view: AppView) => void;
}) {
  return (
    <header className="app-header fixed inset-x-0 z-50 flex h-14 items-center gap-6 bg-transparent" style={{ top }}>
      <svg width="24" height="24" viewBox="0 0 28 28" className="shrink-0 fill-foreground" aria-label="SOURCE">
        <circle cx="14" cy="14" r="14" />
      </svg>
      <AnimatedTabNav tabs={TABS} value={view} onValueChange={(value) => onNavigate(value as AppView)} />
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button type="button" aria-label="User profile: A" className="profile-trigger ml-auto flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-neutral-600 text-white outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2">
            <span className="select-none text-[9px] font-medium leading-none">A</span>
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content align="end" sideOffset={12} className="z-[100] min-w-40 rounded-lg border bg-popover p-1 text-sm text-popover-foreground shadow-lg">
            <DropdownMenu.Item onSelect={() => onNavigate("id")} className="flex cursor-pointer items-center gap-2 rounded-md px-3 py-2 outline-none focus:bg-accent">
              <UserRound className="h-4 w-4" /> Profile
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </header>
  );
}
