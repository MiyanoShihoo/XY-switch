# Work Items

| ID | title | role | targets | surface | status | evidence | notes |
|----|-------|------|---------|---------|--------|----------|-------|
| WI-001 | Scope and build identity | lead | own installer | process | done | E-001 | granted / offline scope preserved |
| WI-002 | Independent PE and NSIS header observations | lead | own installer | static bytes | done | E-002, E-003, E-006 | ten scalar / byte comparisons matched |
| WI-003 | Direct imports and bounded strings | lead | own installer shell | imports | done | E-004, E-005 | runtime capabilities remain candidate |
| WI-004 | Official reference knowledge | lead | documentation only | references | done | E-007 | snapshots fixed by SHA256 |
| WI-005 | Formal report, editable diagram and journal | lead | case | synthesis | done | E-001, E-002, E-003, E-004, E-005, E-006, E-007 | flavor=null; anonymized case-local journal |
| WI-006 | Strict case review with fixity verification | lead | case graph | review | done | E-001, E-002, E-003, E-004, E-005, E-006, E-007 | strict/hash review PASS; report/case-review.json and case-review.md preserve the final result |

## Coverage
- [x] Recon/analysis complete for in_scope assets
- [x] Critical/High candidates triaged (N/A for pure RE)
- [x] Validated findings have independent Evidence (E-*)
- [x] Path documented (solve)
- [x] Timeline continuous across major phases
- [x] Report via docs-generator
- [x] field-journal anonymized

## Refs
- skills/ops/timeline-workitem.md
- skills/ops/evidence-finding-path.md
