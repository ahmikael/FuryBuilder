import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
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
      <div className="flex h-full min-h-96 items-center justify-center bg-background p-8 text-foreground">
        <Card className="w-full max-w-lg gap-0 py-0">
          <CardHeader className="space-y-2 p-6">
            <CardTitle className="text-xl">The editor interface stopped unexpectedly</CardTitle>
            <p className="text-sm leading-relaxed text-muted-foreground">
              Your project and the Rust world are not modified by this screen. Reload the interface
              to continue. The error message was written to the local diagnostics log without world
              data or credentials.
            </p>
          </CardHeader>
          <CardContent className="space-y-4 p-6 pt-0">
            <details className="rounded-xl border bg-muted/30 p-4 text-sm">
              <summary className="cursor-pointer text-muted-foreground">Error details</summary>
              <pre className="mt-2 max-h-40 overflow-auto font-mono text-xs whitespace-pre-wrap break-words">
                {this.state.error.message}
              </pre>
            </details>
            <div className="flex flex-wrap gap-2">
              <Button onClick={() => window.location.reload()}>Reload interface</Button>
              <Button variant="outline" onClick={this.openLogs}>
                Open diagnostics folder
              </Button>
            </div>
            <p className="text-sm leading-relaxed text-muted-foreground">
              Settings → Diagnostics shows the log location and lets you clear old entries.
            </p>
          </CardContent>
        </Card>
      </div>
    );
  }
}
