function subscribe(bus: EventBus, event: string) {
  bus.subscribe(event, () => {});
}
