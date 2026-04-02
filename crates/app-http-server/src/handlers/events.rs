use super::*;

pub(crate) async fn events_handler(
    State(state): State<ServerContext>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.event_sender.subscribe();
    let stream = BroadcastStream::new(receiver).map(|item| {
        let event = match item {
            Ok(event) => event,
            Err(_) => ApiEvent {
                event: "system:lagged".to_string(),
                message: "事件缓冲拥塞，已丢弃部分消息".to_string(),
                source_id: None,
                timestamp: current_timestamp_rfc3339()
                    .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
            },
        };
        let payload = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default().event(event.event).data(payload))
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    )
}
