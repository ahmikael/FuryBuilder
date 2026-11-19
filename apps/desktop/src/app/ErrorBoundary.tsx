import { Component, type ErrorInfo, type ReactNode } from "react";
import { openDiagnosticsFolder, recordUiError } from "@/lib/diagnostics-api";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Last-resort UI boundary. Rust and project data remain untouched; the user can reload the
 * interface and inspect the sanitized diagnostics log.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  override state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    recordUiError(
      `${error.message}${info.componentStack ? ` | ${info.componentStack}` : ""}`.slice(0, 3_800),
      "react.error_boundary",
    );
  }

  private openLogs = (): void => {
    void openDiagnosticsFolder().catch(() => {
      // The fallback remains usable even when the backend is already unavailable.
    });
  };

  override render(): ReactNode {
    if (!this.state.error) return this.props.children;
    return (
      <div className="flex h-full min-h-96 flex-col items-center justify-center gap-3 bg-background p-8 text-center text-foreground">
        <div className="max-w-lg space-y-2">
          <h1 className="text-base font-semibold">The editor interface stopped unexpectedly</h1>
          <p className="text-[12px] text-muted-foreground">
            Your project and the Rust world are not modified by this screen. Reload the interface to
            continue. The error message was written to the local diagnostics log without world data
            or credentials.
          </p>
          <details className="rounded border border-border bg-muted/30 p-2 text-left text-[11px]">
            <summary className="cursor-pointer text-muted-foreground">Error details</summary>
            <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words">
              {this.state.error.message}
            </pre>
          </details>
          <div className="flex flex-wrap justify-center gap-2">
            <button
              type="button"
              className="rounded bg-primary px-3 py-1.5 text-[11px] text-primary-foreground"
              onClick={() => window.location.reload()}
            >
              Reload interface
            </button>
            <button
              type="button"
              className="rounded border border-border px-3 py-1.5 text-[11px]"
              onClick={this.openLogs}
            >
              Open diagnostics folder
            </button>
          </div>
          <p className="text-[10px] text-muted-foreground">
            Settings → Diagnostics shows the log location and lets you clear old entries.
          </p>
        </div>
      </div>
    );
  }
}
