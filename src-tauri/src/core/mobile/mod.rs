mod discovery;
mod enrollment;
mod ingest;
mod pair_requests;
mod pairing;
mod qr;
mod routes_clips;
mod routes_pair;
mod server;
mod tls;
mod types;

pub use enrollment::{Enrollment, EnrollmentPayload};
pub use pair_requests::{short_auth_string, PairRequests, PairState, PendingPair};
pub use qr::{local_hostname, render_enrollment_qr};
pub use pairing::{hash_token, PairingManager};
pub use server::{serve_mobile, MobileState};
pub use tls::ensure_mobile_cert;

use crate::core::database::Database;
use crate::core::multimodal::SupervisorCommand;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, Mutex};

static MOBILE_PAIRING: OnceLock<Arc<PairingManager>> = OnceLock::new();
static MOBILE_PAIR_REQUESTS: OnceLock<Arc<PairRequests>> = OnceLock::new();
static MOBILE_ENROLLMENT: OnceLock<Arc<Enrollment>> = OnceLock::new();
static MOBILE_FINGERPRINT: OnceLock<String> = OnceLock::new();
static MOBILE_PORT: AtomicU16 = AtomicU16::new(0);

pub fn pairing_manager() -> Option<Arc<PairingManager>> {
    MOBILE_PAIRING.get().cloned()
}

pub fn pair_requests() -> Option<Arc<PairRequests>> {
    MOBILE_PAIR_REQUESTS.get().cloned()
}

pub fn enrollment() -> Option<Arc<Enrollment>> {
    MOBILE_ENROLLMENT.get().cloned()
}

pub fn mobile_fingerprint() -> Option<String> {
    MOBILE_FINGERPRINT.get().cloned()
}

pub fn mobile_port() -> u16 {
    MOBILE_PORT.load(Ordering::SeqCst)
}

/// Spawn the mobile HTTPS server onto the existing tokio runtime.
/// Bonjour presence while up is the phone's "is Source running?" signal.
pub fn spawn_mobile_server(
    db: Arc<Database>,
    commands: Arc<Mutex<Option<mpsc::Sender<SupervisorCommand>>>>,
    session: Option<Arc<crate::core::session_manager::SessionManager>>,
    app_handle: Option<tauri::AppHandle>,
    enabled: bool,
    preferred_port: u16,
) {
    if !enabled {
        return;
    }
    tauri::async_runtime::spawn(async move {
        match serve_mobile(db, commands, session, app_handle, true, preferred_port).await {
            Ok((port, fingerprint)) => {
                MOBILE_PORT.store(port, Ordering::SeqCst);
                let _ = MOBILE_FINGERPRINT.set(fingerprint);
                println!("Source Mobile server listening on port {port} (TLS)");
            }
            Err(error) => eprintln!("Source Mobile server failed: {error}"),
        }
    });
}

pub(crate) fn set_shared_pairing(
    pairing: Arc<PairingManager>,
    requests: Arc<PairRequests>,
    enrollment: Arc<Enrollment>,
) {
    let _ = MOBILE_PAIRING.set(pairing);
    let _ = MOBILE_PAIR_REQUESTS.set(requests);
    let _ = MOBILE_ENROLLMENT.set(enrollment);
}
