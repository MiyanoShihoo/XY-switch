# XY Switch desktop workbench — implementation overrides

Date: 2026-11-06. Stack: React 19 / TypeScript / Vite / Tauri. Platforms: Windows, macOS, Linux.

## Evidence and applicability

This document describes the current renderer redesign. The 2026-10-30 design exploration and its generated `MASTER.md` remain historical references. Current implementation and verification are described in [the redesign report](../../../docs/ui-redesign-20261105.md).

## Information architecture

The desktop activity bar has five group buttons. Each group exposes its pages through horizontal section navigation in the top bar.

| Navigation group | Pages in horizontal navigation |
| --- | --- |
| Overview / 总览 | Overview |
| Connections / 连接 | Providers, Client connections, Account pool, Connection diagnostics |
| Tools / 工具 | Local plugins, Plugin marketplace, MCP servers, Prompts, Skills |
| History / 记录 | Sessions, Usage & audit |
| Settings / 设置 | Settings, Safety & Acceptance |

- Overview is the default home. Returning to a group restores its last visited page during the current app session.
- The desktop activity bar is 68px wide. The 48px top bar holds the current group's horizontal section navigation; destinations scroll horizontally when necessary.
- A 30px bottom runtime bar shows local-routing status, the currently configured provider, and the desktop or browser-preview environment. Expand routing to inspect the listener and target clients and access native start/stop controls. Configuration selection, listener state, and historical request evidence are distinct; cached success does not establish current connectivity.
- At viewport widths of 760px and below, the activity bar is hidden and the top-left menu opens all groups and destinations in a left drawer. Choosing a destination closes the drawer. Horizontal section navigation remains available; the bottom runtime bar is 34px high. Theme switching moves from the desktop activity-bar footer to the top bar.
- Feature workspaces use the width remaining after navigation and page padding. Directories, persistent detail panels, readers, forms, and audit sections follow their task's layout and container breakpoints.
- Selecting an enabled, non-current provider row activates that provider. Its Configuration action opens the addressable configuration page without activating it.
- Connection diagnostics has a provider selector and separate connection/model-review views. Opening the page does not initiate an upstream request.
- Hash routes support refresh and browser back/forward. Invalid or deleted provider IDs show an explicit empty state.

## Visual and interaction system

- Both themes use the shared `--xy-ds-*` semantic tokens and `--xy-p-*` palette primitives. Surfaces, text, accents, status colors, typography, spacing, and radii follow the current token definitions rather than the historical design palette. Fonts are local system UI and monospace stacks.
- Standard desktop controls are 32px high and compact controls are 28px. At viewport widths of 600px and below or with a coarse pointer, they use 40px and 36px respectively. Page headings, directory rows, and reading surfaces follow their task-specific hierarchy rather than a single card layout.
- Narrow content areas reflow lists, detail values, and forms. Section navigation scrolls horizontally; drawer destinations scroll vertically. Settings changes to a category selector and sessions switch between directory and reading according to their own content widths.
- Keep keyboard navigation, visible focus, accessible tabs, destination focus, dialog focus restoration, and reduced-motion preferences intact.
- Keep primary actions beside the configuration, runtime, and verification evidence needed to choose them. Long descriptions, source metadata, import options, permissions, and destructive operations use disclosures or dedicated dialogs, with explicit previews and confirmations where required.

## Feature presentation

- Overview presents the client → provider → model configuration chain, provider choices, historical diagnostics, and continuation shortcuts. A historical diagnostic result is not current health.
- Clients use a grouped compact directory beside a persistent detail panel. The inspected client, its configuration, and runtime or historical evidence remain distinguishable near connection actions. Native login, account binding, and path settings retain separate entry points and protections.
- The account pool separates gateway request accounts from native-client references, with authorization and quota evidence in each row. A native reference is not a gateway inference credential. Account details and account-selection policy open separate dialogs.
- Plugin discovery and installed plugins have separate destinations. Publisher trust, tool calls, withdrawals, imports, and reviews open dedicated panels.
- MCP pairs its server directory with a persistent target-client panel; editing a server opens its configuration dialog. Prompts pairs its directory with document reading and management. Skills uses a resource directory, explicit detail dialogs, and an Add and manage sources entry for source authorization and folder imports.
- Sessions pair a directory with an inline conversation reader. Search, client selection, and the favorite filter stay in the directory toolbar; Refresh index stays in the page header. Expand reader explicitly opens the larger reading dialog, and closing it preserves the selected inline conversation. Favorite and Copy resume command actions remain in the reading header. Session information and actions expands source metadata and guarded deletion separately; there is no automatic resume execution. At a session content width of 820px or less, directory and reading share one view with a Back to directory action.
- Usage provides Overview, Requests, Audit analysis, and Audit setup sections. Request evidence opens in a dedicated dialog; Model pricing opens from the page header. Safety & Acceptance presents policy categories with their own configuration previews, paid-request confirmations, and high-risk confirmations.
- Settings uses four directory groups: Local preferences (General, Privacy), Connections and credentials (Local routing, Credentials), Configuration and recovery (Data, Encrypted Sync), and Application information (About). It displays one category at a time. At a settings content width of 760px or less, a Settings categories selector replaces the directory; each form retains its save and confirmation flow.

## Behavior and evidence boundaries

Existing services continue to handle credentials, provider switching, order persistence, account authorization, plugin verification, model-review budgets, MCP, prompts, Skills, sessions, audit, encrypted sync, routing, and settings. Browser previews retain their native-operation restrictions.

Renderer checks establish layout, navigation, and fixture-driven interaction behavior. Native client inference, live account rotation, external marketplace deployment, and platform package acceptance require their own evidence. This redesign is a source update; released installers and historical native receipts retain their original scope.
