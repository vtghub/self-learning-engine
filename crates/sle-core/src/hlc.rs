use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::id::ReplicaId;

/// A hybrid logical clock timestamp: (wall-clock millis, tie-break counter).
/// Two HLCs from different replicas are totally ordered, which is what lets
/// last-writer-wins fields (like node payload edits) resolve deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Hlc {
    pub physical: u64,
    pub logical: u64,
}

impl Hlc {
    pub const ZERO: Hlc = Hlc { physical: 0, logical: 0 };
}

fn physical_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as u64
}

/// Per-replica clock generator. `tick` mints a new local timestamp; `observe`
/// folds in a timestamp seen from a remote replica so causality is preserved
/// even if wall clocks disagree slightly, per the standard HLC algorithm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HlcClock {
    pub replica: ReplicaId,
    last: Hlc,
}

impl HlcClock {
    pub fn new(replica: ReplicaId) -> Self {
        Self { replica, last: Hlc::ZERO }
    }

    pub fn tick(&mut self) -> Hlc {
        let now = physical_now_ms();
        self.last = if now > self.last.physical {
            Hlc { physical: now, logical: 0 }
        } else {
            Hlc { physical: self.last.physical, logical: self.last.logical + 1 }
        };
        self.last
    }

    pub fn observe(&mut self, remote: Hlc) -> Hlc {
        let now = physical_now_ms();
        let physical = now.max(self.last.physical).max(remote.physical);
        let logical = if physical == self.last.physical && physical == remote.physical {
            self.last.logical.max(remote.logical) + 1
        } else if physical == self.last.physical {
            self.last.logical + 1
        } else if physical == remote.physical {
            remote.logical + 1
        } else {
            0
        };
        self.last = Hlc { physical, logical };
        self.last
    }
}
