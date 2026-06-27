use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

const DEFAULT_SCROLL_SETTLE_MS: i64 = 900;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OcrTriggerSignalsSnapshot {
    pub last_cursor_motion_at: Option<i64>,
    pub last_scroll_at: Option<i64>,
    pub last_app_switch_at: Option<i64>,
    pub last_large_scene_change_at: Option<i64>,
    pub scroll_settle_deadline_at: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct OcrTriggerOutcome {
    pub app_switch: bool,
    pub large_scene_change: bool,
    pub scroll_settled: bool,
    pub cursor_recent: bool,
}

#[derive(Debug, Default)]
struct OcrTriggerSignalsState {
    last_cursor_motion_at: Option<i64>,
    last_scroll_at: Option<i64>,
    last_app_switch_at: Option<i64>,
    last_large_scene_change_at: Option<i64>,
    scroll_settle_deadline_at: Option<i64>,
    pending_app_switch: bool,
    pending_large_scene_change: bool,
}

pub struct OcrTriggerSignals {
    state: RwLock<OcrTriggerSignalsState>,
}

impl OcrTriggerSignals {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(OcrTriggerSignalsState::default()),
        }
    }

    pub async fn mark_cursor_motion(&self, timestamp: i64) {
        let mut state = self.state.write().await;
        state.last_cursor_motion_at = Some(timestamp);
    }

    pub async fn mark_scroll(&self, timestamp: i64) {
        let mut state = self.state.write().await;
        state.last_scroll_at = Some(timestamp);
        state.scroll_settle_deadline_at = Some(timestamp + DEFAULT_SCROLL_SETTLE_MS);
    }

    pub async fn mark_app_switch(&self, timestamp: i64) {
        let mut state = self.state.write().await;
        state.last_app_switch_at = Some(timestamp);
        state.pending_app_switch = true;
    }

    pub async fn mark_large_scene_change(&self, timestamp: i64) {
        let mut state = self.state.write().await;
        state.last_large_scene_change_at = Some(timestamp);
        state.pending_large_scene_change = true;
    }

    pub async fn consume_due(&self, timestamp: i64) -> OcrTriggerOutcome {
        let mut state = self.state.write().await;
        let cursor_recent = state
            .last_cursor_motion_at
            .map(|last| timestamp - last <= 1_500)
            .unwrap_or(false);

        let scroll_settled = state
            .scroll_settle_deadline_at
            .map(|deadline| timestamp >= deadline)
            .unwrap_or(false);

        if scroll_settled {
            state.scroll_settle_deadline_at = None;
        }

        let outcome = OcrTriggerOutcome {
            app_switch: state.pending_app_switch,
            large_scene_change: state.pending_large_scene_change,
            scroll_settled,
            cursor_recent,
        };

        state.pending_app_switch = false;
        state.pending_large_scene_change = false;

        outcome
    }

    pub async fn snapshot(&self) -> OcrTriggerSignalsSnapshot {
        let state = self.state.read().await;
        OcrTriggerSignalsSnapshot {
            last_cursor_motion_at: state.last_cursor_motion_at,
            last_scroll_at: state.last_scroll_at,
            last_app_switch_at: state.last_app_switch_at,
            last_large_scene_change_at: state.last_large_scene_change_at,
            scroll_settle_deadline_at: state.scroll_settle_deadline_at,
        }
    }
}
