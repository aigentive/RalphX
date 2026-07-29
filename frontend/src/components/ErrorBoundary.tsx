import { Component, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorInfo: React.ErrorInfo | null;
}

/**
 * Error Boundary component that catches React errors and displays them visually.
 * Shows a useful error message and retains full details for diagnosis.
 */
export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null, errorInfo: null };
  }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    this.setState({ errorInfo });
    console.error("ErrorBoundary caught an error:", error, errorInfo);

    try {
      void invoke("log_frontend_error", {
        input: {
          message: error.message,
          componentStack: errorInfo.componentStack,
        },
      }).catch(() => undefined);
    } catch {
      // Tauri is optional for web and development builds.
    }
  }

  render() {
    if (this.state.hasError) {
      // Custom fallback provided
      if (this.props.fallback) {
        return this.props.fallback;
      }

      const isDev = import.meta.env.DEV;

      return (
        <div
          style={{
            padding: "20px",
            margin: "20px",
            borderRadius: "8px",
            backgroundColor: "var(--status-error-muted)",
            border: "1px solid var(--status-error-border)",
            fontFamily: "SF Pro, system-ui, sans-serif",
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: "8px",
              marginBottom: "12px",
            }}
          >
            <span style={{ fontSize: "1.25rem" }}>⚠️</span>
            <h2
              style={{
                margin: 0,
                fontSize: "1rem",
                fontWeight: 600,
                color: "#ef4444",
              }}
            >
              Something went wrong
            </h2>
          </div>

          {this.state.error && (
            <>
              <div
                style={{
                  padding: "12px",
                  borderRadius: "6px",
                  backgroundColor: "var(--overlay-scrim)",
                  marginBottom: "12px",
                  overflow: "auto",
                }}
              >
                <code
                  style={{
                    fontSize: "0.8125rem",
                    color: "#fca5a5",
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                  }}
                >
                  {isDev ? this.state.error.toString() : this.state.error.message}
                </code>
              </div>

              <details style={{ marginTop: "8px" }}>
                <summary
                  style={{
                    cursor: "pointer",
                    fontSize: "0.8125rem",
                    color: "#9ca3af",
                    marginBottom: "8px",
                  }}
                >
                  Error details
                </summary>
                <div
                  style={{
                    padding: "12px",
                    borderRadius: "6px",
                    backgroundColor: "var(--overlay-scrim)",
                    overflow: "auto",
                    maxHeight: "300px",
                  }}
                >
                  <pre
                    style={{
                      margin: 0,
                      fontSize: "0.6875rem",
                      color: "#9ca3af",
                      whiteSpace: "pre-wrap",
                    }}
                  >
                    {this.state.error.toString()}
                    {this.state.errorInfo?.componentStack}
                  </pre>
                </div>
              </details>
            </>
          )}

          <button
            onClick={() => this.setState({ hasError: false, error: null, errorInfo: null })}
            style={{
              marginTop: "12px",
              padding: "8px 16px",
              borderRadius: "6px",
              border: "none",
              backgroundColor: "#ef4444",
              color: "white",
              fontSize: "0.8125rem",
              fontWeight: 500,
              cursor: "pointer",
            }}
          >
            Try Again
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
