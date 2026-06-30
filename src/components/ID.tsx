import { useState } from "react";
import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";
import { type UserProfile, MOCK_PROFILES } from "./ID.data";
import { GovDocsSection, LegalDocsSection, BankingSection, RelationshipsSection } from "./id/ProfileSections";
import { HeadScanViewer } from "./id/HeadScanViewer";

function formatDateLong(iso: string): string {
  const [y, m, d] = iso.split("-");
  const months = ["January","February","March","April","May","June","July","August","September","October","November","December"];
  const day = parseInt(d, 10);
  const suffix = [11, 12, 13].includes(day)
    ? "th"
    : day % 10 === 1
      ? "st"
      : day % 10 === 2
        ? "nd"
        : day % 10 === 3
          ? "rd"
          : "th";
  return `${months[parseInt(m, 10) - 1]} ${day}${suffix}, ${y}`;
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
          {user.phones.length > 0 ? user.phones.map((phone) => (
            <p key={phone.id} className="text-sm font-medium">{phone.number}</p>
          )) : (
            <p className="text-sm text-muted-foreground">—</p>
          )}
        </InfoField>

        <InfoField label="Email">
          {user.emails.length > 0 ? user.emails.map((email) => (
            <p key={email.id} className="text-sm font-medium">{email.address}</p>
          )) : (
            <p className="text-sm text-muted-foreground">—</p>
          )}
        </InfoField>
      </div>

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

      {user.relationships.length > 0 && (
        <RelationshipsSection relationships={user.relationships} />
      )}
    </div>
  );
}

const PROFILE_TABS = [
  { value: "info", label: "Info" },
  { value: "gov-ids", label: "Government IDs" },
  { value: "legal", label: "Legal Docs" },
  { value: "banking", label: "Banking" },
];

function ProfileDetail({ user }: { user: UserProfile }) {
  const [activeTab, setActiveTab] = useState("info");

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="text-2xl font-bold">{user.firstName} {user.lastName}</h2>
      </div>

      <AnimatedTabNav
        tabs={PROFILE_TABS}
        value={activeTab}
        onValueChange={setActiveTab}
      />

      {activeTab === "info" ? (
        <div className="grid grid-cols-1 lg:grid-cols-[2fr_3fr] gap-8 items-start">
          <HeadScanViewer />
          <InfoSection user={user} />
        </div>
      ) : (
        <div>
          {activeTab === "gov-ids" && <GovDocsSection docs={user.govDocuments} />}
          {activeTab === "legal" && <LegalDocsSection docs={user.legalDocuments} />}
          {activeTab === "banking" && <BankingSection accounts={user.bankAccounts} />}
        </div>
      )}
    </div>
  );
}

export default function ID() {
  const user = MOCK_PROFILES[0];
  return <ProfileDetail user={user} />;
}
