/**
 * Webview smoke checks for the CSP / asset-scope hardening work.
 *
 * `reportWebviewReady` runs in every build: it proves the bundle executed
 * and React mounted under the active CSP. `runDevSmokeTour` is dev-only and
 * additionally verifies the asset protocol scope at runtime (vault paths
 * rejected, media paths allowed) and walks the four top-level tabs, which
 * are pure navigation targets and safe to click programmatically.
 *
 * Results go to the Rust log via `webview_smoke_event` and appear as
 * `SMOKE[...]` lines next to `CSP VIOLATION` reports.
 */
import { invoke, convertFileSrc } from "@tauri-apps/api/core";

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

function metrics(label: string) {
  const images = Array.from(document.images);
  return {
    label,
    textLength: document.body?.innerText?.length ?? 0,
    buttons: document.querySelectorAll("button").length,
    tabs: Array.from(document.querySelectorAll('[role="tab"]'))
      .map((t) => t.textContent?.trim())
      .filter(Boolean),
    images: {
      total: images.length,
      loaded: images.filter((i) => i.complete && i.naturalWidth > 0).length,
    },
  };
}

async function report(kind: string, payload: unknown) {
  try {
    await invoke("webview_smoke_event", {
      kind,
      payload: JSON.stringify(payload),
    });
  } catch {
    // Reporting must never break the app it reports on.
  }
}

/** Every build: report once the app has rendered real content. */
export function reportWebviewReady() {
  let attempts = 0;
  const tick = () => {
    attempts += 1;
    const m = metrics("ready");
    if (m.textLength > 100 || attempts >= 20) {
      void report("ready", m);
      return;
    }
    setTimeout(tick, 300);
  };
  setTimeout(tick, 500);
}

function probeImage(path: string): Promise<string> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve("loaded");
    img.onerror = () => resolve("blocked-or-missing");
    img.src = convertFileSrc(path);
    setTimeout(() => resolve("timeout"), 5000);
  });
}

function probeVideo(path: string): Promise<string> {
  return new Promise((resolve) => {
    const video = document.createElement("video");
    // Detached media elements may never start loading; attach hidden.
    video.style.display = "none";
    video.muted = true;
    video.onloadedmetadata = () => {
      video.remove();
      resolve("loaded");
    };
    video.onerror = () => {
      const code = video.error?.code;
      video.remove();
      resolve(code ? `error(code=${code})` : "blocked-or-missing");
    };
    document.body.appendChild(video);
    video.src = convertFileSrc(path);
    video.load();
    setTimeout(() => {
      video.remove();
      resolve("timeout");
    }, 6000);
  });
}

/**
 * Runtime proof of the asset protocol scope. The vault probe file exists on
 * disk and is a valid PNG, so "blocked-or-missing" here means the scope
 * denied it. The media samples must load. Runs wherever the debug command
 * exists (dev server and debug bundles); release builds skip it.
 */
export async function runAssetScopeProbe() {
  try {
    const probe = await invoke<{
      vault_probe: string;
      image_sample: string | null;
      video_sample: string | null;
    }>("debug_asset_scope_probe");

    const result: Record<string, unknown> = {
      vault: await probeImage(probe.vault_probe),
    };
    result.image = probe.image_sample
      ? await probeImage(probe.image_sample)
      : "skipped: no image found";
    result.video = probe.video_sample
      ? await probeVideo(probe.video_sample)
      : "skipped: no video found";
    await report("asset-scope", result);
  } catch {
    await report("asset-scope", "skipped: debug command unavailable");
  }
}

async function tabTour() {
  const tabs = Array.from(
    document.querySelectorAll<HTMLElement>('[role="tab"]'),
  );
  for (const tab of tabs) {
    tab.click();
    await delay(900);
    await report("tab", metrics(tab.textContent?.trim() ?? "unknown"));
  }
  await report("tour-done", metrics("final"));
}

/** Dev server only: a pass over every top-level tab. */
export function runDevSmokeTour() {
  void (async () => {
    await delay(1500);
    await tabTour();
  })();
}
