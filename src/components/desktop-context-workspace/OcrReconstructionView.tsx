import { convertFileSrc } from "@tauri-apps/api/core";
import { OcrReconstruction } from "@/types/contextTimeline";
import { parseBoundingBox } from "@/components/desktop-context-workspace/utils";

export function OcrReconstructionView({ reconstruction }: { reconstruction: OcrReconstruction }) {
  const backdropSrc = reconstruction.framePath ? convertFileSrc(reconstruction.framePath) : null;

  return (
    <div className="space-y-3">
      <div className="text-xs uppercase tracking-wide text-muted-foreground">Reconstructed scene</div>
      <div className="overflow-hidden rounded-2xl border border-border/70 bg-zinc-950/80">
        <div
          className="relative mx-auto w-full"
          style={{ aspectRatio: `${Math.max(reconstruction.width, 1)} / ${Math.max(reconstruction.height, 1)}` }}
        >
          {backdropSrc && reconstruction.backdropAvailable ? (
            <img
              src={backdropSrc}
              alt="OCR evidence backdrop"
              className="absolute inset-0 h-full w-full object-cover opacity-25 grayscale"
            />
          ) : (
            <div className="absolute inset-0 bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.08),transparent_50%),linear-gradient(180deg,rgba(255,255,255,0.03),rgba(0,0,0,0.08))]" />
          )}

          {reconstruction.blocks.map((block) => {
            const bbox = parseBoundingBox(block.boundingBox);
            if (!bbox) return null;
            return (
              <div
                key={block.id}
                className="absolute overflow-hidden rounded-lg border border-amber-300/45 bg-black/30 shadow-[0_0_0_1px_rgba(255,255,255,0.05)] backdrop-blur-[1px]"
                style={{
                  left: `${(bbox.x / reconstruction.width) * 100}%`,
                  top: `${(bbox.y / reconstruction.height) * 100}%`,
                  width: `${(bbox.width / reconstruction.width) * 100}%`,
                  height: `${(bbox.height / reconstruction.height) * 100}%`,
                }}
              >
                <div className="line-clamp-3 px-2 py-1 text-[10px] leading-4 text-white">{block.text}</div>
                {block.piiEntities.length > 0 ? (
                  <div className="absolute inset-x-1 bottom-1 flex flex-wrap gap-1">
                    {block.piiEntities.slice(0, 2).map((entity) => (
                      <span
                        key={entity.id}
                        className="rounded bg-red-500/25 px-1 py-0.5 text-[9px] uppercase tracking-wide text-red-100"
                      >
                        {entity.entityType}
                      </span>
                    ))}
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
