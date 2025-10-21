use tokio::sync::{Mutex, broadcast};

use crate::dto::sse::ServerEvent;

/// SSE-specific sub-state carved out from [`AppState`].
pub struct SseState {
    public: SseHub,
    admin: AdminSseState,
}

impl SseState {
    /// Build the SSE sub-tree with per-stream channel capacities.
    pub fn new(public_capacity: usize, admin_capacity: usize) -> Self {
        Self {
            public: SseHub::new(public_capacity),
            admin: AdminSseState::new(admin_capacity),
        }
    }

    /// Access the public SSE hub used to fan out broadcast events.
    pub fn public(&self) -> &SseHub {
        &self.public
    }

    /// Access the admin SSE state bundle containing both hub and token.
    pub fn admin(&self) -> &AdminSseState {
        &self.admin
    }
}

/// State bundle holding the admin SSE hub and its coordinating token.
pub struct AdminSseState {
    hub: SseHub,
    token: Mutex<Option<String>>,
}

impl AdminSseState {
    /// Create the admin SSE manager backed by a broadcast channel and token lock.
    fn new(capacity: usize) -> Self {
        Self {
            hub: SseHub::new(capacity),
            token: Mutex::new(None),
        }
    }

    /// Borrow the broadcast hub used for admin-only events.
    pub fn hub(&self) -> &SseHub {
        &self.hub
    }

    /// Borrow the token mutex that coordinates the single admin connection.
    pub fn token(&self) -> &Mutex<Option<String>> {
        &self.token
    }
}

/// Simple broadcast hub wrapper used by the SSE services.
pub struct SseHub {
    sender: broadcast::Sender<ServerEvent>,
}

impl SseHub {
    /// Construct a new hub backed by a Tokio broadcast channel with the given capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _receiver) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Register a new subscriber that will receive subsequent events.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.sender.subscribe()
    }

    /// Send an event to all current subscribers, ignoring delivery errors.
    pub fn broadcast(&self, event: ServerEvent) {
        let _ = self.sender.send(event);
    }
}
