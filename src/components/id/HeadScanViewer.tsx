import { useState, useRef, useEffect, useCallback } from "react";
import { Button } from "@/components/ui/button";

export function HeadScanViewer() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const angleRef = useRef(0);
  const autoRef = useRef(true);
  const dirRef = useRef(1);
  const dragRef = useRef<{ active: boolean; lastX: number }>({ active: false, lastX: 0 });
  const rafRef = useRef<number>(0);
  const [isAuto, setIsAuto] = useState(true);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const W = canvas.width;
    const H = canvas.height;
    ctx.clearRect(0, 0, W, H);
    ctx.fillStyle = "#0a0a0a";
    ctx.fillRect(0, 0, W, H);

    const cx = W / 2;
    const cy = H / 2 - 10;
    const angle = angleRef.current;
    const cos = Math.cos((angle * Math.PI) / 180);
    const headH = H * 0.48;
    const headW = W * 0.32;

    const scanY = cy - headH * 0.5 + ((Date.now() % 2400) / 2400) * headH;
    const scanGrad = ctx.createLinearGradient(0, scanY - 6, 0, scanY + 6);
    scanGrad.addColorStop(0, "transparent");
    scanGrad.addColorStop(0.5, "rgba(99,202,183,0.35)");
    scanGrad.addColorStop(1, "transparent");
    ctx.fillStyle = scanGrad;
    ctx.fillRect(cx - headW * 1.1, scanY - 6, headW * 2.2, 12);

    ctx.strokeStyle = "rgba(99,202,183,0.12)";
    ctx.lineWidth = 0.5;
    for (let i = 0; i <= 18; i += 1) {
      const t = i / 18;
      const y = cy - headH * 0.5 + t * headH;
      const localCos = Math.sin(t * Math.PI);
      const xSpan = headW * Math.abs(cos) * localCos;
      ctx.beginPath();
      ctx.moveTo(cx - xSpan, y);
      ctx.lineTo(cx + xSpan, y);
      ctx.stroke();
    }

    for (let i = 0; i <= 10; i += 1) {
      const t = (i / 10 - 0.5) * 2;
      const rawX = t * headW;
      const projX = rawX * cos;
      if (Math.abs(projX) > headW * Math.abs(cos) + 2) continue;
      const x = cx + projX;
      const ySpan = headH * 0.5 * Math.sqrt(Math.max(0, 1 - t * t));
      ctx.beginPath();
      ctx.strokeStyle = "rgba(99,202,183,0.10)";
      ctx.moveTo(x, cy - ySpan);
      ctx.lineTo(x, cy + ySpan);
      ctx.stroke();
    }

    ctx.beginPath();
    ctx.ellipse(cx, cy, headW * Math.abs(cos), headH * 0.5, 0, 0, Math.PI * 2);
    ctx.strokeStyle = "rgba(99,202,183,0.6)";
    ctx.lineWidth = 1.5;
    ctx.stroke();

    const neckW = headW * 0.22 * Math.abs(cos);
    ctx.beginPath();
    ctx.moveTo(cx - neckW, cy + headH * 0.5);
    ctx.lineTo(cx - neckW * 1.5, cy + headH * 0.72);
    ctx.lineTo(cx + neckW * 1.5, cy + headH * 0.72);
    ctx.lineTo(cx + neckW, cy + headH * 0.5);
    ctx.strokeStyle = "rgba(99,202,183,0.4)";
    ctx.lineWidth = 1;
    ctx.stroke();

    const bracketSize = 14;
    const bx = cx - headW * Math.abs(cos) - 10;
    const by = cy - headH * 0.5 - 10;
    const bw = headW * Math.abs(cos) * 2 + 20;
    const bh = headH + 20;
    ctx.strokeStyle = "rgba(99,202,183,0.5)";
    ctx.lineWidth = 1.5;
    const corners: [number, number, number, number][] = [
      [bx, by, 1, 1],
      [bx + bw, by, -1, 1],
      [bx, by + bh, 1, -1],
      [bx + bw, by + bh, -1, -1],
    ];
    for (const [x, y, dx, dy] of corners) {
      ctx.beginPath();
      ctx.moveTo(x + dx * bracketSize, y);
      ctx.lineTo(x, y);
      ctx.lineTo(x, y + dy * bracketSize);
      ctx.stroke();
    }

    ctx.fillStyle = "rgba(99,202,183,0.5)";
    ctx.font = "10px monospace";
    ctx.fillText(`ROT ${angle.toFixed(1)}°`, 10, H - 10);
  }, []);

  useEffect(() => {
    let last = 0;
    function loop(ts: number) {
      const dt = ts - last;
      last = ts;
      if (autoRef.current) {
        angleRef.current += dirRef.current * (dt / 1000) * 45;
        if (angleRef.current > 88) {
          angleRef.current = 88;
          dirRef.current = -1;
        }
        if (angleRef.current < -88) {
          angleRef.current = -88;
          dirRef.current = 1;
        }
      }
      draw();
      rafRef.current = requestAnimationFrame(loop);
    }
    rafRef.current = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(rafRef.current);
  }, [draw]);

  function handleMouseDown(e: React.MouseEvent) {
    autoRef.current = false;
    setIsAuto(false);
    dragRef.current = { active: true, lastX: e.clientX };
  }

  function handleMouseMove(e: React.MouseEvent) {
    if (!dragRef.current.active) return;
    const dx = e.clientX - dragRef.current.lastX;
    dragRef.current.lastX = e.clientX;
    angleRef.current = Math.max(-88, Math.min(88, angleRef.current + dx * 0.5));
  }

  function handleMouseUp() {
    dragRef.current.active = false;
  }

  function resumeAuto() {
    autoRef.current = true;
    setIsAuto(true);
  }

  return (
    <div className="relative select-none">
      <canvas
        ref={canvasRef}
        width={480}
        height={560}
        className="w-full rounded-lg cursor-grab active:cursor-grabbing"
        style={{ background: "#0a0a0a" }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
      />
      <p className="text-[10px] text-muted-foreground text-center mt-1.5">
        Click and drag to rotate
      </p>
      {!isAuto && (
        <Button
          variant="ghost"
          size="sm"
          className="absolute top-2 right-2 text-xs h-7 px-2"
          onClick={resumeAuto}
        >
          Resume auto-rotation
        </Button>
      )}
    </div>
  );
}
