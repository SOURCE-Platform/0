import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { BankAccount, TransactionPage, TransactionQuery } from "./types";
import { formatMoney } from "./format";

const selectClass = "h-9 cursor-pointer rounded-md border bg-background px-3 text-sm disabled:cursor-not-allowed";

export function BankTransactions({ page, accounts, query, onFilter, busy }: {
  page: TransactionPage;
  accounts: BankAccount[];
  query: TransactionQuery;
  onFilter: (patch: Partial<TransactionQuery>) => void;
  busy: boolean;
}) {
  const labels = new Map(accounts.map((a) => [a.id, a.label]));
  return (
    <section className="ui-panel space-y-4" aria-label="Bank transactions">
      <h2 className="text-sm font-medium">Transactions</h2>
      <div className="flex flex-wrap items-end gap-3">
        <label className="space-y-1 text-xs text-muted-foreground">
          <span className="block">Account</span>
          <select aria-label="Filter by account" className={selectClass} disabled={busy}
            value={query.accountId ?? ""} onChange={(e) => onFilter({ accountId: e.target.value || undefined })}>
            <option value="">All accounts</option>
            {accounts.map((a) => <option key={a.id} value={a.id}>{a.label} {a.maskedNumber}</option>)}
          </select>
        </label>
        <label className="space-y-1 text-xs text-muted-foreground">
          <span className="block">Direction</span>
          <select aria-label="Filter by direction" className={selectClass} disabled={busy}
            value={query.direction ?? ""} onChange={(e) => onFilter({ direction: e.target.value || undefined })}>
            <option value="">Incoming and outgoing</option>
            <option value="incoming">Incoming</option>
            <option value="outgoing">Outgoing</option>
          </select>
        </label>
        <label className="space-y-1 text-xs text-muted-foreground">
          <span className="block">From</span>
          <Input type="date" aria-label="Transactions from" disabled={busy} value={query.dateFrom ?? ""}
            onChange={(e) => onFilter({ dateFrom: e.target.value || undefined })} />
        </label>
        <label className="space-y-1 text-xs text-muted-foreground">
          <span className="block">To</span>
          <Input type="date" aria-label="Transactions to" disabled={busy} value={query.dateTo ?? ""}
            onChange={(e) => onFilter({ dateTo: e.target.value || undefined })} />
        </label>
        <label className="min-w-48 flex-1 space-y-1 text-xs text-muted-foreground">
          <span className="block">Search descriptions and counterparties</span>
          <Input aria-label="Search bank transactions" maxLength={500} disabled={busy} value={query.search ?? ""}
            onChange={(e) => onFilter({ search: e.target.value || undefined })} />
        </label>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full text-left text-sm">
          <thead className="border-b text-xs text-muted-foreground">
            <tr><th className="py-3 pr-4 font-medium">Date</th><th className="py-3 pr-4 font-medium">Transaction</th>
              <th className="py-3 pr-4 font-medium">Status</th><th className="py-3 text-right font-medium">Amount</th></tr>
          </thead>
          <tbody className="divide-y">
            {page.rows.map((transaction) => (
              <tr key={transaction.id}>
                <td className="whitespace-nowrap py-3 pr-4 text-muted-foreground">{transaction.date ?? "Not supplied"}</td>
                <td className="min-w-48 max-w-md py-3 pr-4">
                  <p className="break-words font-medium">{transaction.counterparty ?? transaction.description}</p>
                  {transaction.counterparty && <p className="mt-1 max-w-[60ch] break-words text-xs text-muted-foreground">{transaction.description}</p>}
                  <p className="mt-1 text-xs text-muted-foreground">{labels.get(transaction.accountId)} · {transaction.revisionCount} bank revision(s)</p>
                  {transaction.identityQuality !== "bank-reference" && <p className="mt-1 text-xs text-amber-700 dark:text-amber-400">
                    No stable bank reference. Matching is provisional.
                  </p>}
                </td>
                <td className="py-3 pr-4 text-xs text-muted-foreground">{transaction.status === "BOOK" ? "Booked" : transaction.status === "PDNG" ? "Pending" : transaction.status}</td>
                <td className={`whitespace-nowrap py-3 text-right tabular-nums ${transaction.direction === "incoming" ? "text-emerald-700 dark:text-emerald-400" : ""}`}>
                  {transaction.direction === "incoming" ? "+" : "−"}{formatMoney({ ...transaction.money, amount: transaction.money.amount.replace(/^-/, "") })}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!page.rows.length && <p className="py-8 text-sm text-muted-foreground">{busy ? "Loading transactions…" : "No transactions match these filters."}</p>}
      </div>
      <div className="flex items-center justify-between gap-3">
        <p className="text-xs text-muted-foreground">{page.total} matching transactions</p>
        <div className="flex gap-2">
          <Button type="button" variant="outline" size="sm" disabled={busy || query.offset === 0}
            onClick={() => onFilter({ offset: Math.max(0, query.offset - query.limit) })}>Previous</Button>
          <Button type="button" variant="outline" size="sm" disabled={busy || query.offset + query.limit >= page.total}
            onClick={() => onFilter({ offset: query.offset + query.limit })}>Next</Button>
        </div>
      </div>
    </section>
  );
}
