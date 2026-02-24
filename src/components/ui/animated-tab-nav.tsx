import { useState, useRef } from "react";
import type { CSSProperties } from "react";
import { cn } from "@/lib/utils";

interface Tab {
  value: string;
  label: string;
}

interface AnimatedTabNavProps {
  tabs: Tab[];
  value: string;
  onValueChange: (value: string) => void;
  className?: string;
}

export function AnimatedTabNav({ tabs, value, onValueChange, className }: AnimatedTabNavProps) {
  const [tabAnim, setTabAnim] = useState<{ from: string; to: string } | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  function handleClick(newValue: string) {
    if (newValue === value) return;

    setTabAnim({ from: value, to: newValue });
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => setTabAnim(null), 200);

    onValueChange(newValue);
  }

  function getUnderlineStyle(tabValue: string): CSSProperties {
    if (!tabAnim) {
      return {
        transform: value === tabValue ? "scaleX(1)" : "scaleX(0)",
        transformOrigin: "left",
      };
    }

    const fromIdx = tabs.findIndex(t => t.value === tabAnim.from);
    const toIdx   = tabs.findIndex(t => t.value === tabAnim.to);
    const goingRight = toIdx > fromIdx;

    if (tabValue === tabAnim.from) {
      return {
        transformOrigin: goingRight ? "right" : "left",
        animation: "tab-underline-exit 0.2s linear forwards",
      };
    }

    if (tabValue === tabAnim.to) {
      return {
        transformOrigin: goingRight ? "left" : "right",
        animation: "tab-underline-enter 0.2s cubic-bezier(0.0, 0.0, 0.2, 1) forwards",
      };
    }

    return { transform: "scaleX(0)", transformOrigin: "left" };
  }

  return (
    <nav className={cn("flex items-center gap-5", className)} role="tablist">
      {tabs.map(tab => (
        <button
          key={tab.value}
          role="tab"
          aria-selected={value === tab.value}
          onClick={() => handleClick(tab.value)}
          className={cn(
            "relative cursor-pointer text-sm font-normal transition-colors focus-visible:outline-none",
            value === tab.value
              ? "text-foreground"
              : "text-foreground/60 hover:text-foreground"
          )}
        >
          {tab.label}
          <span
            className="absolute -bottom-[5px] inset-x-0 h-px bg-foreground"
            style={getUnderlineStyle(tab.value)}
          />
        </button>
      ))}
    </nav>
  );
}
