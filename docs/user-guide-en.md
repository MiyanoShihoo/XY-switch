# XY Switch User Guide

## 1. Installation and startup

Install the Windows MSI package or extract the portable ZIP and run `xy-switch.exe`. The local proxy is disabled on startup and does not take over other applications automatically.

## 2. Provider management

Open Providers and select the Claude, Codex, or Gemini group. Add a preset or custom provider, then enter its endpoint, model, and API key. API keys must not be copied into notes, logs, or documentation.

Providers can be edited, duplicated, deleted, reordered, and selected as the active provider.

## 3. Model discovery

After configuring a compatible endpoint and credential, run model discovery. Select any discovered model from the menu or import selected models into the advanced model mapping list. The number of models depends on the upstream service response.

## 4. Local proxy

Enable the local proxy only when protocol conversion, retry, or failover is required. Keep the listener on a loopback address. The proxy stops and managed configurations are restored when the application exits.

## 5. System tray

Use the system tray menu to show or hide the main window and switch the local proxy when needed. The tray state and the main-window state should remain synchronized. Before exiting, disable takeover if another application configuration is currently managed.

## 6. MCP servers

Create stdio or HTTP MCP servers, select target applications, and enable synchronization. Configuration updates use guarded writes and attempt to restore previous files after a failure.

## 7. Prompts

Create Markdown prompts and activate them for supported applications. Prompts can be synchronized to CLAUDE.md, AGENTS.md, or GEMINI.md. Back up an existing target file before replacing its content.

## 8. Troubleshooting

- Model discovery fails: verify the endpoint, API prefix, credential, and permission.
- Only one model is available: run discovery again and use the model menu or import action.
- Requests fail: verify the selected API format, model, endpoint, and proxy status.
- Port conflict: choose another local proxy port.
- Configuration takeover is abnormal: disable takeover first, then verify that the target application configuration has been restored.

## 9. Security

Never publish API keys, local databases, diagnostic artifacts, logs, or user configuration. XY Switch does not start the routing proxy automatically.

## 10. Current limitations

Version 1.0.0 does not formally deliver full Skills management, complete usage analytics, WebDAV synchronization, account authentication, macOS packages, or Linux packages.
