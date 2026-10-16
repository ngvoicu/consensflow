---
id: upload-fix
title: Fix File Upload Bug
status: completed
created: 2026-09-28
updated: 2026-09-30
priority: low
---

# Fix File Upload Bug

## Phase 1: Fix and Test [completed]

- [x] [UPL-01] Reproduce the bug with a 15MB file
- [x] [UPL-02] Fix the multipart parser's size limit in `src/upload/parser.ts`
- [x] [UPL-03] A proper error for oversized files
- [x] [UPL-04] Tests at 10MB, 10.1MB and 100MB
- [x] [UPL-05] Test in staging
