export type ActivityType = 'desktop' | 'phone' | 'tablet' | 'physical' | 'sleep' | 'away' | 'sensor';
export type TrackType = 'digital' | 'sensor';

export interface ActivitySegment {
  id: string;
  type: ActivityType;
  track?: TrackType; // defaults to 'digital'
  start: number;
  end: number;
  label?: string;
  children?: ActivitySegment[];
}

function ts(day: number, h: number, m = 0): number {
  return new Date(2026, 2, day, h, m).getTime();
}

export const mockActivities: ActivitySegment[] = [
  { id: 's1',  type: 'sleep',    start: ts(17,  0,  0), end: ts(17,  7, 30), label: 'Sleep' },
  { id: 'a1',  type: 'phone',    start: ts(17,  7, 30), end: ts(17,  8,  0), label: 'Morning check-in' },
  {
    id: 'a2', type: 'desktop', start: ts(17, 8, 0), end: ts(17, 12, 30), label: 'Work',
    children: [
      { id: 'a2-1', type: 'desktop', start: ts(17,  8,  0), end: ts(17,  8, 30), label: 'Email & Slack' },
      { id: 'a2-2', type: 'desktop', start: ts(17,  8, 30), end: ts(17, 10,  0), label: 'Code review' },
      { id: 'a2-3', type: 'desktop', start: ts(17, 10,  0), end: ts(17, 12,  0), label: 'Development' },
      { id: 'a2-4', type: 'desktop', start: ts(17, 12,  0), end: ts(17, 12, 30), label: 'Standup' },
    ],
  },
  {
    id: 'a3', type: 'physical', start: ts(17, 12, 30), end: ts(17, 13, 30), label: 'Lunch out',
    children: [
      { id: 'a3-1', type: 'physical', start: ts(17, 12, 30), end: ts(17, 12, 45), label: 'Walk to restaurant' },
      { id: 'a3-2', type: 'physical', start: ts(17, 12, 45), end: ts(17, 13, 15), label: 'Ate & caught up with coworker' },
      { id: 'a3-3', type: 'physical', start: ts(17, 13, 15), end: ts(17, 13, 30), label: 'Walk back' },
    ],
  },
  {
    id: 'a4', type: 'desktop', start: ts(17, 13, 30), end: ts(17, 17, 30), label: 'Work',
    children: [
      { id: 'a4-1', type: 'desktop', start: ts(17, 13, 30), end: ts(17, 14, 30), label: 'Code review' },
      { id: 'a4-2', type: 'desktop', start: ts(17, 14, 30), end: ts(17, 15,  0), label: 'Team sync call' },
      { id: 'a4-3', type: 'desktop', start: ts(17, 15,  0), end: ts(17, 17,  0), label: 'Feature development' },
      { id: 'a4-4', type: 'desktop', start: ts(17, 17,  0), end: ts(17, 17, 30), label: 'Wrap-up & notes' },
    ],
  },
  { id: 'a5',  type: 'phone',    start: ts(17, 17, 30), end: ts(17, 18,  0), label: 'Messages' },
  {
    id: 'a6', type: 'physical', start: ts(17, 18, 0), end: ts(17, 19, 0), label: 'Gym',
    children: [
      { id: 'a6-1', type: 'physical', start: ts(17, 18,  0), end: ts(17, 18, 10), label: 'Warm-up' },
      { id: 'a6-2', type: 'physical', start: ts(17, 18, 10), end: ts(17, 18, 40), label: 'Upper body' },
      { id: 'a6-3', type: 'physical', start: ts(17, 18, 40), end: ts(17, 18, 55), label: 'Cardio' },
      { id: 'a6-4', type: 'physical', start: ts(17, 18, 55), end: ts(17, 19,  0), label: 'Cool-down' },
    ],
  },
  { id: 'a7',  type: 'phone',    start: ts(17, 19,  0), end: ts(17, 19, 30), label: 'Messages' },
  {
    id: 'a8', type: 'desktop', start: ts(17, 19, 30), end: ts(17, 21, 0), label: 'Personal projects',
    children: [
      {
        id: 'a8-r', type: 'desktop', start: ts(17, 19, 30), end: ts(17, 19, 52), label: 'Research',
        children: [
          { id: 'a8-r-1', type: 'desktop', start: ts(17, 19, 30), end: ts(17, 19, 40), label: 'Perplexity' },
          { id: 'a8-r-2', type: 'desktop', start: ts(17, 19, 40), end: ts(17, 19, 48), label: 'YouTube' },
          { id: 'a8-r-3', type: 'desktop', start: ts(17, 19, 48), end: ts(17, 19, 52), label: 'Gemini' },
        ],
      },
      {
        id: 'a8-d', type: 'desktop', start: ts(17, 19, 52), end: ts(17, 20, 38), label: 'Design',
        children: [
          { id: 'a8-d-1', type: 'desktop', start: ts(17, 19, 52), end: ts(17, 20, 10), label: 'Figma' },
          { id: 'a8-d-2', type: 'desktop', start: ts(17, 20, 10), end: ts(17, 20, 25), label: 'Claude Code' },
          { id: 'a8-d-3', type: 'desktop', start: ts(17, 20, 25), end: ts(17, 20, 38), label: 'Cosmos' },
        ],
      },
      {
        id: 'a8-v', type: 'desktop', start: ts(17, 20, 38), end: ts(17, 21, 0), label: 'DevOps',
        children: [
          { id: 'a8-v-1', type: 'desktop', start: ts(17, 20, 38), end: ts(17, 20, 46), label: 'Cloudflare' },
          { id: 'a8-v-2', type: 'desktop', start: ts(17, 20, 46), end: ts(17, 20, 51), label: 'Digital Ocean' },
          { id: 'a8-v-3', type: 'desktop', start: ts(17, 20, 51), end: ts(17, 20, 56), label: 'Vercel' },
          { id: 'a8-v-4', type: 'desktop', start: ts(17, 20, 56), end: ts(17, 21,  0), label: 'GitHub' },
        ],
      },
    ],
  },
  { id: 'a9',  type: 'tablet',   start: ts(17, 21,  0), end: ts(17, 22, 30), label: 'Streaming' },
  { id: 'a10', type: 'desktop',  start: ts(17, 22, 30), end: ts(18,  1,  0), label: 'Browsing' },

  { id: 's2',  type: 'sleep',    start: ts(18,  1,  0), end: ts(18,  8,  0), label: 'Sleep' },
  { id: 'b1',  type: 'phone',    start: ts(18,  8,  0), end: ts(18,  8, 30), label: 'Morning messages' },
  { id: 'b2',  type: 'desktop',  start: ts(18,  8, 30), end: ts(18, 13,  0), label: 'Work' },
  { id: 'b3',  type: 'physical', start: ts(18, 13,  0), end: ts(18, 14, 30), label: 'Errands' },
  { id: 'b4',  type: 'desktop',  start: ts(18, 14, 30), end: ts(18, 18,  0), label: 'Work' },
  { id: 'b5',  type: 'physical', start: ts(18, 18,  0), end: ts(18, 20,  0), label: 'Dinner with friends' },
  { id: 'b6',  type: 'phone',    start: ts(18, 20,  0), end: ts(18, 21,  0), label: 'Browsing' },
  { id: 'b7',  type: 'tablet',   start: ts(18, 21,  0), end: ts(18, 22,  0), label: 'Streaming' },
  { id: 's3',  type: 'sleep',    start: ts(18, 22,  0), end: ts(19,  7,  0), label: 'Sleep' },

  { id: 'c1',  type: 'phone',    start: ts(19,  7,  0), end: ts(19,  7, 30), label: 'Morning check' },
  { id: 'c2',  type: 'desktop',  start: ts(19,  7, 30), end: ts(19, 12,  0), label: 'Work' },
  { id: 'c3',  type: 'physical', start: ts(19, 12,  0), end: ts(19, 13,  0), label: 'Lunch' },
  { id: 'c4',  type: 'desktop',  start: ts(19, 13,  0), end: ts(19, 17,  0), label: 'Work' },
  { id: 'c5',  type: 'physical', start: ts(19, 17,  0), end: ts(19, 18, 30), label: 'Grocery shopping' },
  { id: 'c6',  type: 'desktop',  start: ts(19, 18, 30), end: ts(19, 20,  0), label: 'Personal' },
  { id: 'c7',  type: 'tablet',   start: ts(19, 20,  0), end: ts(19, 23,  0), label: 'Streaming' },
  { id: 'c8',  type: 'phone',    start: ts(19, 23,  0), end: ts(20,  0, 30), label: 'Late scrolling' },

  { id: 's4',  type: 'sleep',    start: ts(20,  0, 30), end: ts(20,  8, 30), label: 'Sleep' },
  { id: 'd1',  type: 'phone',    start: ts(20,  8, 30), end: ts(20,  9,  0), label: 'Morning messages' },
  { id: 'd2',  type: 'desktop',  start: ts(20,  9,  0), end: ts(20, 13, 30), label: 'Work' },
  { id: 'd3',  type: 'away',     start: ts(20, 13, 30), end: ts(20, 16,  0), label: 'Office / meetings' },
  { id: 'd4',  type: 'desktop',  start: ts(20, 16,  0), end: ts(20, 19,  0), label: 'Work from home' },
  { id: 'd5',  type: 'physical', start: ts(20, 19,  0), end: ts(20, 21,  0), label: 'Social evening' },
  { id: 'd6',  type: 'desktop',  start: ts(20, 21,  0), end: ts(20, 23, 30), label: 'Late work' },
  { id: 'd7',  type: 'phone',    start: ts(20, 23, 30), end: ts(21,  1,  0), label: 'Unwinding' },

  { id: 's5',  type: 'sleep',    start: ts(21,  1,  0), end: ts(21,  9,  0), label: 'Sleep in' },
  { id: 'e1',  type: 'phone',    start: ts(21,  9,  0), end: ts(21, 10,  0), label: 'Slow morning' },
  { id: 'e2',  type: 'desktop',  start: ts(21, 10,  0), end: ts(21, 14,  0), label: 'Light work' },
  { id: 'e3',  type: 'away',     start: ts(21, 14,  0), end: ts(21, 16,  0), label: 'Out & about' },
  { id: 'e4',  type: 'desktop',  start: ts(21, 16,  0), end: ts(21, 18,  0), label: 'Wrapping up' },
  { id: 'e5',  type: 'away',     start: ts(21, 18,  0), end: ts(21, 23,  0), label: 'Friday night out' },
  { id: 'e6',  type: 'phone',    start: ts(21, 23,  0), end: ts(22,  2,  0), label: 'Late night' },

  { id: 's6',  type: 'sleep',    start: ts(22,  2,  0), end: ts(22, 11,  0), label: 'Long sleep' },
  { id: 'f1',  type: 'phone',    start: ts(22, 11,  0), end: ts(22, 12,  0), label: 'Lazy morning' },
  { id: 'f2',  type: 'tablet',   start: ts(22, 12,  0), end: ts(22, 12, 30), label: 'Browsing' },
  { id: 'f3',  type: 'physical', start: ts(22, 12, 30), end: ts(22, 14,  0), label: 'Lunch out' },
  { id: 'f4',  type: 'desktop',  start: ts(22, 14,  0), end: ts(22, 17,  0), label: 'Personal projects' },
  { id: 'f5',  type: 'physical', start: ts(22, 17,  0), end: ts(22, 20,  0), label: 'Evening out' },
  { id: 'f6',  type: 'tablet',   start: ts(22, 20,  0), end: ts(22, 22, 30), label: 'Movie' },
  { id: 'f7',  type: 'desktop',  start: ts(22, 22, 30), end: ts(22, 23, 30), label: 'Browsing' },
  { id: 'f8',  type: 'phone',    start: ts(22, 23, 30), end: ts(23,  0, 30), label: 'Scrolling' },

  { id: 's7',  type: 'sleep',    start: ts(23,  0, 30), end: ts(23,  9,  0), label: 'Sleep' },
  {
    id: 'g1', type: 'phone', start: ts(23, 9, 0), end: ts(23, 10, 30), label: 'Sunday morning',
    children: [
      { id: 'g1-1', type: 'phone', start: ts(23,  9,  0), end: ts(23,  9, 15), label: 'Good morning texts' },
      { id: 'g1-2', type: 'phone', start: ts(23,  9, 15), end: ts(23,  9, 50), label: 'News & socials' },
      { id: 'g1-3', type: 'phone', start: ts(23,  9, 50), end: ts(23, 10, 15), label: 'Podcast' },
      { id: 'g1-4', type: 'phone', start: ts(23, 10, 15), end: ts(23, 10, 30), label: 'Planning the day' },
    ],
  },
  {
    id: 'g2', type: 'physical', start: ts(23, 10, 30), end: ts(23, 12, 0), label: 'Walk outside',
    children: [
      { id: 'g2-1', type: 'physical', start: ts(23, 10, 30), end: ts(23, 10, 48), label: 'Neighborhood streets' },
      { id: 'g2-2', type: 'physical', start: ts(23, 10, 48), end: ts(23, 11, 20), label: 'Park loop' },
      { id: 'g2-3', type: 'physical', start: ts(23, 11, 20), end: ts(23, 11, 45), label: 'Sat by the fountain' },
      { id: 'g2-4', type: 'physical', start: ts(23, 11, 45), end: ts(23, 12,  0), label: 'Walk home' },
    ],
  },
  {
    id: 'g3', type: 'desktop', start: ts(23, 12, 0), end: ts(23, 14, 0), label: 'Browsing',
    children: [
      { id: 'g3-1', type: 'desktop', start: ts(23, 12,  0), end: ts(23, 12, 28), label: 'Reddit' },
      { id: 'g3-2', type: 'desktop', start: ts(23, 12, 28), end: ts(23, 13, 10), label: 'YouTube' },
      { id: 'g3-3', type: 'desktop', start: ts(23, 13, 10), end: ts(23, 13, 38), label: 'News articles' },
      { id: 'g3-4', type: 'desktop', start: ts(23, 13, 38), end: ts(23, 14,  0), label: 'Wikipedia rabbit hole' },
    ],
  },
  {
    id: 'g4', type: 'physical', start: ts(23, 14, 0), end: ts(23, 16, 0), label: 'Family time',
    children: [
      { id: 'g4-1', type: 'physical', start: ts(23, 14,  0), end: ts(23, 14, 32), label: 'Late brunch together' },
      {
        id: 'g4-2', type: 'physical', start: ts(23, 14, 32), end: ts(23, 15, 12), label: 'Family splits up',
        children: [
          {
            id: 'g4-2a', type: 'physical', start: ts(23, 14, 32), end: ts(23, 15, 12), label: 'Mom & Dad — coffee chat',
            children: [
              { id: 'g4-2a-1', type: 'physical', start: ts(23, 14, 32), end: ts(23, 14, 50), label: 'Weekend recap' },
              { id: 'g4-2a-2', type: 'physical', start: ts(23, 14, 50), end: ts(23, 15,  4), label: 'Planning next weekend' },
              { id: 'g4-2a-3', type: 'physical', start: ts(23, 15,  4), end: ts(23, 15, 12), label: 'Laughing about Friday night' },
            ],
          },
          { id: 'g4-2b', type: 'physical', start: ts(23, 14, 32), end: ts(23, 15, 12), label: 'Kids — video games' },
        ],
      },
      { id: 'g4-3', type: 'physical', start: ts(23, 15, 12), end: ts(23, 15, 42), label: 'Watch Super Mario Bros together' },
      {
        id: 'g4-4', type: 'physical', start: ts(23, 15, 42), end: ts(23, 16,  0), label: 'Family splits up again',
        children: [
          { id: 'g4-4a', type: 'physical', start: ts(23, 15, 42), end: ts(23, 16,  0), label: 'Dad — laptop' },
          { id: 'g4-4b', type: 'physical', start: ts(23, 15, 42), end: ts(23, 16,  0), label: 'Mom — reads book' },
          { id: 'g4-4c', type: 'physical', start: ts(23, 15, 42), end: ts(23, 16,  0), label: 'Kids — video games' },
        ],
      },
    ],
  },
  {
    id: 'g5', type: 'desktop', start: ts(23, 16, 0), end: ts(23, 19, 0), label: 'Personal projects',
    children: [
      {
        id: 'g5-r', type: 'desktop', start: ts(23, 16,  0), end: ts(23, 16, 30), label: 'Research',
        children: [
          { id: 'g5-r-1', type: 'desktop', start: ts(23, 16,  0), end: ts(23, 16, 12), label: 'Perplexity' },
          { id: 'g5-r-2', type: 'desktop', start: ts(23, 16, 12), end: ts(23, 16, 22), label: 'YouTube' },
          { id: 'g5-r-3', type: 'desktop', start: ts(23, 16, 22), end: ts(23, 16, 30), label: 'Gemini' },
        ],
      },
      {
        id: 'g5-d', type: 'desktop', start: ts(23, 16, 30), end: ts(23, 18, 0), label: 'Design',
        children: [
          { id: 'g5-d-1', type: 'desktop', start: ts(23, 16, 30), end: ts(23, 17, 10), label: 'Figma' },
          { id: 'g5-d-2', type: 'desktop', start: ts(23, 17, 10), end: ts(23, 17, 40), label: 'Claude Code' },
          { id: 'g5-d-3', type: 'desktop', start: ts(23, 17, 40), end: ts(23, 18,  0), label: 'Cosmos' },
        ],
      },
      {
        id: 'g5-v', type: 'desktop', start: ts(23, 18, 0), end: ts(23, 19, 0), label: 'DevOps',
        children: [
          { id: 'g5-v-1', type: 'desktop', start: ts(23, 18,  0), end: ts(23, 18, 20), label: 'Cloudflare' },
          { id: 'g5-v-2', type: 'desktop', start: ts(23, 18, 20), end: ts(23, 18, 40), label: 'Vercel' },
          { id: 'g5-v-3', type: 'desktop', start: ts(23, 18, 40), end: ts(23, 19,  0), label: 'GitHub' },
        ],
      },
    ],
  },
  {
    id: 'g6', type: 'tablet', start: ts(23, 19, 0), end: ts(23, 21, 0), label: 'Streaming',
    children: [
      { id: 'g6-1', type: 'tablet', start: ts(23, 19,  0), end: ts(23, 19, 12), label: 'Scrolling for something to watch' },
      { id: 'g6-2', type: 'tablet', start: ts(23, 19, 12), end: ts(23, 20, 55), label: 'Severance S2E3' },
      { id: 'g6-3', type: 'tablet', start: ts(23, 20, 55), end: ts(23, 21,  0), label: 'Post-episode Reddit thread' },
    ],
  },
  {
    id: 'g7', type: 'desktop', start: ts(23, 21, 0), end: ts(23, 23, 0), label: 'Week prep',
    children: [
      { id: 'g7-1', type: 'desktop', start: ts(23, 21,  0), end: ts(23, 21, 22), label: 'Review calendar' },
      { id: 'g7-2', type: 'desktop', start: ts(23, 21, 22), end: ts(23, 21, 50), label: 'Clear email inbox' },
      { id: 'g7-3', type: 'desktop', start: ts(23, 21, 50), end: ts(23, 22, 30), label: 'Write task list' },
      { id: 'g7-4', type: 'desktop', start: ts(23, 22, 30), end: ts(23, 23,  0), label: 'Read docs / catch up' },
    ],
  },
  {
    id: 'g8', type: 'phone', start: ts(23, 23, 0), end: ts(23, 23, 59), label: 'Wind down',
    children: [
      { id: 'g8-1', type: 'phone', start: ts(23, 23,  0), end: ts(23, 23, 20), label: 'Socials scroll' },
      { id: 'g8-2', type: 'phone', start: ts(23, 23, 20), end: ts(23, 23, 45), label: 'Voice notes to self' },
      { id: 'g8-3', type: 'phone', start: ts(23, 23, 45), end: ts(23, 23, 59), label: 'Set alarms' },
    ],
  },

  // ══ PHYSICAL SPACE TRACK (home sensor data) ════════════════════════════
  // Presence detected by sensors in the home environment.
  // Gaps = user has left home (no sensor coverage).

  // ── Monday Mar 17 ──
  { id: 'ps17-1', track: 'sensor', type: 'sleep',  start: ts(17,  0,  0), end: ts(17,  7, 30), label: 'Bedroom' },
  { id: 'ps17-2', track: 'sensor', type: 'sensor', start: ts(17,  7, 30), end: ts(17, 12, 30), label: 'Home office' },
  // GAP 12:30–13:30 — left home for lunch
  { id: 'ps17-3', track: 'sensor', type: 'sensor', start: ts(17, 13, 30), end: ts(17, 18,  0), label: 'Home office' },
  // GAP 18:00–19:00 — gym (away from home)
  { id: 'ps17-4', track: 'sensor', type: 'sensor', start: ts(17, 19,  0), end: ts(17, 21,  0), label: 'Home office' },
  { id: 'ps17-5', track: 'sensor', type: 'sensor', start: ts(17, 21,  0), end: ts(18,  1,  0), label: 'Living room' },

  // ── Tuesday Mar 18 ──
  { id: 'ps18-1', track: 'sensor', type: 'sensor', start: ts(18,  0,  0), end: ts(18,  1,  0), label: 'Home office' },
  { id: 'ps18-2', track: 'sensor', type: 'sleep',  start: ts(18,  1,  0), end: ts(18,  8,  0), label: 'Bedroom' },
  { id: 'ps18-3', track: 'sensor', type: 'sensor', start: ts(18,  8,  0), end: ts(18, 13,  0), label: 'Home office' },
  // GAP 13:00–14:30 — errands (away from home)
  { id: 'ps18-4', track: 'sensor', type: 'sensor', start: ts(18, 14, 30), end: ts(18, 18,  0), label: 'Home office' },
  // GAP 18:00–20:00 — dinner out (away from home)
  { id: 'ps18-5', track: 'sensor', type: 'sensor', start: ts(18, 20,  0), end: ts(18, 22,  0), label: 'Living room' },
  { id: 'ps18-6', track: 'sensor', type: 'sleep',  start: ts(18, 22,  0), end: ts(19,  7,  0), label: 'Bedroom' },

  // ── Wednesday Mar 19 ──
  { id: 'ps19-1', track: 'sensor', type: 'sensor', start: ts(19,  7,  0), end: ts(19, 12,  0), label: 'Home office' },
  { id: 'ps19-2', track: 'sensor', type: 'sensor', start: ts(19, 12,  0), end: ts(19, 13,  0), label: 'Kitchen' },
  { id: 'ps19-3', track: 'sensor', type: 'sensor', start: ts(19, 13,  0), end: ts(19, 17,  0), label: 'Home office' },
  // GAP 17:00–18:30 — grocery shopping (away from home)
  { id: 'ps19-4', track: 'sensor', type: 'sensor', start: ts(19, 18, 30), end: ts(20,  0, 30), label: 'Home office' },

  // ── Thursday Mar 20 ──
  { id: 'ps20-1', track: 'sensor', type: 'sensor', start: ts(20,  0,  0), end: ts(20,  0, 30), label: 'Home office' },
  { id: 'ps20-2', track: 'sensor', type: 'sleep',  start: ts(20,  0, 30), end: ts(20,  8, 30), label: 'Bedroom' },
  { id: 'ps20-3', track: 'sensor', type: 'sensor', start: ts(20,  8, 30), end: ts(20, 13, 30), label: 'Home office' },
  // GAP 13:30–16:00 — at the office (away from home)
  { id: 'ps20-4', track: 'sensor', type: 'sensor', start: ts(20, 16,  0), end: ts(20, 19,  0), label: 'Home office' },
  // GAP 19:00–21:00 — social (away from home)
  { id: 'ps20-5', track: 'sensor', type: 'sensor', start: ts(20, 21,  0), end: ts(21,  1,  0), label: 'Home office' },

  // ── Friday Mar 21 ──
  { id: 'ps21-1', track: 'sensor', type: 'sensor', start: ts(21,  0,  0), end: ts(21,  1,  0), label: 'Bedroom' },
  { id: 'ps21-2', track: 'sensor', type: 'sleep',  start: ts(21,  1,  0), end: ts(21,  9,  0), label: 'Bedroom' },
  { id: 'ps21-3', track: 'sensor', type: 'sensor', start: ts(21,  9,  0), end: ts(21, 14,  0), label: 'Home office' },
  // GAP 14:00–16:00 — out & about (away from home)
  { id: 'ps21-4', track: 'sensor', type: 'sensor', start: ts(21, 16,  0), end: ts(21, 18,  0), label: 'Home office' },
  // GAP 18:00–23:00 — Friday night out (away from home)
  { id: 'ps21-5', track: 'sensor', type: 'sensor', start: ts(21, 23,  0), end: ts(22,  2,  0), label: 'Living room' },

  // ── Saturday Mar 22 ──
  // ps21-5 crosses midnight — sensor sees them arrive home at 23:00
  { id: 'ps22-1', track: 'sensor', type: 'sleep',  start: ts(22,  2,  0), end: ts(22, 11,  0), label: 'Bedroom' },
  { id: 'ps22-2', track: 'sensor', type: 'sensor', start: ts(22, 11,  0), end: ts(22, 12, 30), label: 'Living room' },
  // GAP 12:30–14:00 — lunch out (away from home)
  { id: 'ps22-3', track: 'sensor', type: 'sensor', start: ts(22, 14,  0), end: ts(22, 17,  0), label: 'Home office' },
  // GAP 17:00–20:00 — evening out (away from home)
  { id: 'ps22-4', track: 'sensor', type: 'sensor', start: ts(22, 20,  0), end: ts(23,  0, 30), label: 'Living room' },

  // ── Sunday Mar 23 ──
  // ps22-4 crosses midnight
  {
    id: 'ps23-1', track: 'sensor', type: 'sleep', start: ts(23, 0, 30), end: ts(23, 9, 0), label: 'Bedroom',
    children: [
      { id: 'ps23-1-1', track: 'sensor', type: 'sleep', start: ts(23,  0, 30), end: ts(23,  2,  0), label: 'Falling asleep' },
      { id: 'ps23-1-2', track: 'sensor', type: 'sleep', start: ts(23,  2,  0), end: ts(23,  6, 30), label: 'Deep sleep' },
      { id: 'ps23-1-3', track: 'sensor', type: 'sleep', start: ts(23,  6, 30), end: ts(23,  8, 15), label: 'Light sleep / REM' },
      { id: 'ps23-1-4', track: 'sensor', type: 'sleep', start: ts(23,  8, 15), end: ts(23,  9,  0), label: 'Waking up' },
    ],
  },
  {
    id: 'ps23-2', track: 'sensor', type: 'sensor', start: ts(23, 9, 0), end: ts(23, 10, 30), label: 'Kitchen',
    children: [
      { id: 'ps23-2-1', track: 'sensor', type: 'sensor', start: ts(23,  9,  0), end: ts(23,  9, 18), label: 'Making coffee' },
      { id: 'ps23-2-2', track: 'sensor', type: 'sensor', start: ts(23,  9, 18), end: ts(23, 10,  5), label: 'Breakfast' },
      { id: 'ps23-2-3', track: 'sensor', type: 'sensor', start: ts(23, 10,  5), end: ts(23, 10, 30), label: 'Reading at kitchen table' },
    ],
  },
  // GAP 10:30–12:00 — walk outside (away from home)
  {
    id: 'ps23-3', track: 'sensor', type: 'sensor', start: ts(23, 12, 0), end: ts(23, 14, 0), label: 'Home office',
    children: [
      { id: 'ps23-3-1', track: 'sensor', type: 'sensor', start: ts(23, 12,  0), end: ts(23, 12, 35), label: 'Lunch at desk' },
      { id: 'ps23-3-2', track: 'sensor', type: 'sensor', start: ts(23, 12, 35), end: ts(23, 14,  0), label: 'At desk — browsing' },
    ],
  },
  // GAP 14:00–16:00 — family time (away from home)
  {
    id: 'ps23-4', track: 'sensor', type: 'sensor', start: ts(23, 16, 0), end: ts(23, 23, 59), label: 'Home',
    children: [
      { id: 'ps23-4-1', track: 'sensor', type: 'sensor', start: ts(23, 16,  0), end: ts(23, 19,  0), label: 'Home office' },
      { id: 'ps23-4-2', track: 'sensor', type: 'sensor', start: ts(23, 19,  0), end: ts(23, 21,  0), label: 'Living room' },
      { id: 'ps23-4-3', track: 'sensor', type: 'sensor', start: ts(23, 21,  0), end: ts(23, 22, 45), label: 'Home office' },
      { id: 'ps23-4-4', track: 'sensor', type: 'sensor', start: ts(23, 22, 45), end: ts(23, 23, 59), label: 'Bedroom' },
    ],
  },
];
