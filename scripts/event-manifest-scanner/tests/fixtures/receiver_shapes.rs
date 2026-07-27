const AGENT_RUN_COMPLETED: &str = "agent:run_completed";

fn emit_app_event(app: &App, event: &str, payload: Payload) {
    app.emit(event, payload).unwrap();
}

fn emit_http_event(app: &App, event: &str, payload: Payload) {
    emit_app_event(app, event, payload);
}

fn emit_queue_changed(app: &App) {
    app.emit("execution:queue_changed", ()).unwrap();
}

fn emit_ticketing_operation_event(app: &App, payload: TicketingOperationEvent) {
    app.emit("ticketing:cache_invalidated", payload).unwrap();
}

fn emit_serialized(sink: &Sink, event: &str, payload: Payload) {
    sink.emit(event, payload);
}

struct ThrottledEmitter;
impl ThrottledEmitter {
    fn emit(&self, event: &str, payload: Payload) {
        self.sink.emit(event, payload);
    }
}

fn sites(app: &App, app_handle: &AppHandle, sink: &Sink, state: &State, throttled: &ThrottledEmitter) {
    app.emit("task:created", ()).unwrap();
    app_handle.emit(AGENT_RUN_COMPLETED, ()).unwrap();
    self.app.emit("task:deleted", ()).unwrap();
    sink.emit("agent:chunk", ()).unwrap();
    let chained = app
        .clone()
        .manager();
    chained
        .emit("notification:created", ())
        .unwrap();
    emit_app_event(app, "task:status_changed", ());
    emit_http_event(app, "task:archived", ());
    emit_queue_changed(app);
    emit_ticketing_operation_event(app, TicketingOperationEvent::Changed);
    emit_serialized(sink, "task:merge_progress", ());
    throttled.emit("task:created", ());
    state.events.emit("task:restored", ());
}

struct AppChatService;
impl AppChatService {
    fn emit_event(&self, event: &str, payload: Payload) {
        self.handle.emit(event, payload).unwrap();
    }

    fn sites(&self, emitter: &EventEmitter) {
        self.emit_event("agent:message_queued", ());
        emitter.emit_with_payload("review:update", "task", "{}");
    }
}
