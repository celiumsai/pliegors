// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataCancelReason {
    ClientDisconnect,
    Deadline,
    Shutdown,
    ApplicationAbort,
    RequestBodyLimit,
    ResponseBodyLimit,
    ScopeClosed,
}

struct CancellationState {
    cancelled: AtomicBool,
    notify: Notify,
}

#[derive(Clone)]
pub struct DataCancellation {
    inner: Arc<CancellationState>,
}

impl DataCancellation {
    pub(crate) fn channel() -> (Self, DataCancellationControl) {
        let inner = Arc::new(CancellationState {
            cancelled: AtomicBool::new(false),
            notify: Notify::new(),
        });
        (
            Self {
                inner: inner.clone(),
            },
            DataCancellationControl { inner },
        )
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.inner.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

#[derive(Clone)]
pub(crate) struct DataCancellationControl {
    inner: Arc<CancellationState>,
}

impl DataCancellationControl {
    pub(crate) fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }
}
