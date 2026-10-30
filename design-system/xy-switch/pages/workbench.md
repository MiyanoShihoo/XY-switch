# XY Switch desktop workbench — implementation overrides

Date: 2026-10-30. Stack: React 19 / TypeScript / Vite / Tauri. Platforms: Windows, macOS, Linux.

## Evidence and applicability

UI/UX Pro Max was actually invoked with a system-wide design query, then a narrower developer-tools query, navigation UX, and React state/focus searches. Raw search output is retained in artifacts/ui-reconstruction-20261030/skill-*.md.

The skill's Minimalism & Swiss Style, high contrast, semantic tokens, local-first typography and restrained motion fit this product. Both design searches returned a FAQ/documentation landing pattern: it does not fit a desktop configuration application and is NOT implemented. The following workbench information architecture overrides that marketing pattern. The skill's generated MASTER is retained unmodified for provenance.

## Information architecture

- Workspace: Providers, MCP, Prompts, Skills.
- Observe: Connection diagnostics, Sessions, Usage and audit.
- Manage: Security and acceptance, Settings.
- Providers is a full-width switching list. It has no persistent secondary inspector or hidden duplicate inspector.
- Configuration details is a dedicated, addressable child page of Providers: endpoint, protocol, URL mode, default model, model mappings, edit, duplicate without secrets, and confirmed deletion.
- Connection diagnostics is a separate navigation destination: provider selector, last connection result, explicit test actions, latency/error/success metrics, and the unchanged low-token verification controls in a second accessible tab.
- Viewing either destination never activates a provider and never initiates an upstream request.
- Hash routes support refresh and browser back/forward. Invalid or deleted provider IDs show an explicit empty state, never an assumed activation.

## Visual and interaction system

- 220px grouped dark navigation; 72px labeled-by-accessible-name rail on narrow desktop windows; horizontal navigation on compact viewports.
- 64px context bar, 48px runtime bar, single main scroller. Native runtime state remains distinct from selected configuration and health evidence.
- Slate surfaces and teal configuration accent; success, warning and failure use distinct semantic tokens. Light and dark palettes are authored together.
- 4/8px rhythm; 14px desktop body, 12px metadata, 26px main title; local system UI/monospace fonts only. No remote font imports, external images, or decorative effect dependencies.
- 40px controls, 36px compact controls, visible focus, keyboard drag sorting, accessible tabs, focus on destination changes, and modal focus restoration.
- Reduced-motion respects the existing application motion guard. Hover changes colors, not layout.

## Retained behavior

Provider credentials, switching, order persistence, advanced editor, model verification budgets/privacy gates, MCP, prompts, opt-in skills, sessions, audit, encrypted sync, routing and settings remain backed by existing services. Historical release packages and native acceptance evidence are not replaced by this renderer redesign.
