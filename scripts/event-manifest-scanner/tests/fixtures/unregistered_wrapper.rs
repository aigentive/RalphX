fn unregistered(app: &App, event: &str) {
    app.emit(event, ()).unwrap();
}

fn call(app: &App) {
    unregistered(app, "task:created");
}
