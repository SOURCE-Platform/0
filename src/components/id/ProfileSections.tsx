import { useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { type UserProfile, type BankFilters, MOCK_PROFILES } from "../ID.data";

const AVATAR_COLORS = ["#4f6ef7","#22b899","#e07b39","#9c6ef7","#d94f7a","#2eb8b8","#e8a838","#6e8ef7"];

const GOV_STATUS_BADGE: Record<string, { variant: "secondary" | "outline" | "destructive"; className?: string }> = {
  valid: { variant: "secondary" },
  expiring_soon: { variant: "outline", className: "text-amber-500 border-amber-500" },
  expired: { variant: "destructive" },
  pending: { variant: "outline" },
};

const LEGAL_STATUS_BADGE: Record<string, { variant: "secondary" | "outline" | "destructive"; className?: string }> = {
  active: { variant: "secondary" },
  expired: { variant: "destructive" },
  pending: { variant: "outline", className: "text-amber-500 border-amber-500" },
  terminated: { variant: "outline" },
};

function nameToColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i += 1) hash = name.charCodeAt(i) + ((hash << 5) - hash);
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

function getInitials(name: string): string {
  const parts = name.trim().split(/\s+/);
  if (parts.length >= 2) return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
  return name.slice(0, 2).toUpperCase();
}

function sourceMonogram(source: string): string {
  const words = source.trim().split(/\s+/);
  if (words.length >= 2) return (words[0][0] + words[1][0]).toUpperCase();
  return source.slice(0, 2).toUpperCase();
}

function FamilyMemberCard({ relationship }: { relationship: UserProfile["relationships"][number] }) {
  const linked = relationship.profileId ? MOCK_PROFILES.find((profile) => profile.id === relationship.profileId) : null;
  const color = linked ? linked.avatarColor : nameToColor(relationship.name);
  const initials = linked ? linked.initials : getInitials(relationship.name);

  return (
    <div className="flex items-center gap-3 py-2">
      <div className="w-9 h-9 rounded-full shrink-0 flex items-center justify-center" style={{ backgroundColor: color }}>
        <span className="text-[11px] font-semibold text-white select-none leading-none">{initials}</span>
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium leading-tight">{relationship.name}</p>
        <div className="flex items-center gap-2 mt-0.5">
          <span className="text-xs text-muted-foreground">{relationship.relation}</span>
          {relationship.livesWith && (
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
        {members.map((relationship) => <FamilyMemberCard key={relationship.id} relationship={relationship} />)}
      </div>
    </div>
  );
}

export function RelationshipsSection({ relationships }: { relationships: UserProfile["relationships"] }) {
  const nuclear = relationships.filter((relationship) => relationship.category === "nuclear");
  const extended = relationships.filter((relationship) => relationship.category === "extended");

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

export function GovDocsSection({ docs }: { docs: UserProfile["govDocuments"] }) {
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
              {doc.issueDate && <p className="text-[11px] text-muted-foreground">Issued: {doc.issueDate}</p>}
              {doc.expiryDate && <p className="text-[11px] text-muted-foreground">Expires: {doc.expiryDate}</p>}
              {doc.documentNumber && <p className="text-[11px] font-mono text-muted-foreground">{doc.documentNumber}</p>}
            </CardContent>
          </Card>
        );
      })}
    </div>
  );
}

export function LegalDocsSection({ docs }: { docs: UserProfile["legalDocuments"] }) {
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
              {doc.startDate && <p className="text-[11px] text-muted-foreground">{doc.startDate}{doc.endDate ? ` → ${doc.endDate}` : ""}</p>}
              {doc.fileRef && <p className="text-[11px] font-mono text-muted-foreground truncate">{doc.fileRef}</p>}
            </CardContent>
          </Card>
        );
      })}
    </div>
  );
}

export function BankingSection({ accounts }: { accounts: UserProfile["bankAccounts"] }) {
  const [filters, setFilters] = useState<BankFilters>({ direction: "all", category: "all", source: "all" });

  if (accounts.length === 0) {
    return <p className="text-sm text-muted-foreground py-2">No bank accounts recorded.</p>;
  }

  const allTransactions = accounts.flatMap((account) => account.transactions);
  const categories = Array.from(new Set(allTransactions.map((transaction) => transaction.category))).sort();
  const sources = Array.from(new Set(allTransactions.map((transaction) => transaction.source))).sort();
  const totalBalance = accounts.reduce((sum, account) => sum + account.balance, 0);

  const filtered = allTransactions
    .filter((transaction) => filters.direction === "all" || transaction.direction === filters.direction)
    .filter((transaction) => filters.category === "all" || transaction.category === filters.category)
    .filter((transaction) => filters.source === "all" || transaction.source === filters.source)
    .sort((a, b) => b.date.localeCompare(a.date));

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap gap-3">
        <Card className="flex-1 min-w-[140px]">
          <CardContent className="px-3 py-2.5">
            <p className="text-[11px] text-muted-foreground">Total balance</p>
            <p className="text-base font-semibold">${totalBalance.toLocaleString("en-US", { minimumFractionDigits: 2 })}</p>
          </CardContent>
        </Card>
        {accounts.map((account) => (
          <Card key={account.id} className="flex-1 min-w-[140px]">
            <CardContent className="px-3 py-2.5">
              <p className="text-[11px] text-muted-foreground">{account.source} · {account.label}</p>
              <p className="text-sm font-medium">${account.balance.toLocaleString("en-US", { minimumFractionDigits: 2 })}</p>
            </CardContent>
          </Card>
        ))}
      </div>

      <div className="flex flex-wrap gap-2">
        <Select value={filters.direction} onValueChange={(value) => setFilters((current) => ({ ...current, direction: value as BankFilters["direction"] }))}>
          <SelectTrigger className="h-8 text-xs w-36"><SelectValue placeholder="Direction" /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All directions</SelectItem>
            <SelectItem value="incoming">Incoming</SelectItem>
            <SelectItem value="outgoing">Outgoing</SelectItem>
          </SelectContent>
        </Select>
        <Select value={filters.category} onValueChange={(value) => setFilters((current) => ({ ...current, category: value }))}>
          <SelectTrigger className="h-8 text-xs w-36"><SelectValue placeholder="Category" /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All categories</SelectItem>
            {categories.map((category) => <SelectItem key={category} value={category}>{category}</SelectItem>)}
          </SelectContent>
        </Select>
        <Select value={filters.source} onValueChange={(value) => setFilters((current) => ({ ...current, source: value }))}>
          <SelectTrigger className="h-8 text-xs w-36"><SelectValue placeholder="Source" /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All sources</SelectItem>
            {sources.map((source) => <SelectItem key={source} value={source}>{source}</SelectItem>)}
          </SelectContent>
        </Select>
      </div>

      <ScrollArea className="h-64 rounded-md border">
        <div className="divide-y">
          {filtered.length === 0 ? (
            <p className="text-sm text-muted-foreground px-4 py-6 text-center">
              No transactions match the current filters.
            </p>
          ) : (
            filtered.map((transaction) => (
              <div key={transaction.id} className="flex items-center gap-3 px-4 py-2.5">
                <div className="w-7 h-7 rounded-full bg-muted flex items-center justify-center shrink-0">
                  <span className="text-[9px] font-bold text-muted-foreground leading-none">
                    {sourceMonogram(transaction.source)}
                  </span>
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-xs truncate">{transaction.description}</p>
                  <p className="text-[10px] text-muted-foreground">{transaction.date} · {transaction.category}</p>
                </div>
                <span className={cn("text-sm font-medium shrink-0", transaction.direction === "incoming" ? "text-emerald-500" : "text-foreground")}>
                  {transaction.direction === "incoming" ? "+" : "−"}${transaction.amount.toLocaleString("en-US", { minimumFractionDigits: 2 })}
                </span>
              </div>
            ))
          )}
        </div>
      </ScrollArea>
    </div>
  );
}
