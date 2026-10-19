use crate::{Error, Result};
use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    routing::get,
    Router,
};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tokio::{
    net::TcpListener,
    sync::{oneshot, watch},
    task::JoinHandle,
};

type Reply = Arc<Mutex<Option<oneshot::Sender<Result<String>>>>>;

#[derive(Clone)]
struct CallbackState {
    expected_state: String,
    reply: Reply,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallbackQuery {
    state: String,
    code: Option<String>,
    error: Option<String>,
}

async fn callback(
    State(state): State<CallbackState>,
    headers: HeaderMap,
    query: std::result::Result<Query<CallbackQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => {
            return crate::callback_page::response(
                StatusCode::BAD_REQUEST,
                "Invalid bank authorization callback.",
            )
        }
    };
    let valid = headers.get(header::HOST).and_then(|h| h.to_str().ok()) == Some("127.0.0.1:1431")
        && query.state == state.expected_state;
    let (status, text) = if !valid {
        (
            StatusCode::BAD_REQUEST,
            "Invalid bank authorization callback.",
        )
    } else {
        let result = match query
            .code
            .filter(|code| !code.is_empty() && code.len() <= 2048)
        {
            Some(code) if query.error.is_none() => Ok(code),
            _ => Err(Error::Cancelled),
        };
        let delivered = state
            .reply
            .lock()
            .ok()
            .and_then(|mut tx| tx.take())
            .is_some_and(|tx| tx.send(result).is_ok());
        if delivered {
            (StatusCode::OK, "Authorization received. Return to SOURCE to see the import result. You can close this tab.")
        } else {
            (
                StatusCode::GONE,
                "This authorization attempt has already finished.",
            )
        }
    };
    crate::callback_page::response(status, text)
}

pub struct CallbackListener {
    receiver: Option<oneshot::Receiver<Result<String>>>,
    task: JoinHandle<()>,
}

impl CallbackListener {
    pub async fn bind(expected_state: String) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 1431))
            .await
            .map_err(|_| Error::CallbackPort)?;
        let (sender, receiver) = oneshot::channel();
        let app = Router::new()
            .route("/banking/callback", get(callback))
            .with_state(CallbackState {
                expected_state,
                reply: Arc::new(Mutex::new(Some(sender))),
            });
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self {
            receiver: Some(receiver),
            task,
        })
    }

    pub async fn wait(&mut self, mut cancel: watch::Receiver<u64>) -> Result<String> {
        let receiver = self.receiver.take().ok_or(Error::Cancelled)?;
        tokio::select! {
            result = receiver => result.map_err(|_| Error::Cancelled)?,
            _ = tokio::time::sleep(std::time::Duration::from_secs(600)) => Err(Error::Cancelled),
            _ = cancel.changed() => Err(Error::Cancelled),
        }
    }
}

impl Drop for CallbackListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
#[path = "callback_tests.rs"]
mod tests;
