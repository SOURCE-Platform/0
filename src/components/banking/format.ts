import type { Money } from "./types";

/** Exact decimal formatting, including amounts beyond Number.MAX_SAFE_INTEGER. */
export function formatMoney(money: Money): string {
  const match = /^(-?)(\d+)(?:\.(\d+))?$/.exec(money.amount);
  if (!match) return `Unavailable ${money.currency}`;
  const [, sign, whole, fraction] = match;
  const grouped = new Intl.NumberFormat("pl-PL").format(BigInt(whole));
  return `${sign}${grouped}${fraction ? `,${fraction}` : ""} ${money.currency}`;
}

export function formatTimestamp(timestamp: string | null): string {
  return timestamp ? new Date(timestamp).toLocaleString() : "Not yet";
}
