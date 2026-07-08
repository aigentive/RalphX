#[cfg(test)]
mod tests {
    use crate::application::ThrottledEmitter;
    use ralphx_events::NullEventSink;
    use std::sync::Arc;

    #[test]
    fn is_batchable_returns_true_for_task_status_changed() {
        assert!(ThrottledEmitter::is_batchable("task:status_changed"));
    }

    #[test]
    fn is_batchable_returns_true_for_task_created() {
        assert!(ThrottledEmitter::is_batchable("task:created"));
    }

    #[test]
    fn is_batchable_returns_false_for_other_events() {
        assert!(!ThrottledEmitter::is_batchable("agent:run_completed"));
        assert!(!ThrottledEmitter::is_batchable("task:updated"));
        assert!(!ThrottledEmitter::is_batchable("agent:message_created"));
    }

    #[test]
    fn new_does_not_require_tokio_runtime() {
        // Spawn a dedicated OS thread — guaranteed no Tokio runtime context.
        // Construct EVERYTHING on this thread to avoid Send bound issues.
        let result = std::thread::spawn(|| {
            // If someone reintroduces tokio::spawn in the constructor,
            // this will panic: "there is no reactor running"
            let _emitter =
                crate::application::ThrottledEmitter::new(Arc::new(NullEventSink));
            drop(_emitter);
        })
        .join();

        assert!(
            result.is_ok(),
            "ThrottledEmitter::new() panicked — likely uses tokio::spawn instead of std::thread::spawn. See .claude/rules/tokio-runtime-safety.md"
        );
    }
}
