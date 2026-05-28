//! In-tree observation sink extension point.

use crate::event::ExportEvent;
use crossbeam_channel::Receiver;

pub trait EventSink: Send {
    fn run(self, rx: Receiver<ExportEvent>);
}
