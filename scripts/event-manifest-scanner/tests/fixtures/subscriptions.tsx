const ATTENTION_INVALIDATION_EVENTS = [
  "agent:run_started",
  "agent:run_completed",
] as const;
const DIRECT_EVENT = "notification:created";

function subscribe(bus: EventBus) {
  bus.subscribe(DIRECT_EVENT, () => {});
  ATTENTION_INVALIDATION_EVENTS.map((event) => bus.subscribe(event, () => {}));
}
