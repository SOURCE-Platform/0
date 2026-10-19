import type { BankAccount } from "./types";
import { formatMoney, formatTimestamp } from "./format";

const balanceLabels: Record<string, string> = {
  CLBD: "Booked balance", ITAV: "Available balance", CLAV: "Closing available balance",
  ITBD: "Interim booked balance", XPCD: "Expected balance",
};

export function BankAccounts({ accounts }: { accounts: BankAccount[] }) {
  if (!accounts.length) return null;
  return (
    <section aria-label="Bank accounts" className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
      {accounts.map((account) => (
        <article key={account.id} className="ui-panel space-y-3">
          <div>
            <h2 className="text-sm font-medium">{account.label}</h2>
            <p className="mt-1 text-xs text-muted-foreground">
              Bank Millennium · {account.maskedNumber ?? account.currency}
              {!account.active && " · Not syncing"}
            </p>
          </div>
          {!account.balances.length && <p className="text-sm text-muted-foreground">No balance supplied by the bank.</p>}
          {account.balances.map((balance, index) => (
            <div key={`${balance.balanceType}-${balance.money.currency}-${index}`}>
              <p className="text-xs text-muted-foreground">{balanceLabels[balance.balanceType] ?? balance.balanceType}</p>
              <p className="mt-1 text-lg font-medium tabular-nums">{formatMoney(balance.money)}</p>
              <p className="mt-1 text-xs text-muted-foreground">Observed {formatTimestamp(balance.observedAt)}</p>
            </div>
          ))}
          <p className="max-w-[60ch] text-xs leading-5 text-muted-foreground">
            Earliest imported booking: {account.earliestBookedDate ?? "Not yet known"}.
            This is not proof of complete history.
          </p>
        </article>
      ))}
    </section>
  );
}
