//! Add-login form (Phase C). The values cross to the helper exactly once
//! via `add_item` (§1.5); local state is cleared immediately after the
//! call returns, success or failure.

import { useState } from "react";
import { vaultAddLogin, vaultErrorMessage } from "@/lib/vault";

export function AddLoginForm({
  onDone,
  onError,
}: {
  onDone: () => void;
  onError: (message: string | null) => void;
}) {
  const [title, setTitle] = useState("");
  const [username, setUsername] = useState("");
  const [host, setHost] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);

  const complete = title.trim() !== "" && password !== "";

  async function submit() {
    if (!complete || busy) return;
    setBusy(true);
    onError(null);
    try {
      await vaultAddLogin(title.trim(), username.trim(), host.trim(), password);
      setTitle("");
      setUsername("");
      setHost("");
      setPassword("");
      onDone();
    } catch (e) {
      setPassword(""); // never keep a rejected secret in the form
      onError(vaultErrorMessage(String(e)));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="rounded-2xl border border-border/70 bg-black/10 p-5">
      <h3 className="text-sm font-medium text-foreground">Add a synthetic login</h3>
      <p className="mt-1 max-w-[60ch] text-xs leading-5 text-muted-foreground">
        Phase C accepts synthetic credentials only. Saving asks macOS to confirm your
        presence (Touch ID or login password).
      </p>
      <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Field label="Title" value={title} onChange={setTitle} placeholder="Example Bank" />
        <Field label="Username" value={username} onChange={setUsername} placeholder="synthetic-user" />
        <Field label="Host" value={host} onChange={setHost} placeholder="example.test" />
        <Field
          label="Password"
          value={password}
          onChange={setPassword}
          placeholder="synthetic-password"
          password
        />
      </div>
      <div className="mt-4 flex items-center gap-3">
        <button
          onClick={submit}
          disabled={!complete || busy}
          className="cursor-pointer rounded-xl bg-white px-4 py-2 text-sm font-medium text-black transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
        >
          {busy ? "Confirming…" : "Save login"}
        </button>
        <button
          onClick={onDone}
          className="cursor-pointer rounded-xl border border-border/70 px-4 py-2 text-sm text-foreground hover:bg-white/5"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

function Field({
  label,
  value,
  onChange,
  placeholder,
  password,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
  password?: boolean;
}) {
  return (
    <label className="block">
      <span className="mb-1 block text-xs text-muted-foreground">{label}</span>
      <input
        type={password ? "password" : "text"}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoComplete="off"
        className="w-full rounded-xl border border-border/70 bg-black/20 px-3 py-2 text-sm text-foreground outline-none placeholder:text-muted-foreground/50 focus:border-white/30"
      />
    </label>
  );
}

export default AddLoginForm;
