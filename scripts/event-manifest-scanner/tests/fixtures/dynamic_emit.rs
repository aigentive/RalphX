fn dynamic(app: &App, event: &str) {
    app.emit(event, ()).unwrap();
}
