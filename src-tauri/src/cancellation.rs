use std::fmt;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::watch;

pub(crate) struct CancellationState {
    pub(crate) gate: Mutex<()>,
    pub(crate) changed: watch::Sender<bool>,
}

#[derive(Clone)]
pub struct Cancellation(pub(crate) Arc<CancellationState>);

impl Default for Cancellation {
    fn default() -> Self {
        Self(Arc::new(CancellationState {
            gate: Mutex::new(()),
            changed: watch::channel(false).0,
        }))
    }
}

impl fmt::Debug for Cancellation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cancellation")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl Cancellation {
    pub fn cancel(&self) {
        if let Ok(_guard) = self.0.gate.lock() {
            self.0.changed.send_replace(true);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        *self.0.changed.borrow()
    }

    pub async fn cancelled(&self) {
        let mut receiver = self.0.changed.subscribe();
        loop {
            if *receiver.borrow_and_update() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }

    pub(crate) fn downgrade(&self) -> Weak<CancellationState> {
        Arc::downgrade(&self.0)
    }
}
