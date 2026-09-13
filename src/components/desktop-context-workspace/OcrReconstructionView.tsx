import { EvidenceImage } from "@/components/desktop-context-workspace/EvidenceImage";
import { OcrReconstruction } from "@/types/contextTimeline";
import { parseBoundingBox } from "@/components/desktop-context-workspace/utils";

/// Recognized text sits at roughly this share of its line box's height.
const TEXT_TO_LINE_HEIGHT = 0.8;

export function OcrReconstructionView({ reconstruction }: { reconstruction: OcrReconstruction }) {
  const width = Math.max(reconstruction.width, 1);
  const height = Math.max(reconstruction.height, 1);

  return (
    <div className="space-y-3">
      <div className="text-xs uppercase tracking-wide text-muted-foreground">Reconstructed scene</div>
      <div className="overflow-hidden rounded-2xl border border-border/70 bg-zinc-950/80">
        {/* An inline-size container, so each line's font can be sized in cqw to
            match its real height on screen. Lines used to be fixed 10px text
            with padding inside boxes only a few pixels tall, clipped to an
            empty outline. */}
        <div
          className="relative mx-auto w-full [container-type:inline-size]"
          style={{ aspectRatio: `${width} / ${height}` }}
        >
          <div className="absolute inset-0 bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.08),transparent_50%),linear-gradient(180deg,rgba(255,255,255,0.03),rgba(0,0,0,0.08))]" />
          {reconstruction.framePath && reconstruction.backdropAvailable ? (
            <EvidenceImage
              path={reconstruction.framePath}
              alt="OCR evidence backdrop"
              className="absolute inset-0 h-full w-full object-cover opacity-25 grayscale"
            />
          ) : null}

          {reconstruction.blocks.map((block) => {
            const bbox = parseBoundingBox(block.boundingBox);
            if (!bbox) return null;
            const hasPii = block.piiEntities.length > 0;
            return (
              <div
                key={block.id}
                title={block.text}
                className={`absolute whitespace-nowrap leading-none ${hasPii ? "text-red-200" : "text-zinc-100"}`}
                style={{
                  left: `${(bbox.x / width) * 100}%`,
                  top: `${(bbox.y / height) * 100}%`,
                  fontSize: `${(bbox.height / width) * 100 * TEXT_TO_LINE_HEIGHT}cqw`,
                }}
              >
                {block.text}
              </div>
            );
          })}
        </div>
      </div>

      {reconstruction.blocks.length > 0 ? (
        <div className="space-y-2">
          <div className="text-xs uppercase tracking-wide text-muted-foreground">
            Text on screen · {reconstruction.blocks.length} lines
          </div>
          <div className="max-h-80 space-y-1 overflow-y-auto rounded-2xl border border-border/70 bg-zinc-950/60 p-4 text-sm leading-6 text-foreground select-text">
            {reconstruction.blocks.map((block) => (
              <div key={block.id} className="flex flex-wrap items-baseline gap-2">
                <span>{block.text}</span>
                {block.piiEntities.slice(0, 3).map((entity) => (
                  <span
                    key={entity.id}
                    className="rounded bg-red-500/25 px-1 py-0.5 text-[10px] uppercase tracking-wide text-red-100"
                  >
                    {entity.entityType}
                  </span>
                ))}
              </div>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}
