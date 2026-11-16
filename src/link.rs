use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnId(u64);

/// Bookkeeping for the single live extension WebSocket. Each attach gets a fresh id so a
/// late-closing old socket cannot tear down a newer one.
#[derive(Default)]
pub struct ExtLink {
    next: u64,
    current: Option<(ConnId, UnboundedSender<String>)>,
}

impl ExtLink {
    pub fn attach(&mut self, tx: UnboundedSender<String>) -> ConnId {
        self.next += 1;
        let id = ConnId(self.next);
        self.current = Some((id, tx));
        id
    }

    /// Returns true if `id` was the live connection (and is now cleared).
    pub fn detach(&mut self, id: ConnId) -> bool {
        if self.is_current(id) {
            self.current = None;
            true
        } else {
            false
        }
    }

    pub fn is_current(&self, id: ConnId) -> bool {
        matches!(&self.current, Some((c, _)) if *c == id)
    }

    pub fn is_connected(&self) -> bool {
        self.current.is_some()
    }

    pub fn sender(&self) -> Option<&UnboundedSender<String>> {
        self.current.as_ref().map(|(_, tx)| tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn stale_detach_keeps_newer_connection() {
        let mut link = ExtLink::default();
        let (a_tx, _a_rx) = unbounded_channel();
        let (b_tx, mut b_rx) = unbounded_channel();
        let a = link.attach(a_tx);
        let b = link.attach(b_tx);
        assert!(!link.is_current(a));
        assert!(!link.detach(a), "stale detach must not clear");
        assert!(link.is_connected());
        assert!(link.is_current(b));
        link.sender().unwrap().send("hi".into()).unwrap();
        assert_eq!(b_rx.try_recv().unwrap(), "hi");
    }

    #[test]
    fn current_detach_clears() {
        let mut link = ExtLink::default();
        let (a_tx, _a_rx) = unbounded_channel();
        let (b_tx, _b_rx) = unbounded_channel();
        link.attach(a_tx);
        let b = link.attach(b_tx);
        assert!(link.detach(b));
        assert!(!link.is_connected());
        assert!(link.sender().is_none());
        assert!(!link.detach(b), "second detach is a no-op");
    }

    #[test]
    fn ids_are_unique_and_reattach_after_detach_works() {
        let mut link = ExtLink::default();
        let (t1, _r1) = unbounded_channel();
        let (t2, _r2) = unbounded_channel();
        let a = link.attach(t1);
        assert!(link.detach(a));
        let b = link.attach(t2);
        assert_ne!(a, b);
        assert!(link.is_current(b));
        assert!(!link.detach(a));
        assert!(link.is_connected());
    }
}
