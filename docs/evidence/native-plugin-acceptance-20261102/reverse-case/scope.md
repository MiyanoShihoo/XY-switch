# Case Scope

## meta
- case_id: xy-switch-installer
- created: 2026-11-02T19:02:13.8947339+08:00
- operator: local
- project_root: E:\XY switch\workspaces\session-favorites-20261029\artifacts\native-plugin-acceptance-20261102\plugin-business
- primary_skill: reverse-engineering/SKILL.md
- primary_id: R0
- lead_role: lead
- specialist_roles: []
- hint: Offline Windows PE EXE static package inspection
- preset: offline-sample

## auth
- status: granted
- basis: own_system
- evidence_of_auth: User approved representative plugin acceptance of the own XY Switch build fe4946bd2fcd4c27a7432aeadb5a656e0e6e676b from GitHub run 41419299553
- MUST NOT proceed if status != granted

## in_scope
- assets:
  - E:\XY switch\workspaces\session-favorites-20261029\artifacts\native-plugin-acceptance-20261102\github-run-41419299553\windows-x64\1.2.0\windows-x64\XY-Switch-1.2.0-windows-x64-setup.exe
- surfaces: [installer_container]
- activities: [offline_static_pe_analysis]

## out_of_scope
- assets: []
- activities: [dos, phishing_real_users, unrestricted_exfil]

## network_profile
- mode: offline
- notes: |
    offline | lab_only | authorized_target_only | unrestricted_lab
    Change mode only after auth.status = granted.

## deliverables
- report: true
- field_journal: true
- diagrams: true
- timeline: true

## constraints
- timebox: {}
- stealth: low
- data_handling: anonymize

## signoff
- ready_for_act: true
- checklist:
  - [x] auth.status = granted
  - [x] in_scope.assets non-empty OR offline sample path set
  - [x] network_profile.mode chosen
  - [x] out_of_scope reviewed
  - [x] roles assigned (lead performs analysis and review; no specialists)

## ops_refs
- skills/ops/scope-contract.md
- skills/ops/evidence-finding-path.md
- skills/ops/role-map.md
- skills/ops/timeline-workitem.md
- skills/ops/IDENTITY.md
