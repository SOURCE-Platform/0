import { Disclosure } from "@/components/ui/page";

export function MobileConnectionDetails({ fingerprint }: { fingerprint: string }) {
  return (
    <Disclosure title="Connection details" description="Verify this Mac’s connection certificate.">
      <p className="break-all font-mono text-xs text-muted-foreground">{fingerprint || "Starting…"}</p>
      <p className="mt-2 text-sm leading-6 text-muted-foreground">Your iPhone remembers this fingerprint when paired and warns if it changes.</p>
    </Disclosure>
  );
}
