export interface Money {
  amount: string;
  currency: string;
}

export interface BankAccount {
  id: string;
  label: string;
  currency: string;
  maskedNumber: string | null;
  active: boolean;
  balances: { money: Money; balanceType: string; observedAt: string }[];
  earliestBookedDate?: string;
}

export interface BankingOverview {
  supported: boolean;
  configured: boolean;
  connected: boolean;
  consentValidUntil: string | null;
  accounts: BankAccount[];
  transactionCount: number;
  pendingCount: number;
  autoSync: boolean;
  lastSyncedAt: string | null;
  lastError: string | null;
  callbackUrl: string;
}

export interface BankTransaction {
  id: string;
  accountId: string;
  money: Money;
  direction: "incoming" | "outgoing";
  status: string;
  date: string | null;
  description: string;
  counterparty: string | null;
  identityQuality: string;
  revisionCount: number;
}

export interface TransactionQuery {
  accountId?: string;
  direction?: string;
  dateFrom?: string;
  dateTo?: string;
  search?: string;
  offset: number;
  limit: number;
}

export interface TransactionPage {
  rows: BankTransaction[];
  total: number;
}
