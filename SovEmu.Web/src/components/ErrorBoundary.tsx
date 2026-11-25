// A screen that throws while rendering shows this in its place, not a blank page: the rest of
// the app (the navigation, other screens) keeps working. The one class component in the app,
// because React has error boundaries only as classes.
import { Component, type ErrorInfo, type ReactNode } from "react";
import { reportClientEvent } from "../lib/clientEvents";
import { go } from "../lib/go";
import { Button } from "./ui/button";

type ErrorBoundaryProps = {
  /** The screen's title, for the report. */
  screen: string;
  children: ReactNode;
};

type ErrorBoundaryState = { error: Error | null };

/**
 * Catches a render error below it: shows what happened with a way on (try the screen again, or
 * go home), and reports it as `SCREEN_ERROR` (React does not pass these to `window.onerror`).
 * Key it by the screen's URL, so moving to another screen starts clean.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const where = info.componentStack?.trim().split("\n")[0]?.trim() ?? "";
    reportClientEvent(
      "SCREEN_ERROR",
      `${this.props.screen}: ${error.name}: ${error.message}${where ? ` (${where})` : ""}`,
    );
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div role="alert" className="mx-auto flex max-w-md flex-col items-center gap-4 px-6 py-24 text-center">
        <h1 className="text-xl font-semibold text-slate-100">Something went wrong on this screen</h1>
        <p className="text-sm text-slate-400">{error.message || "It stopped while drawing."}</p>
        <div className="flex gap-3">
          <Button onClick={() => this.setState({ error: null })}>Try again</Button>
          <Button variant="outline" onClick={() => go("/")}>
            Go home
          </Button>
        </div>
      </div>
    );
  }
}
