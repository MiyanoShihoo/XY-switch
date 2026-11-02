# Timeline (append-only)

## 2026-11-02T19:02:13.8947339+08:00 | lead | init
- action: case-init
- command_or_ref: skills/scripts/case-init.ps1
- result_summary: case directory created; scope ready_for_act=true
- artifacts: [scope.md, workitems.md]
- evidence_ids: []
- decision_delta: [case_initialized]
- carry_forward_refs: [scope.md]
- next: open PRIMARY SKILL.md and ACT within scope

## 2026-11-02T14:54:13.081477+00:00 | lead | static
- action: bounded offline PE / imports / strings / NSIS overlay inspection
- command_or_ref: notes/inspect_pe.py
- result_summary: manifest-bound original sample read only; seven modules and 163 direct imports retained
- artifacts: [evidence/pe-analysis.json, evidence/imports.json, evidence/selected-strings.json, evidence/manifest-binding.json]
- evidence_ids: [E-001, E-002, E-004, E-005]
- decision_delta: [phase=triage->static, direct_imports_are_shell_observations]
- carry_forward_refs: [scope.md]
- next: independent byte and hash inspection

## 2026-11-02T14:55:09.9896621+00:00 | lead | verification
- action: independent PowerShell byte inspection
- command_or_ref: notes/inspect_header.ps1
- result_summary: ten scalar and raw-byte comparisons matched; process-only script-policy recovery was approved
- artifacts: [evidence/independent-header.json, evidence/header-comparison.json]
- evidence_ids: [E-003, E-006]
- decision_delta: [PE_shell_and_NSIS_header_promoted_with_independent_support]
- carry_forward_refs: [scope.md, evidence/E-002.md]
- next: structured Findings, Path and report

## 2026-11-02T15:07:31.554Z | lead | synthesis
- action: bind Evidence, Findings and Path; save official reference knowledge, report, diagram and anonymized journal
- command_or_ref: report/2026-11-02_reverse-xy-switch-installer-report.md
- result_summary: two validated static findings; runtime and signature claims remain candidate; paid API calls zero
- artifacts: [report/analysis-flow.mmd, notes/field-journal/_index.md, notes/references/pe-nsis-knowledge.md, evidence/reference-observation.json]
- evidence_ids: [E-001, E-002, E-003, E-004, E-005, E-006, E-007]
- decision_delta: [phase=static->synthesis, official_reference_snapshots_retrieved, strict_review_pending]
- carry_forward_refs: [scope.md, evidence/E-006.md]
- next: strict case/hash review

## 2026-11-02T15:11:38.115692+00:00 | lead | review
- action: strict read-only case review with artifact hash verification
- command_or_ref: review_case.py --verify-hashes --strict --format json
- result_summary: PASS; errors=0; warnings=0; seven Evidence; six workitems; five Findings; one Path
- artifacts: [report/case-review.json, report/case-review.md, workitems.md]
- evidence_ids: [E-001, E-002, E-003, E-004, E-005, E-006, E-007]
- decision_delta: [strict_review_passed, workitems_closed]
- carry_forward_refs: [scope.md, evidence/E-006.md]
- next: preserve final review and export boundary receipts

## 2026-11-02T15:19:16.3223022Z | lead | handoff
- action: save official Mermaid export attempt and verify append-only timeline history
- command_or_ref: report/diagram-render-receipt.json
- result_summary: SVG renderer exitCode=1 because mmdc is unavailable; editable source retained; original 401-byte timeline prefix unchanged
- artifacts: [report/analysis-flow.mmd, report/diagram-render-receipt.json]
- evidence_ids: [E-006, E-007]
- decision_delta: [editable_diagram_delivered, svg_export_not_available]
- carry_forward_refs: [scope.md, report/2026-11-02_reverse-xy-switch-installer-report.md]
- next: final strict review, public evidence ledger and branch delivery
