// Compatibility entry point for the rewritten workbench.
// Use an isolated CDP browser; all app operations are served by memory fixtures.
process.env.QA_ONLY ??= "layout,settings,routing,preview";
await import("./verify-workspace-ui.mjs");
