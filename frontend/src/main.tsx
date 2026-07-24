import * as React from "react";
import * as ReactDOM from "react-dom/client";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { StartupRoot } from "./StartupRoot";
import { syncThemeAttributesFromStore } from "./stores/themeStore";
import "./styles/globals.css";

// Reassert persisted theme / motion / font-scale on DOM after main.tsx loads.
// The inline script in index.html handles the pre-hydration case; this keeps
// attributes in sync with the Zustand store for any in-flight settings
// changes that happened between the two loads.
syncThemeAttributesFromStore();

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);
const visualTest = new URLSearchParams(window.location.search).get("test");

if (visualTest === "startup") {
  void import("./test-pages/StartupVisualTest").then(({ StartupVisualTestPage }) => {
    root.render(
      <React.StrictMode>
        <ErrorBoundary>
          <StartupVisualTestPage
            scenario={new URLSearchParams(window.location.search).get("scenario")}
          />
        </ErrorBoundary>
      </React.StrictMode>,
    );
  });
} else {
  root.render(
    <React.StrictMode>
      <ErrorBoundary>
        <StartupRoot />
      </ErrorBoundary>
    </React.StrictMode>,
  );
}
