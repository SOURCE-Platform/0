import type { ReactNode } from "react";
import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";
import { PageHeader } from "@/components/ui/page";
import { LoadingState } from "@/components/ui/loading-state";
import type { SettingsTab } from "./types";

const TABS = [
  { value: "general", label: "General" }, { value: "capture", label: "Capture" },
  { value: "privacy", label: "Privacy" }, { value: "storage", label: "Storage" },
  { value: "hardware", label: "Hardware" }, { value: "dictation", label: "Dictation" },
  { value: "mobile", label: "Mobile" },
];

export function SettingsLayout({ loading, saving, showDescriptions, tab, onTabChange, children }: {
  loading: boolean; saving: boolean; showDescriptions: boolean; tab: SettingsTab;
  onTabChange: (tab: SettingsTab) => void; children?: ReactNode;
}) {
  return (
    <div className="settings-page space-y-6" aria-busy={loading}>
      <PageHeader title="Settings" description={showDescriptions ? "Choose what SOURCE captures and how it runs on your devices." : undefined}
        action={!loading && <span role="status" className="text-xs text-muted-foreground">{saving ? "Saving…" : "Changes save automatically"}</span>} />
      {loading ? <LoadingState label="Loading settings…" /> : <>
        <div className="settings-nav">
          <AnimatedTabNav tabs={TABS} value={tab} onValueChange={(value) => onTabChange(value as SettingsTab)} />
        </div>
        <div className="settings-body" data-section={tab}>{children}</div>
      </>}
    </div>
  );
}
