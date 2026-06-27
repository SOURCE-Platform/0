import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";

function renderFatalError(error: unknown) {
  const rootElement = document.getElementById("root");
  if (!rootElement) return;

  const message =
    error instanceof Error
      ? `${error.name}: ${error.message}\n\n${error.stack ?? ""}`
      : String(error);

  ReactDOM.createRoot(rootElement).render(
    <React.StrictMode>
      <div className="min-h-screen bg-background px-6 py-8 text-foreground">
        <div className="mx-auto max-w-3xl rounded-2xl border border-red-500/30 bg-red-950/20 p-6">
          <h1 className="text-2xl font-semibold text-foreground">
            SOURCE failed to start
          </h1>
          <p className="mt-3 max-w-[60ch] text-sm leading-6 text-muted-foreground">
            The desktop window hit a startup error. The details below are
            shown so we can debug the exact failure instead of leaving the
            app blank.
          </p>
          <pre className="mt-5 overflow-x-auto rounded-xl border border-border/70 bg-black/20 p-4 text-xs leading-6 text-red-100">
{message}
          </pre>
        </div>
      </div>
    </React.StrictMode>,
  );
}

window.addEventListener("error", (event) => {
  renderFatalError(event.error ?? event.message);
});

window.addEventListener("unhandledrejection", (event) => {
  renderFatalError(event.reason);
});

async function bootstrap() {
  try {
    const { default: App } = await import("./App");
    const rootElement = document.getElementById("root");

    if (!rootElement) {
      throw new Error("Missing #root mount element.");
    }

    ReactDOM.createRoot(rootElement).render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    );
  } catch (error) {
    renderFatalError(error);
  }
}

void bootstrap();
