import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  /** What is being isolated, for the message and the log. */
  label: string;
  children: ReactNode;
  /** Render nothing on failure (status-bar items). */
  silent?: boolean;
}

interface State {
  error: Error | null;
}

/** Isolates a module contribution so a failure never takes down the editor (SPEC §5.3). */
export class ErrorBoundary extends Component<Props, State> {
  override state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error(`[margin] ${this.props.label} failed`, error, info.componentStack);
  }

  override render() {
    if (!this.state.error) return this.props.children;
    if (this.props.silent) return null;
    return (
      <div className="module-error" role="alert">
        <p>
          {this.props.label} stopped working: {this.state.error.message}
        </p>
        <button onClick={() => this.setState({ error: null })}>Try again</button>
      </div>
    );
  }
}
