use super::AgentEvent;
use std::sync::{Arc, Mutex};

pub trait EventSink: Send + Sync {
    fn append(&self, event: AgentEvent);
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryEventSink {
    events: Arc<Mutex<Vec<AgentEvent>>>,
}

impl InMemoryEventSink {
    pub fn events(&self) -> Vec<AgentEvent> {
        self.events
            .lock()
            .expect("in-memory event sink mutex should not be poisoned")
            .clone()
    }
}

impl EventSink for InMemoryEventSink {
    fn append(&self, event: AgentEvent) {
        self.events
            .lock()
            .expect("in-memory event sink mutex should not be poisoned")
            .push(event);
    }
}
