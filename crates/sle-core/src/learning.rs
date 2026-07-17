use crate::forest::Forest;
use crate::model::EdgeKey;
use crate::traits::EventKind;

impl Forest {
    /// Map a usage-feedback event to a reinforce/decay op on `key`. This is
    /// the whole "self-learning" step: no re-parsing happens here, only the
    /// edge weight moves based on how the knowledge was actually used.
    pub fn report_event(&mut self, key: &EdgeKey, event: EventKind) {
        let (positive, amount) = event.magnitude();
        if positive {
            self.reinforce(key, amount);
        } else {
            self.decay(key, amount);
        }
    }
}
