import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "@mimic/ui";

interface State {
  error: Error | null;
}

/** Never white-screen (spec §18): render the error and a way back. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("[ui] uncaught render error", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="fatal">
        <h1>Something went wrong in this view</h1>
        <p className="neutral">Mimic is still running. Your data and jobs are unaffected.</p>
        <pre>{this.state.error.message}</pre>
        <div className="row gap-2">
          <Button variant="primary" onClick={() => this.setState({ error: null })}>
            Try again
          </Button>
          <Button onClick={() => window.location.assign("/")}>Go to Home</Button>
        </div>
      </div>
    );
  }
}
