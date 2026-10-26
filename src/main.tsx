import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { FrontendErrorBoundary } from "./components/FrontendErrorBoundary";
import { ProviderModelQaHost } from "./components/ProviderModelQaHost";
import "./design/index.css";
import { initializeLanguage } from "./i18n/language";
void initializeLanguage();

const client = new QueryClient();
const qaProviderModels = import.meta.env.DEV && new URLSearchParams(window.location.search).get("qa") === "provider-models";
const root = qaProviderModels
  ? <ProviderModelQaHost />
  : <QueryClientProvider client={client}><FrontendErrorBoundary><App /></FrontendErrorBoundary></QueryClientProvider>;

ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode>{root}</React.StrictMode>);
