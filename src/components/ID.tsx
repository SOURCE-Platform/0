import { useState, useRef, useEffect, useCallback } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";
import { cn } from "@/lib/utils";
import {
  type UserProfile,
  type BankFilters,
  MOCK_PROFILES,
} from "./ID.data";


// ─── HeadScanViewer ───────────────────────────────────────────────────────────

function HeadScanViewer() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const angleRef = useRef(0);
  const autoRef = useRef(true);
  const dirRef = useRef(1);
  const dragRef = useRef<{ active: boolean; lastX: number }>({ active: false, lastX: 0 });
  const rafRef = useRef<number>(0);
  const [isAuto, setIsAuto] = useState(true);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const W = canvas.width;
    const H = canvas.height;
    ctx.clearRect(0, 0, W, H);

    ctx.fillStyle = "#0a0a0a";
    ctx.fillRect(0, 0, W, H);

    const cx = W / 2;
    const cy = H / 2 - 10;
    const angle = angleRef.current;
    const cos = Math.cos((angle * Math.PI) / 180);
    const headH = H * 0.48;
    const headW = W * 0.32;

    // Scan line glow
    const scanY = cy - headH * 0.5 + ((Date.now() % 2400) / 2400) * headH;
    const scanGrad = ctx.createLinearGradient(0, scanY - 6, 0, scanY + 6);
    scanGrad.addColorStop(0, "transparent");
    scanGrad.addColorStop(0.5, "rgba(99,202,183,0.35)");
    scanGrad.addColorStop(1, "transparent");
    ctx.fillStyle = scanGrad;
    ctx.fillRect(cx - headW * 1.1, scanY - 6, headW * 2.2, 12);

    // Horizontal grid lines
    ctx.strokeStyle = "rgba(99,202,183,0.12)";
    ctx.lineWidth = 0.5;
    const lineCount = 18;
    for (let i = 0; i <= lineCount; i++) {
      const t = i / lineCount;
      const y = cy - headH * 0.5 + t * headH;
      const localCos = Math.sin(t * Math.PI);
      const xSpan = headW * Math.abs(cos) * localCos;
      ctx.beginPath();
      ctx.moveTo(cx - xSpan, y);
      ctx.lineTo(cx + xSpan, y);
      ctx.stroke();
    }

    // Vertical scan lines
    const vCount = 10;
    for (let i = 0; i <= vCount; i++) {
      const t = (i / vCount - 0.5) * 2;
      const rawX = t * headW;
      const projX = rawX * cos;
      if (Math.abs(projX) > headW * Math.abs(cos) + 2) continue;
      const x = cx + projX;
      ctx.beginPath();
      ctx.strokeStyle = "rgba(99,202,183,0.10)";
      const ySpan = headH * 0.5 * Math.sqrt(Math.max(0, 1 - t * t));
      ctx.moveTo(x, cy - ySpan);
      ctx.lineTo(x, cy + ySpan);
      ctx.stroke();
    }

    // Head silhouette
    ctx.beginPath();
    ctx.ellipse(cx, cy, headW * Math.abs(cos), headH * 0.5, 0, 0, Math.PI * 2);
    ctx.strokeStyle = "rgba(99,202,183,0.6)";
    ctx.lineWidth = 1.5;
    ctx.stroke();

    // Neck
    const neckW = headW * 0.22 * Math.abs(cos);
    ctx.beginPath();
    ctx.moveTo(cx - neckW, cy + headH * 0.5);
    ctx.lineTo(cx - neckW * 1.5, cy + headH * 0.72);
    ctx.lineTo(cx + neckW * 1.5, cy + headH * 0.72);
    ctx.lineTo(cx + neckW, cy + headH * 0.5);
    ctx.strokeStyle = "rgba(99,202,183,0.4)";
    ctx.lineWidth = 1;
    ctx.stroke();

    // Corner brackets
    const bSize = 14;
    const bx = cx - headW * Math.abs(cos) - 10;
    const by = cy - headH * 0.5 - 10;
    const bw = headW * Math.abs(cos) * 2 + 20;
    const bh = headH + 20;
    ctx.strokeStyle = "rgba(99,202,183,0.5)";
    ctx.lineWidth = 1.5;
    const corners: [number, number, number, number][] = [
      [bx,      by,      1,  1],
      [bx + bw, by,     -1,  1],
      [bx,      by + bh, 1, -1],
      [bx + bw, by + bh,-1, -1],
    ];
    for (const [x, y, dx, dy] of corners) {
      ctx.beginPath();
      ctx.moveTo(x + dx * bSize, y);
      ctx.lineTo(x, y);
      ctx.lineTo(x, y + dy * bSize);
      ctx.stroke();
    }

    // Rotation readout
    ctx.fillStyle = "rgba(99,202,183,0.5)";
    ctx.font = "10px monospace";
    ctx.fillText(`ROT ${angle.toFixed(1)}°`, 10, H - 10);
  }, []);

  useEffect(() => {
    let last = 0;
    function loop(ts: number) {
      const dt = ts - last;
      last = ts;
      if (autoRef.current) {
        angleRef.current += dirRef.current * (dt / 1000) * 45;
        if (angleRef.current > 88)  { angleRef.current = 88;  dirRef.current = -1; }
        if (angleRef.current < -88) { angleRef.current = -88; dirRef.current =  1; }
      }
      draw();
      rafRef.current = requestAnimationFrame(loop);
    }
    rafRef.current = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(rafRef.current);
  }, [draw]);

  function handleMouseDown(e: React.MouseEvent) {
    autoRef.current = false;
    setIsAuto(false);
    dragRef.current = { active: true, lastX: e.clientX };
  }

  function handleMouseMove(e: React.MouseEvent) {
    if (!dragRef.current.active) return;
    const dx = e.clientX - dragRef.current.lastX;
    dragRef.current.lastX = e.clientX;
    angleRef.current = Math.max(-88, Math.min(88, angleRef.current + dx * 0.5));
  }

  function handleMouseUp() {
    dragRef.current.active = false;
  }

  function resumeAuto() {
    autoRef.current = true;
    setIsAuto(true);
  }

  return (
    <div className="relative select-none">
      <canvas
        ref={canvasRef}
        width={480}
        height={560}
        className="w-full rounded-lg cursor-grab active:cursor-grabbing"
        style={{ background: "#0a0a0a" }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
      />
      <p className="text-[10px] text-muted-foreground text-center mt-1.5">
        Click and drag to rotate
      </p>
      {!isAuto && (
        <Button
          variant="ghost"
          size="sm"
          className="absolute top-2 right-2 text-xs h-7 px-2"
          onClick={resumeAuto}
        >
          Resume auto-rotation
        </Button>
      )}
    </div>
  );
}

// ─── RelationshipsSection ─────────────────────────────────────────────────────

const AVATAR_COLORS = ["#4f6ef7","#22b899","#e07b39","#9c6ef7","#d94f7a","#2eb8b8","#e8a838","#6e8ef7"];

function nameToColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = name.charCodeAt(i) + ((hash << 5) - hash);
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

function getInitials(name: string): string {
  const parts = name.trim().split(/\s+/);
  if (parts.length >= 2) return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
  return name.slice(0, 2).toUpperCase();
}

function FamilyMemberCard({ r }: { r: UserProfile["relationships"][number] }) {
  const linked = r.profileId ? MOCK_PROFILES.find((p) => p.id === r.profileId) : null;
  const color   = linked ? linked.avatarColor : nameToColor(r.name);
  const initials = linked ? linked.initials   : getInitials(r.name);

  return (
    <div className="flex items-center gap-3 py-2">
      <div
        className="w-9 h-9 rounded-full shrink-0 flex items-center justify-center"
        style={{ backgroundColor: color }}
      >
        <span className="text-[11px] font-semibold text-white select-none leading-none">
          {initials}
        </span>
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium leading-tight">{r.name}</p>
        <div className="flex items-center gap-2 mt-0.5">
          <span className="text-xs text-muted-foreground">{r.relation}</span>
          {r.livesWith && (
            <Badge variant="secondary" className="text-[10px] px-1.5 py-0">Lives with</Badge>
          )}
        </div>
      </div>
    </div>
  );
}

function FamilyColumn({ label, members }: { label: string; members: UserProfile["relationships"] }) {
  if (members.length === 0) return null;
  return (
    <div>
      <p className="text-xs text-muted-foreground mb-1">{label}</p>
      <div className="divide-y divide-border/50">
        {members.map((r) => <FamilyMemberCard key={r.id} r={r} />)}
      </div>
    </div>
  );
}

function RelationshipsSection({ relationships }: { relationships: UserProfile["relationships"] }) {
  const nuclear  = relationships.filter((r) => r.category === "nuclear");
  const extended = relationships.filter((r) => r.category === "extended");

  if (relationships.length === 0) {
    return <p className="text-sm text-muted-foreground">No relationships recorded.</p>;
  }

  return (
    <div className="grid grid-cols-2 gap-6">
      <FamilyColumn label="Family" members={nuclear} />
      <FamilyColumn label="Extended Family" members={extended} />
    </div>
  );
}

// ─── GovDocsSection ───────────────────────────────────────────────────────────

const GOV_STATUS_BADGE: Record<
  string,
  { variant: "secondary" | "outline" | "destructive"; className?: string }
> = {
  valid:         { variant: "secondary" },
  expiring_soon: { variant: "outline", className: "text-amber-500 border-amber-500" },
  expired:       { variant: "destructive" },
  pending:       { variant: "outline" },
};

function GovDocsSection({ docs }: { docs: UserProfile["govDocuments"] }) {
  if (docs.length === 0) {
    return <p className="text-sm text-muted-foreground py-2">No government documents recorded.</p>;
  }
  return (
    <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
      {docs.map((doc) => {
        const badge = GOV_STATUS_BADGE[doc.status] ?? { variant: "outline" as const };
        return (
          <Card key={doc.id} className="gap-2">
            <CardHeader className="pb-1 pt-3 px-3">
              <div className="flex items-start justify-between gap-2">
                <CardTitle className="text-xs font-medium leading-tight">{doc.type}</CardTitle>
                <Badge variant={badge.variant} className={cn("text-[10px] shrink-0", badge.className)}>
                  {doc.status.replace("_", " ")}
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="px-3 pb-3 space-y-0.5">
              {doc.issuingAuthority && <p className="text-[11px] text-muted-foreground">{doc.issuingAuthority}</p>}
              {doc.issueDate    && <p className="text-[11px] text-muted-foreground">Issued: {doc.issueDate}</p>}
              {doc.expiryDate   && <p className="text-[11px] text-muted-foreground">Expires: {doc.expiryDate}</p>}
              {doc.documentNumber && <p className="text-[11px] font-mono text-muted-foreground">{doc.documentNumber}</p>}
            </CardContent>
          </Card>
        );
      })}
    </div>
  );
}

// ─── LegalDocsSection ─────────────────────────────────────────────────────────

const LEGAL_STATUS_BADGE: Record<
  string,
  { variant: "secondary" | "outline" | "destructive"; className?: string }
> = {
  active:     { variant: "secondary" },
  expired:    { variant: "destructive" },
  pending:    { variant: "outline", className: "text-amber-500 border-amber-500" },
  terminated: { variant: "outline" },
};

function LegalDocsSection({ docs }: { docs: UserProfile["legalDocuments"] }) {
  if (docs.length === 0) {
    return <p className="text-sm text-muted-foreground py-2">No legal documents recorded.</p>;
  }
  return (
    <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
      {docs.map((doc) => {
        const badge = LEGAL_STATUS_BADGE[doc.status] ?? { variant: "outline" as const };
        return (
          <Card key={doc.id} className="gap-2">
            <CardHeader className="pb-1 pt-3 px-3">
              <div className="flex items-start justify-between gap-2">
                <CardTitle className="text-xs font-medium leading-tight">{doc.title}</CardTitle>
                <Badge variant={badge.variant} className={cn("text-[10px] shrink-0", badge.className)}>
                  {doc.status}
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="px-3 pb-3 space-y-0.5">
              <p className="text-[11px] text-muted-foreground">{doc.type}</p>
              {doc.counterparty && <p className="text-[11px] text-muted-foreground">{doc.counterparty}</p>}
              {doc.startDate && (
                <p className="text-[11px] text-muted-foreground">
                  {doc.startDate}{doc.endDate ? ` → ${doc.endDate}` : ""}
                </p>
              )}
              {doc.fileRef && <p className="text-[11px] font-mono text-muted-foreground truncate">{doc.fileRef}</p>}
            </CardContent>
          </Card>
        );
      })}
    </div>
  );
}

// ─── BankingSection ───────────────────────────────────────────────────────────

function sourceMonogram(source: string): string {
  const words = source.trim().split(/\s+/);
  if (words.length >= 2) return (words[0][0] + words[1][0]).toUpperCase();
  return source.slice(0, 2).toUpperCase();
}

function BankingSection({ accounts }: { accounts: UserProfile["bankAccounts"] }) {
  const [filters, setFilters] = useState<BankFilters>({ direction: "all", category: "all", source: "all" });

  if (accounts.length === 0) {
    return <p className="text-sm text-muted-foreground py-2">No bank accounts recorded.</p>;
  }

  const allTransactions = accounts.flatMap((a) => a.transactions);
  const categories = Array.from(new Set(allTransactions.map((t) => t.category))).sort();
  const sources    = Array.from(new Set(allTransactions.map((t) => t.source))).sort();
  const totalBalance = accounts.reduce((sum, a) => sum + a.balance, 0);

  const filtered = allTransactions
    .filter((t) => filters.direction === "all" || t.direction === filters.direction)
    .filter((t) => filters.category  === "all" || t.category  === filters.category)
    .filter((t) => filters.source    === "all" || t.source    === filters.source)
    .sort((a, b) => b.date.localeCompare(a.date));

  return (
    <div className="space-y-4">
      {/* Account summary */}
      <div className="flex flex-wrap gap-3">
        <Card className="flex-1 min-w-[140px]">
          <CardContent className="px-3 py-2.5">
            <p className="text-[11px] text-muted-foreground">Total balance</p>
            <p className="text-base font-semibold">
              ${totalBalance.toLocaleString("en-US", { minimumFractionDigits: 2 })}
            </p>
          </CardContent>
        </Card>
        {accounts.map((a) => (
          <Card key={a.id} className="flex-1 min-w-[140px]">
            <CardContent className="px-3 py-2.5">
              <p className="text-[11px] text-muted-foreground">{a.source} · {a.label}</p>
              <p className="text-sm font-medium">
                ${a.balance.toLocaleString("en-US", { minimumFractionDigits: 2 })}
              </p>
            </CardContent>
          </Card>
        ))}
      </div>

      {/* Filter bar */}
      <div className="flex flex-wrap gap-2">
        <Select value={filters.direction} onValueChange={(v) => setFilters((f) => ({ ...f, direction: v as BankFilters["direction"] }))}>
          <SelectTrigger className="h-8 text-xs w-36"><SelectValue placeholder="Direction" /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All directions</SelectItem>
            <SelectItem value="incoming">Incoming</SelectItem>
            <SelectItem value="outgoing">Outgoing</SelectItem>
          </SelectContent>
        </Select>
        <Select value={filters.category} onValueChange={(v) => setFilters((f) => ({ ...f, category: v }))}>
          <SelectTrigger className="h-8 text-xs w-36"><SelectValue placeholder="Category" /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All categories</SelectItem>
            {categories.map((c) => <SelectItem key={c} value={c}>{c}</SelectItem>)}
          </SelectContent>
        </Select>
        <Select value={filters.source} onValueChange={(v) => setFilters((f) => ({ ...f, source: v }))}>
          <SelectTrigger className="h-8 text-xs w-36"><SelectValue placeholder="Source" /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All sources</SelectItem>
            {sources.map((s) => <SelectItem key={s} value={s}>{s}</SelectItem>)}
          </SelectContent>
        </Select>
      </div>

      {/* Transaction list */}
      <ScrollArea className="h-64 rounded-md border">
        <div className="divide-y">
          {filtered.length === 0 ? (
            <p className="text-sm text-muted-foreground px-4 py-6 text-center">
              No transactions match the current filters.
            </p>
          ) : (
            filtered.map((t) => (
              <div key={t.id} className="flex items-center gap-3 px-4 py-2.5">
                <div className="w-7 h-7 rounded-full bg-muted flex items-center justify-center shrink-0">
                  <span className="text-[9px] font-bold text-muted-foreground leading-none">
                    {sourceMonogram(t.source)}
                  </span>
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-xs truncate">{t.description}</p>
                  <p className="text-[10px] text-muted-foreground">{t.date} · {t.category}</p>
                </div>
                <span className={cn("text-sm font-medium shrink-0", t.direction === "incoming" ? "text-emerald-500" : "text-foreground")}>
                  {t.direction === "incoming" ? "+" : "−"}${t.amount.toLocaleString("en-US", { minimumFractionDigits: 2 })}
                </span>
              </div>
            ))
          )}
        </div>
      </ScrollArea>
    </div>
  );
}

// ─── InfoSection ──────────────────────────────────────────────────────────────

function formatDateLong(iso: string): string {
  const [y, m, d] = iso.split("-");
  const months = ["January","February","March","April","May","June","July","August","September","October","November","December"];
  const day = parseInt(d, 10);
  const sfx = [11, 12, 13].includes(day) ? "th"
    : day % 10 === 1 ? "st"
    : day % 10 === 2 ? "nd"
    : day % 10 === 3 ? "rd" : "th";
  return `${months[parseInt(m, 10) - 1]} ${day}${sfx}, ${y}`;
}

function InfoField({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="text-xs text-muted-foreground mb-0.5">{label}</p>
      {children}
    </div>
  );
}

function InfoSection({ user }: { user: UserProfile }) {
  return (
    <div className="space-y-6">
      {/* Row 1: Address | Phone | Email */}
      <div className="grid grid-cols-3 gap-6">
        <InfoField label="Address">
          {user.address ? (
            <>
              <p className="text-sm font-medium">{user.address.street},</p>
              <p className="text-sm font-medium">{user.address.city} {user.address.state},</p>
              <p className="text-sm font-medium">{user.address.zip}</p>
              <p className="text-sm font-medium">{user.address.country}</p>
            </>
          ) : (
            <p className="text-sm text-muted-foreground">—</p>
          )}
        </InfoField>

        <InfoField label="Phone">
          {user.phones.length > 0 ? user.phones.map((ph) => (
            <p key={ph.id} className="text-sm font-medium">{ph.number}</p>
          )) : (
            <p className="text-sm text-muted-foreground">—</p>
          )}
        </InfoField>

        <InfoField label="Email">
          {user.emails.length > 0 ? user.emails.map((em) => (
            <p key={em.id} className="text-sm font-medium">{em.address}</p>
          )) : (
            <p className="text-sm text-muted-foreground">—</p>
          )}
        </InfoField>
      </div>

      {/* Row 2: DOB | Height | Weight */}
      <div className="grid grid-cols-3 gap-6">
        <InfoField label="DOB">
          <p className="text-sm font-medium">{formatDateLong(user.dateOfBirth)}</p>
        </InfoField>

        <InfoField label="Height">
          <p className="text-sm font-medium">{user.height ?? "—"}</p>
        </InfoField>

        <InfoField label="Weight">
          <p className="text-sm font-medium">{user.weight ?? "—"}</p>
        </InfoField>
      </div>

      {/* Family */}
      {user.relationships.length > 0 && (
        <RelationshipsSection relationships={user.relationships} />
      )}
    </div>
  );
}

// ─── ProfileDetail ────────────────────────────────────────────────────────────

const PROFILE_TABS = [
  { value: "info",    label: "Info" },
  { value: "gov-ids", label: "Government IDs" },
  { value: "legal",   label: "Legal Docs" },
  { value: "banking", label: "Banking" },
];

function ProfileDetail({ user }: { user: UserProfile }) {
  const [activeTab, setActiveTab] = useState("info");

  return (
    <div className="flex flex-col gap-4">
      {/* Name + nationality */}
      <div>
        <h2 className="text-2xl font-bold">{user.firstName} {user.lastName}</h2>
      </div>

      {/* Tab nav directly under name */}
      <AnimatedTabNav
        tabs={PROFILE_TABS}
        value={activeTab}
        onValueChange={setActiveTab}
      />

      {/* Info: canvas left + content right. Other tabs: full width. */}
      {activeTab === "info" ? (
        <div className="grid grid-cols-1 lg:grid-cols-[2fr_3fr] gap-8 items-start">
          <HeadScanViewer />
          <InfoSection user={user} />
        </div>
      ) : (
        <div>
          {activeTab === "gov-ids" && <GovDocsSection docs={user.govDocuments} />}
          {activeTab === "legal"   && <LegalDocsSection docs={user.legalDocuments} />}
          {activeTab === "banking" && <BankingSection accounts={user.bankAccounts} />}
        </div>
      )}
    </div>
  );
}

// ─── ID (default export) ──────────────────────────────────────────────────────

export default function ID() {
  // TODO: invoke("get_active_user") — replace with auto-identified user from room sensors
  const user = MOCK_PROFILES[0];

  return <ProfileDetail user={user} />;
}
