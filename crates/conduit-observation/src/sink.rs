//! In-tree observation sink extension point.

use crate::event::ObservationEvent;
use crossbeam_channel::Receiver;

pub trait ObservationSink: Send {
    fn run(self, rx: Receiver<ObservationEvent>);
}
