import { ReactNode, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { cn } from "@/lib/utils";

type EvidenceImageProps = {
  path: string;
  alt: string;
  className?: string;
  /** Shown above the image, only once it has loaded. */
  label?: ReactNode;
  /** Shown instead when the file is gone. */
  fallback?: ReactNode;
};

// OCR screenshots are deleted about 11 seconds after capture, once their text
// is read, so a path in a payload is no promise the file still exists. Keep the
// image invisible until it actually loads and drop it if it fails, instead of
// flashing an empty box with a broken-image icon.
export function EvidenceImage(props: EvidenceImageProps) {
  // Keyed by path so a new block starts over instead of inheriting "missing".
  return <EvidenceImageInner key={props.path} {...props} />;
}

function EvidenceImageInner({ path, alt, className, label, fallback }: EvidenceImageProps) {
  const [state, setState] = useState<"loading" | "loaded" | "missing">("loading");

  if (state === "missing") return <>{fallback ?? null}</>;

  return (
    <>
      {state === "loaded" ? label : null}
      <img
        src={convertFileSrc(path)}
        alt={alt}
        onLoad={() => setState("loaded")}
        onError={() => setState("missing")}
        className={cn(className, "transition-opacity duration-300", state === "loading" && "!opacity-0")}
      />
    </>
  );
}
