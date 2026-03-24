import { useState } from 'react';
import { cn } from '@/lib/utils';
import { mockActivities, ActivityType, ActivitySegment } from '@/data/mockTimeline';
import { ChevronRight, Play, Sparkles } from 'lucide-react';
import { Button } from '@/components/ui/button';

// ── Constants ──────────────────────────────────────────────────────────────
const PX_PER_HOUR = 64;
const PX_PER_MIN  = PX_PER_HOUR / 60;
const DAY_PX      = PX_PER_HOUR * 24;
const AXIS_X      = 72;
const TRACK_W     = 160; // width of each track column
const TRACK_GAP   = 8;   // gap between digital and sensor columns
const DIGITAL_X   = AXIS_X + 8;
const SENSOR_X    = DIGITAL_X + TRACK_W + TRACK_GAP;

const DAYS: Date[] = Array.from({ length: 7 }, (_, i) => new Date(2026, 2, 23 - i));
const TODAY = DAYS[0];

type ChunkMode = 'calendar' | 'clock-24h' | 'day-night' | 'sleep-awake';

const CHUNK_MODES: { value: ChunkMode; label: string }[] = [
  { value: 'calendar',     label: 'Calendar day' },
  { value: 'clock-24h',   label: '24h clock' },
  { value: 'day-night',   label: 'Day / Night' },
  { value: 'sleep-awake', label: 'Sleep / Awake' },
];

const NIGHT_BANDS = [
  { start: 0,  end: 6,  cls: 'bg-indigo-950/40' },
  { start: 6,  end: 8,  cls: 'bg-indigo-950/15' },
  { start: 20, end: 22, cls: 'bg-indigo-950/15' },
  { start: 22, end: 24, cls: 'bg-indigo-950/40' },
];

// ── Visual styles ──────────────────────────────────────────────────────────
const BG_STYLES: Partial<Record<ActivityType, string>> = {
  sleep: 'bg-slate-800/50 dark:bg-slate-900/60',
  away:  'border border-dashed border-border/50 bg-transparent',
};

const FG_STYLES: Partial<Record<ActivityType, string>> = {
  desktop:  'bg-blue-500/90',
  phone:    'bg-emerald-500/90',
  tablet:   'bg-violet-500/90',
  physical: 'bg-amber-500/90',
  sensor:   'bg-orange-400/80',
};

const LABEL_COLOR: Partial<Record<ActivityType, string>> = {
  desktop:  'text-white',
  phone:    'text-white',
  tablet:   'text-white',
  physical: 'text-white',
  sensor:   'text-white',
  sleep:    'text-slate-400 dark:text-slate-500',
  away:     'text-muted-foreground',
};

const TYPE_DISPLAY: Record<ActivityType, string> = {
  desktop: 'Desktop', phone: 'Phone', tablet: 'Tablet',
  physical: 'Physical', sleep: 'Sleep', away: 'Away', sensor: 'Space',
};

const LEGEND_DOT: Record<ActivityType, string> = {
  desktop:  'bg-blue-500',
  phone:    'bg-emerald-500',
  tablet:   'bg-violet-500',
  physical: 'bg-amber-500',
  sensor:   'bg-orange-400',
  sleep:    'bg-slate-600',
  away:     'border border-dashed border-border',
};

// ── Helpers ────────────────────────────────────────────────────────────────
function dayStart(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

function activitiesForDay(day: Date): { digital: ActivitySegment[]; sensor: ActivitySegment[] } {
  const s = dayStart(day);
  const e = s + 24 * 60 * 60 * 1000;
  const clipped = mockActivities
    .filter(a => a.start < e && a.end > s)
    .map(a => ({ ...a, start: Math.max(a.start, s), end: Math.min(a.end, e) }))
    .sort((a, b) => a.start - b.start);
  return {
    digital: clipped.filter(a => a.track !== 'sensor' && a.type !== 'physical'),
    sensor:  clipped.filter(a => a.track === 'sensor' || a.type === 'physical'),
  };
}

function toY(timestamp: number, refStart: number, pxPerMin: number): number {
  return ((timestamp - refStart) / 60000) * pxPerMin;
}

function hourLabel(h: number, mode: ChunkMode): string {
  if (mode === 'clock-24h') return `${String(h).padStart(2, '0')}:00`;
  if (h === 0) return '12am';
  if (h < 12)  return `${h}am`;
  if (h === 12) return '12pm';
  return `${h - 12}pm`;
}

function formatTime(ms: number): string {
  const d = new Date(ms);
  const h = d.getHours(), m = d.getMinutes();
  const ampm = h >= 12 ? 'PM' : 'AM';
  const hour = h % 12 || 12;
  return m === 0
    ? `${hour} ${ampm}`
    : `${hour}:${String(m).padStart(2, '0')} ${ampm}`;
}

function durationLabel(seg: ActivitySegment): string {
  const min = Math.round((seg.end - seg.start) / 60000);
  if (min < 60) return `${min}m`;
  const h = Math.floor(min / 60), m = min % 60;
  return m === 0 ? `${h}h` : `${h}h ${m}m`;
}

function isBackground(type: ActivityType) {
  return type === 'sleep' || type === 'away';
}

function hasChildren(seg: ActivitySegment) {
  return !!(seg.children && seg.children.length > 0);
}

// Generate detail view tick timestamps
function getDetailTicks(seg: ActivitySegment): number[] {
  const durationMin = (seg.end - seg.start) / 60000;
  const intervalMin = durationMin <= 30 ? 5 : durationMin <= 120 ? 15 : durationMin <= 360 ? 30 : 60;
  const ticks: number[] = [];
  for (let t = seg.start; t <= seg.end; t += intervalMin * 60000) ticks.push(t);
  return ticks;
}

// Scale for detail view: target ~600px min, 5px/min floor
function getDetailPxPerMin(seg: ActivitySegment): number {
  const durationMin = (seg.end - seg.start) / 60000;
  return Math.max(5, 600 / durationMin);
}

// ── Activity block (shared by week view and detail view) ───────────────────
function ActivityBlock({
  seg, top, height, left, width, onClick, className,
}: {
  seg: ActivitySegment;
  top: number;
  height: number;
  left: number;
  width: number;
  onClick?: () => void;
  className?: string;
}) {
  const isSensor = seg.track === 'sensor';
  // Sensor-track items are always solid blocks (cameras run 24/7, including during sleep)
  const isBg     = isBackground(seg.type) && !isSensor;
  const hasKids  = hasChildren(seg);

  // Sensor sleep = bedroom camera → distinct indigo tint
  const style = isBg
    ? BG_STYLES[seg.type]
    : (isSensor && seg.type === 'sleep')
      ? 'bg-indigo-400/70'
      : FG_STYLES[seg.type];

  const labelColor = (isSensor && seg.type === 'sleep') ? 'text-white' : LABEL_COLOR[seg.type];

  return (
    <div
      className={cn(
        'absolute overflow-hidden transition-opacity cursor-pointer hover:opacity-80',
        style,
        className,
      )}
      style={{ top, height, left, width }}
      onClick={onClick}
      title={`${TYPE_DISPLAY[seg.type]}${seg.label ? ` — ${seg.label}` : ''}`}
    >
      {height >= 24 && (
        <div className={cn('flex items-center justify-between px-3 py-2', labelColor)}>
          <p className="text-[11px] leading-tight truncate">
            <span className="font-medium">{seg.label ?? TYPE_DISPLAY[seg.type]}</span>
          </p>
          {hasKids && height >= 20 && (
            <ChevronRight className="h-3 w-3 shrink-0 opacity-60" />
          )}
        </div>
      )}
    </div>
  );
}

// ── Leaf (recording) view ──────────────────────────────────────────────────
function RecordingView({ seg }: { seg: ActivitySegment }) {
  return (
    <div className="max-w-md mx-auto py-12 space-y-6">
      <div className="space-y-1">
        <div className={cn(
          'inline-flex items-center gap-2 px-3 py-1.5 rounded-full text-sm font-medium text-white mb-3',
          FG_STYLES[seg.type],
        )}>
          {TYPE_DISPLAY[seg.type]}
        </div>
        <h2 className="text-2xl font-semibold">{seg.label}</h2>
        <p className="text-sm text-muted-foreground">
          {formatTime(seg.start)} – {formatTime(seg.end)} · {durationLabel(seg)}
        </p>
      </div>

      {/* Placeholder video area */}
      <div className="w-full aspect-video bg-muted rounded-lg flex items-center justify-center border border-border/50">
        <div className="text-center space-y-2">
          <div className="w-12 h-12 rounded-full bg-foreground/10 flex items-center justify-center mx-auto">
            <Play className="h-5 w-5 text-foreground/60 ml-0.5" />
          </div>
          <p className="text-xs text-muted-foreground">Recording available</p>
        </div>
      </div>

      <div className="flex gap-3">
        <Button className="flex-1 gap-2">
          <Play className="h-4 w-4" />
          Play Recording
        </Button>
        <Button variant="outline" className="flex-1 gap-2">
          <Sparkles className="h-4 w-4" />
          AI Overview
        </Button>
      </div>
    </div>
  );
}

// ── Detail timeline (drill-down view) ─────────────────────────────────────
function DetailView({
  seg,
  onDrillDown,
}: {
  seg: ActivitySegment;
  onDrillDown: (s: ActivitySegment) => void;
}) {
  const pxPerMin    = getDetailPxPerMin(seg);
  const totalHeight = Math.max(600, ((seg.end - seg.start) / 60000) * pxPerMin);
  const ticks       = getDetailTicks(seg);
  const children    = seg.children ?? [];

  // Physical segments (sensor track or physical type): show parent + children side-by-side.
  // Digital segments: show children in digital column, sensor coverage in physical column.
  const isPhysical  = seg.track === 'sensor' || seg.type === 'physical';

  const sensorInRange = isPhysical ? [] : mockActivities
    .filter(a => a.track === 'sensor' && a.start < seg.end && a.end > seg.start)
    .map(a => ({ ...a, start: Math.max(a.start, seg.start), end: Math.min(a.end, seg.end) }));

  // Color for the parent context block (left container in physical view — right border only)
  const parentBlockStyle = (seg.track === 'sensor' && seg.type === 'sleep')
    ? 'bg-indigo-500/40'
    : seg.type === 'physical'
      ? 'bg-amber-500/30'
      : 'bg-orange-400/30';

  const ticks_ = ticks; // alias to silence closure lint in JSX below

  return (
    <div className="w-full pb-16">

      {/* Column headers */}
      <div className="relative h-5 mb-1">
        {isPhysical ? (
          <span className="absolute text-[11px] text-muted-foreground/70 font-medium"
            style={{ left: DIGITAL_X }}>
            Physical space
          </span>
        ) : (
          <>
            <span className="absolute text-[11px] text-muted-foreground/70 font-medium"
              style={{ left: DIGITAL_X + TRACK_W / 2, transform: 'translateX(-50%)' }}>
              Digital
            </span>
            <span className="absolute text-[11px] text-muted-foreground/70 font-medium"
              style={{ left: SENSOR_X + TRACK_W / 2, transform: 'translateX(-50%)' }}>
              Physical space
            </span>
          </>
        )}
      </div>

      <div className="relative" style={{ height: totalHeight }}>

        {/* Tick lines + labels */}
        {ticks_.map(tick => {
          const y = toY(tick, seg.start, pxPerMin);
          return (
            <div key={tick} className="absolute inset-x-0" style={{ top: y }}>
              <span
                className="absolute text-[11px] leading-none text-muted-foreground/60 select-none"
                style={{ right: `calc(100% - ${AXIS_X - 6}px)`, top: -1 }}
              >
                {formatTime(tick)}
              </span>
              <div className="absolute h-px bg-border/40" style={{ left: AXIS_X, right: 0 }} />
            </div>
          );
        })}

        {/* Vertical axis line */}
        <div className="absolute top-0 bottom-0 w-px bg-border/50" style={{ left: AXIS_X }} />

        {isPhysical ? (
          <>
            {/* Parent context block — left corners rounded, right border only */}
            <div
              className={cn('absolute overflow-hidden border-r border-white/40 rounded-tl-lg rounded-bl-lg', parentBlockStyle)}
              style={{ top: 0, height: totalHeight, left: DIGITAL_X, width: TRACK_W }}
            >
              <p className="px-3 py-2 text-[11px] font-medium text-white/80 select-none">
                {seg.label ?? TYPE_DISPLAY[seg.type]}
              </p>
            </div>

            {/* Children — immediately right of parent, separated by white 1px lines */}
            {children.map((child, idx) => {
              const top    = toY(child.start, seg.start, pxPerMin);
              const height = Math.max(toY(child.end, seg.start, pxPerMin) - top, 3);
              const childX = DIGITAL_X + TRACK_W;
              const isFirst = idx === 0;
              const isLast  = idx === children.length - 1;
              return (
                <div key={child.id}>
                  {!isFirst && (
                    <div className="absolute h-px bg-white/40"
                      style={{ top, left: childX, width: TRACK_W }} />
                  )}
                  <ActivityBlock seg={child}
                    top={top} height={height} left={childX} width={TRACK_W}
                    className={cn(isFirst && 'rounded-tr-lg', isLast && 'rounded-br-lg')}
                    onClick={() => onDrillDown(child)} />
                </div>
              );
            })}
          </>
        ) : (
          <>
            {/* Digital track — children */}
            {children.map(child => {
              const top    = toY(child.start, seg.start, pxPerMin);
              const height = Math.max(toY(child.end, seg.start, pxPerMin) - top, 3);
              return (
                <ActivityBlock key={child.id} seg={child}
                  top={top} height={height} left={DIGITAL_X} width={TRACK_W}
                  onClick={() => onDrillDown(child)} />
              );
            })}

            {/* Physical space track — sensor coverage for this time range */}
            {sensorInRange.map(item => {
              const top    = toY(item.start, seg.start, pxPerMin);
              const height = Math.max(toY(item.end, seg.start, pxPerMin) - top, 3);
              return (
                <ActivityBlock key={item.id} seg={item}
                  top={top} height={height} left={SENSOR_X} width={TRACK_W}
                  onClick={() => onDrillDown(item)} />
              );
            })}
          </>
        )}
      </div>
    </div>
  );
}

// ── Week view (top level) ──────────────────────────────────────────────────
function WeekView({
  mode,
  onDrillDown,
}: {
  mode: ChunkMode;
  onDrillDown: (seg: ActivitySegment) => void;
}) {
  return (
    <div className="space-y-8 pb-16">
      {/* Column headers — shown once above the first day */}
      <div className="relative h-5">
        <span className="absolute text-[11px] text-muted-foreground/70 font-medium"
          style={{ left: DIGITAL_X + TRACK_W / 2, transform: 'translateX(-50%)' }}>
          Digital
        </span>
        <span className="absolute text-[11px] text-muted-foreground/70 font-medium"
          style={{ left: SENSOR_X + TRACK_W / 2, transform: 'translateX(-50%)' }}>
          Physical space
        </span>
      </div>

      {DAYS.map((day, dayIdx) => {
        const ds      = dayStart(day);
        const { digital, sensor } = activitiesForDay(day);
        const isToday = day.getTime() === TODAY.getTime();

        const sleepBands = sensor
          .filter(a => a.type === 'sleep')
          .map(a => ({ start: toY(a.start, ds, PX_PER_MIN), end: toY(a.end, ds, PX_PER_MIN) }));

        return (
          <div key={dayIdx}>
            <div className="mb-2 pl-1">
              <span className={cn('text-sm font-medium', isToday ? 'text-foreground' : 'text-muted-foreground')}>
                {day.toLocaleDateString('en-US', { weekday: 'long', month: 'short', day: 'numeric' })}
                {isToday && <span className="ml-2 text-xs font-normal text-muted-foreground">Today</span>}
              </span>
            </div>

            <div className="relative" style={{ height: DAY_PX }}>
              {/* Chunk mode overlays */}
              {mode === 'day-night' && NIGHT_BANDS.map((band, i) => (
                <div key={i} className={cn('absolute pointer-events-none', band.cls)}
                  style={{ top: band.start * PX_PER_HOUR, height: (band.end - band.start) * PX_PER_HOUR, left: AXIS_X, right: 0 }}
                />
              ))}
              {mode === 'sleep-awake' && sleepBands.map((band, i) => (
                <div key={i} className="absolute pointer-events-none bg-slate-900/30 dark:bg-slate-950/50"
                  style={{ top: band.start, height: band.end - band.start, left: AXIS_X, right: 0 }}
                />
              ))}

              {/* Hour lines + labels */}
              {Array.from({ length: 25 }, (_, h) => (
                <div key={h} className="absolute inset-x-0" style={{ top: h * PX_PER_HOUR }}>
                  {h < 24 && h % 2 === 0 && (
                    <span className="absolute text-[11px] leading-none text-muted-foreground/60 select-none"
                      style={{ right: `calc(100% - ${AXIS_X - 6}px)`, top: -1 }}>
                      {hourLabel(h, mode)}
                    </span>
                  )}
                  <div className={cn('absolute h-px', h % 2 === 0 ? 'bg-border/40' : 'bg-border/20')}
                    style={{ left: AXIS_X, right: 0 }} />
                </div>
              ))}

              {/* Half-hour ticks */}
              {Array.from({ length: 24 }, (_, h) => (
                <div key={h} className="absolute h-px bg-border/10"
                  style={{ top: h * PX_PER_HOUR + PX_PER_HOUR / 2, left: AXIS_X, right: 0 }} />
              ))}

              {/* Vertical axis line */}
              <div className="absolute top-0 bottom-0 w-px bg-border/50" style={{ left: AXIS_X }} />

              {/* Digital track */}
              {digital.map(seg => {
                const top    = toY(seg.start, ds, PX_PER_MIN);
                const height = Math.max(toY(seg.end, ds, PX_PER_MIN) - top, 3);
                return (
                  <ActivityBlock key={seg.id} seg={seg}
                    top={top} height={height} left={DIGITAL_X} width={TRACK_W}
                    onClick={() => onDrillDown(seg)} />
                );
              })}

              {/* Physical space / sensor track */}
              {sensor.map(seg => {
                const top    = toY(seg.start, ds, PX_PER_MIN);
                const height = Math.max(toY(seg.end, ds, PX_PER_MIN) - top, 3);
                return (
                  <ActivityBlock key={seg.id} seg={seg}
                    top={top} height={height} left={SENSOR_X} width={TRACK_W}
                    onClick={() => onDrillDown(seg)} />
                );
              })}
            </div>
          </div>
        );
      })}
    </div>
  );
}

// ── Breadcrumb ─────────────────────────────────────────────────────────────
function Breadcrumb({
  stack,
  onNavigate,
}: {
  stack: ActivitySegment[];
  onNavigate: (level: number) => void;
}) {
  return (
    <div className="flex items-center gap-1 text-sm min-w-0">
      <button onClick={() => onNavigate(0)} className="text-muted-foreground hover:text-foreground transition-colors shrink-0">
        Week
      </button>
      {stack.map((seg, idx) => (
        <span key={seg.id} className="flex items-center gap-1 min-w-0">
          <ChevronRight className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
          <button
            onClick={() => onNavigate(idx + 1)}
            className={cn(
              'transition-colors truncate',
              idx === stack.length - 1
                ? 'text-foreground font-medium'
                : 'text-muted-foreground hover:text-foreground',
            )}
          >
            {seg.label ?? TYPE_DISPLAY[seg.type]}
          </button>
        </span>
      ))}
    </div>
  );
}

// ── Root component ─────────────────────────────────────────────────────────
export default function VerticalTimeline() {
  const [mode, setMode]           = useState<ChunkMode>('calendar');
  const [drillStack, setDrillStack] = useState<ActivitySegment[]>([]);

  function drillDown(seg: ActivitySegment) {
    setDrillStack(prev => [...prev, seg]);
  }

  function navigateTo(level: number) {
    setDrillStack(prev => prev.slice(0, level));
  }

  const inDrill       = drillStack.length > 0;
  const current       = drillStack[drillStack.length - 1];
  const isLeaf        = inDrill && !hasChildren(current);

  return (
    <div className="w-full">
      {/* ── Top bar ───────────────────────────────────────────── */}
      <div className="sticky top-0 z-20 flex items-center justify-between py-3 bg-background gap-4">
        {/* Left: chunk modes OR breadcrumb */}
        {!inDrill ? (
          <div className="flex items-center gap-1">
            {CHUNK_MODES.map(({ value, label }) => (
              <button key={value} onClick={() => setMode(value)}
                className={cn(
                  'px-3 py-1 text-sm rounded-full transition-colors cursor-pointer',
                  mode === value
                    ? 'bg-foreground text-background font-medium'
                    : 'text-muted-foreground hover:text-foreground hover:bg-muted',
                )}>
                {label}
              </button>
            ))}
          </div>
        ) : (
          <Breadcrumb stack={drillStack} onNavigate={navigateTo} />
        )}

        {/* Right: legend (always visible) */}
        <div className="flex items-center gap-4 text-xs text-muted-foreground shrink-0">
          {(Object.keys(TYPE_DISPLAY) as ActivityType[]).map(type => (
            <div key={type} className="flex items-center gap-1.5">
              <div className={cn('w-2.5 h-2.5 rounded-sm', LEGEND_DOT[type])} />
              <span>{TYPE_DISPLAY[type]}</span>
            </div>
          ))}
        </div>
      </div>

      {/* ── Main content ──────────────────────────────────────── */}
      {!inDrill && (
        <WeekView mode={mode} onDrillDown={drillDown} />
      )}
      {inDrill && isLeaf && (
        <RecordingView seg={current} />
      )}
      {inDrill && !isLeaf && (
        <DetailView seg={current} onDrillDown={drillDown} />
      )}
    </div>
  );
}
