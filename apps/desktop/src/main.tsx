import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "@/app/App";
import { ErrorBoundary } from "@/app/ErrorBoundary";
import { installGlobalErrorReporting } from "@/app/global-errors";
import "@/styles/globals.css";

const container = document.getElementById("root");
if (!container) throw new Error("Root element not found");

installGlobalErrorReporting();

createRoot(container).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
