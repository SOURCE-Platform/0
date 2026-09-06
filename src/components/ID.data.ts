// TODO: invoke("get_user_profiles") — replace mock data with Rust backend call
// Future: src-tauri/src/core/identity.rs + banking.rs + SQLite migrations

export interface Transaction {
  id: string;
  date: string;        // ISO date string
  description: string;
  amount: number;      // absolute value
  direction: "incoming" | "outgoing";
  category: string;
  source: string;      // account label
}

export interface BankAccount {
  id: string;
  source: string;      // institution name e.g. "Chase"
  label: string;       // account label e.g. "Checking"
  balance: number;
  currency: string;
  transactions: Transaction[];
}

export interface BankFilters {
  direction: "all" | "incoming" | "outgoing";
  category: string;    // "all" or specific category
  source: string;      // "all" or specific source
}

export interface Address {
  street: string;
  city: string;
  state: string;
  zip: string;
  country: string;
}

export interface Phone {
  id: string;
  label: string;       // "Mobile", "Home", "Work"
  number: string;
}

export interface Email {
  id: string;
  label: string;       // "Personal", "Work", "Freelance"
  address: string;
}

export interface GovDocument {
  id: string;
  type: string;        // e.g. "Passport", "Driver's License"
  status: "valid" | "expiring_soon" | "expired" | "pending";
  issueDate?: string;
  expiryDate?: string;
  issuingAuthority?: string;
  documentNumber?: string; // stored masked
}

export interface LegalDocument {
  id: string;
  type: string;        // e.g. "Employment Contract", "NDA"
  title: string;
  status: "active" | "expired" | "pending" | "terminated";
  counterparty?: string;
  startDate?: string;
  endDate?: string;
  fileRef?: string;
}

export interface Relationship {
  id: string;
  name: string;
  relation: string;    // e.g. "Spouse", "Child", "Parent"
  category: "nuclear" | "extended";
  livesWith: boolean;
  profileId?: string;  // links to another UserProfile.id if tracked
}

export interface UserProfile {
  id: string;
  firstName: string;
  lastName: string;
  initials: string;
  avatarColor: string; // CSS color string
  dateOfBirth: string; // ISO date string
  nationality: string;
  height?: string;     // e.g. "5'11"
  weight?: string;     // e.g. "178 lbs"
  address?: Address;
  phones: Phone[];
  emails: Email[];
  relationships: Relationship[];
  govDocuments: GovDocument[];
  legalDocuments: LegalDocument[];
  bankAccounts: BankAccount[];
}

// ─── Profiles ───────────────────────────────────────────────────────────────
// No demo profiles ship with the app. Real identity data (user-added documents,
// accounts, and relationships) will populate this list. The UserProfile types
// and ProfileDetail layout in ID.tsx are kept as the template for that work.
export const MOCK_PROFILES: UserProfile[] = [];
