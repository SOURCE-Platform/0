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

// ─── Mock Data ──────────────────────────────────────────────────────────────

export const MOCK_PROFILES: UserProfile[] = [
  // ── 1. Alex Rivera ─────────────────────────────────────────────────────────
  {
    id: "user-alex",
    firstName: "Alex",
    lastName: "Rivera",
    initials: "AR",
    avatarColor: "#4f6ef7",
    dateOfBirth: "1988-03-14",
    nationality: "US",
    height: "5'11",
    weight: "178 lbs",
    address: { street: "123 Oak Lane", city: "San Francisco", state: "CA", zip: "94102", country: "USA" },
    phones: [
      { id: "p1", label: "Mobile", number: "+1 (415) 555-0192" },
      { id: "p2", label: "Home",   number: "+1 (415) 555-0110" },
    ],
    emails: [
      { id: "e1", label: "Personal", address: "alex.rivera@gmail.com" },
      { id: "e2", label: "Work",     address: "a.rivera@techcorp.io" },
    ],
    relationships: [
      { id: "rel-1", name: "Jamie Rivera",  relation: "Spouse",   category: "nuclear",   livesWith: true,  profileId: undefined },
      { id: "rel-2", name: "Morgan Rivera", relation: "Child",    category: "nuclear",   livesWith: true,  profileId: "user-morgan" },
      { id: "rel-3", name: "Casey Rivera",  relation: "Child",    category: "nuclear",   livesWith: true,  profileId: undefined },
      { id: "rel-4", name: "Taylor Brooks", relation: "Aunt",     category: "extended",  livesWith: false, profileId: "user-taylor" },
      { id: "rel-5", name: "Dana Rivera",   relation: "Parent",   category: "extended",  livesWith: false, profileId: undefined },
    ],
    govDocuments: [
      { id: "gdoc-1", type: "Passport",          status: "valid",         issueDate: "2020-06-01", expiryDate: "2030-06-01", issuingAuthority: "US State Dept.",  documentNumber: "***-4821" },
      { id: "gdoc-2", type: "Driver's License",  status: "expiring_soon", issueDate: "2019-03-14", expiryDate: "2026-03-14", issuingAuthority: "CA DMV",          documentNumber: "***-7742" },
      { id: "gdoc-3", type: "Social Security",   status: "valid",                                                            issuingAuthority: "SSA",             documentNumber: "***-**-9314" },
      { id: "gdoc-4", type: "Birth Certificate", status: "valid",         issueDate: "1988-03-14",                           issuingAuthority: "CA Dept. of Health", documentNumber: "***-1988-AR" },
    ],
    legalDocuments: [],
    bankAccounts: [
      {
        id: "bank-alex-chase",
        source: "Chase",
        label: "Checking",
        balance: 12480.55,
        currency: "USD",
        transactions: [
          { id: "t1", date: "2026-02-20", description: "Direct Deposit — Employer",   amount: 3200.00, direction: "incoming", category: "Income",    source: "Chase" },
          { id: "t2", date: "2026-02-18", description: "Whole Foods Market",           amount:   87.43, direction: "outgoing", category: "Groceries", source: "Chase" },
          { id: "t3", date: "2026-02-17", description: "Netflix Subscription",         amount:   15.99, direction: "outgoing", category: "Utilities", source: "Chase" },
          { id: "t4", date: "2026-02-15", description: "Venmo Transfer — Taylor B.",  amount:  200.00, direction: "outgoing", category: "Transfer",  source: "Chase" },
          { id: "t5", date: "2026-02-10", description: "ACH Credit — Freelance",      amount:  750.00, direction: "incoming", category: "Income",    source: "Chase" },
        ],
      },
      {
        id: "bank-alex-venmo",
        source: "Venmo",
        label: "Balance",
        balance: 340.00,
        currency: "USD",
        transactions: [
          { id: "t6", date: "2026-02-15", description: "From Chase",              amount: 200.00, direction: "incoming", category: "Transfer", source: "Venmo" },
          { id: "t7", date: "2026-02-14", description: "Split — Jordan K.",        amount:  45.00, direction: "incoming", category: "Transfer", source: "Venmo" },
          { id: "t8", date: "2026-02-12", description: "Dinner — Sam Chen",       amount:  62.50, direction: "outgoing", category: "Food",     source: "Venmo" },
        ],
      },
    ],
  },

  // ── 2. Jordan Kim ──────────────────────────────────────────────────────────
  {
    id: "user-jordan",
    firstName: "Jordan",
    lastName: "Kim",
    initials: "JK",
    avatarColor: "#22b899",
    dateOfBirth: "1992-07-22",
    nationality: "US",
    height: "5'8",
    weight: "155 lbs",
    address: { street: "45 Riverside Dr, Apt 8C", city: "New York", state: "NY", zip: "10025", country: "USA" },
    phones: [
      { id: "p10", label: "Mobile", number: "+1 (212) 555-0347" },
    ],
    emails: [
      { id: "e10", label: "Personal", address: "jordan.kim@icloud.com" },
      { id: "e11", label: "Work",     address: "jkim@acmecorp.com" },
    ],
    relationships: [
      { id: "rel-10", name: "Casey Kim",  relation: "Partner", category: "nuclear",  livesWith: true,  profileId: undefined },
      { id: "rel-11", name: "Sun-Hi Kim", relation: "Parent",  category: "extended", livesWith: false, profileId: undefined },
      { id: "rel-12", name: "Min Kim",    relation: "Sibling", category: "extended", livesWith: false, profileId: undefined },
    ],
    govDocuments: [
      { id: "gdoc-10", type: "REAL ID", status: "valid", issueDate: "2023-01-10", expiryDate: "2028-01-10", issuingAuthority: "NY DMV", documentNumber: "***-5519" },
    ],
    legalDocuments: [
      { id: "ldoc-10", type: "Employment Contract", title: "Software Engineer — Acme Corp", status: "active", counterparty: "Acme Corp", startDate: "2023-03-01", fileRef: "acme-eng-contract.pdf" },
    ],
    bankAccounts: [
      {
        id: "bank-jordan-chase",
        source: "Chase",
        label: "Savings",
        balance: 28950.00,
        currency: "USD",
        transactions: [
          { id: "t20", date: "2026-02-21", description: "Payroll — Acme Corp",    amount: 4100.00, direction: "incoming", category: "Income",    source: "Chase" },
          { id: "t21", date: "2026-02-19", description: "ConEd Electric",          amount:   95.20, direction: "outgoing", category: "Utilities", source: "Chase" },
          { id: "t22", date: "2026-02-16", description: "Trader Joe's",            amount:   54.30, direction: "outgoing", category: "Groceries", source: "Chase" },
          { id: "t23", date: "2026-02-11", description: "Venmo — Alex R.",         amount:   45.00, direction: "outgoing", category: "Transfer",  source: "Chase" },
          { id: "t24", date: "2026-02-05", description: "Interest Credit",         amount:    8.70, direction: "incoming", category: "Income",    source: "Chase" },
        ],
      },
    ],
  },

  // ── 3. Sam Chen ────────────────────────────────────────────────────────────
  {
    id: "user-sam",
    firstName: "Sam",
    lastName: "Chen",
    initials: "SC",
    avatarColor: "#e07b39",
    dateOfBirth: "1995-11-05",
    nationality: "US",
    height: "5'9",
    weight: "160 lbs",
    address: { street: "78 Tech Blvd, Suite 4", city: "Austin", state: "TX", zip: "78701", country: "USA" },
    phones: [
      { id: "p20", label: "Mobile", number: "+1 (512) 555-0281" },
    ],
    emails: [
      { id: "e20", label: "Freelance", address: "sam@chenstudio.dev" },
    ],
    relationships: [],
    govDocuments: [
      { id: "gdoc-20", type: "Passport", status: "expired", issueDate: "2016-08-12", expiryDate: "2026-08-12", issuingAuthority: "US State Dept.", documentNumber: "***-3304" },
    ],
    legalDocuments: [
      { id: "ldoc-20", type: "Tax Filing", title: "2025 Federal Tax Return", status: "pending", counterparty: "IRS", startDate: "2026-01-01", endDate: "2026-04-15", fileRef: "2025-taxes.pdf" },
    ],
    bankAccounts: [
      {
        id: "bank-sam-wise",
        source: "Wise",
        label: "Multi-currency",
        balance: 6720.00,
        currency: "USD",
        transactions: [
          { id: "t30", date: "2026-02-22", description: "Client Payment — EUR",      amount: 1200.00, direction: "incoming", category: "Income",    source: "Wise" },
          { id: "t31", date: "2026-02-20", description: "FX conversion USD→EUR",     amount:  500.00, direction: "outgoing", category: "Transfer",  source: "Wise" },
          { id: "t32", date: "2026-02-18", description: "Freelance — Design work",   amount:  900.00, direction: "incoming", category: "Income",    source: "Wise" },
          { id: "t33", date: "2026-02-14", description: "Venmo — Alex R.",           amount:   62.50, direction: "incoming", category: "Transfer",  source: "Wise" },
          { id: "t34", date: "2026-02-10", description: "Adobe Creative Cloud",      amount:   54.99, direction: "outgoing", category: "Utilities", source: "Wise" },
          { id: "t35", date: "2026-02-02", description: "Platform fee",              amount:   12.00, direction: "outgoing", category: "Fees",      source: "Wise" },
        ],
      },
    ],
  },

  // ── 4. Morgan Patel ────────────────────────────────────────────────────────
  {
    id: "user-morgan",
    firstName: "Morgan",
    lastName: "Patel",
    initials: "MP",
    avatarColor: "#9c6ef7",
    dateOfBirth: "2015-09-18",
    nationality: "US",
    height: "4'2",
    weight: "65 lbs",
    address: { street: "123 Oak Lane", city: "San Francisco", state: "CA", zip: "94102", country: "USA" },
    phones: [],
    emails: [],
    relationships: [
      { id: "rel-20", name: "Alex Rivera",  relation: "Parent", category: "nuclear", livesWith: true, profileId: "user-alex" },
      { id: "rel-21", name: "Jamie Rivera", relation: "Parent", category: "nuclear", livesWith: true, profileId: undefined },
    ],
    govDocuments: [
      { id: "gdoc-30", type: "Birth Certificate", status: "valid", issueDate: "2015-09-20", issuingAuthority: "CA Dept. of Health", documentNumber: "***-2015-MP" },
    ],
    legalDocuments: [],
    bankAccounts: [],
  },

  // ── 5. Taylor Brooks ───────────────────────────────────────────────────────
  {
    id: "user-taylor",
    firstName: "Taylor",
    lastName: "Brooks",
    initials: "TB",
    avatarColor: "#d94f7a",
    dateOfBirth: "1975-04-30",
    nationality: "US",
    height: "5'7",
    weight: "148 lbs",
    address: { street: "890 Maple Ave", city: "Portland", state: "OR", zip: "97201", country: "USA" },
    phones: [
      { id: "p30", label: "Mobile", number: "+1 (503) 555-0415" },
      { id: "p31", label: "Work",   number: "+1 (503) 555-0490" },
    ],
    emails: [
      { id: "e30", label: "Personal", address: "taylor.brooks@outlook.com" },
    ],
    relationships: [
      { id: "rel-30", name: "Alex Rivera", relation: "Nephew/Niece", category: "extended", livesWith: false, profileId: "user-alex" },
    ],
    govDocuments: [],
    legalDocuments: [
      { id: "ldoc-30", type: "NDA",               title: "Non-Disclosure Agreement — Vertex AI", status: "active", counterparty: "Vertex AI Inc.", startDate: "2024-01-15", endDate: "2027-01-15", fileRef: "vertex-nda.pdf" },
      { id: "ldoc-31", type: "Business Contract", title: "Consulting Services Agreement",         status: "active", counterparty: "Horizon LLC",    startDate: "2025-06-01", endDate: "2026-06-01", fileRef: "horizon-consulting.pdf" },
    ],
    bankAccounts: [
      {
        id: "bank-taylor-paypal",
        source: "PayPal",
        label: "Business",
        balance: 4210.00,
        currency: "USD",
        transactions: [
          { id: "t40", date: "2026-02-20", description: "Consulting payment — Horizon", amount: 2500.00, direction: "incoming", category: "Income",   source: "PayPal" },
          { id: "t41", date: "2026-02-17", description: "PayPal withdrawal",            amount: 1000.00, direction: "outgoing", category: "Transfer", source: "PayPal" },
        ],
      },
      {
        id: "bank-taylor-cashapp",
        source: "Cash App",
        label: "Personal",
        balance: 820.00,
        currency: "USD",
        transactions: [
          { id: "t42", date: "2026-02-15", description: "From Alex Rivera",    amount: 200.00, direction: "incoming", category: "Transfer", source: "Cash App" },
          { id: "t43", date: "2026-02-08", description: "Venmo lunch split",   amount:  34.00, direction: "outgoing", category: "Food",     source: "Cash App" },
        ],
      },
    ],
  },
];
