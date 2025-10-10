use crate::{
    error::ServiceError,
    services::sse_events::broadcast_phase_changed,
    state::{SharedState, state_machine::GameEvent},
};

/// Execute a planned state-machine transition, then broadcast the resulting phase change.
pub async fn run_transition_with_broadcast<F, Fut, T>(
    state: &SharedState,
    event: GameEvent,
    work: F,
) -> Result<T, ServiceError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, ServiceError>>,
{
    let (res, next) = state.run_transition(event, work).await?;
    broadcast_phase_changed(state, &next).await;
    Ok(res)
}
