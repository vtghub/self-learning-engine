# update-docs

Update README.md and docs/ to reflect the current state of the codebase.

Run this after every PR merge. Steps:

1. **Scan for changes** — read the recently merged files to understand what changed (new routes, new components, schema changes, new env vars, new pipeline flags, etc.)

2. **Update README.md** — keep these sections accurate:
   - Architecture table (layers, technologies, roles)
   - Project Structure tree (add/remove files as they change)
   - Multi-User Model section (if schema changed)
   - Environment Variables tables (worker, dashboard, GitHub Actions secrets)
   - Dashboard Features table
   - CI/CD notes (cron schedule, workflow_dispatch inputs)

3. **Update docs/architecture.md** — keep accurate:
   - Mermaid `graph TB` system diagram (nodes, edges, subgraphs)
   - Mermaid `erDiagram` data model (tables, columns, relationships)
   - Provider Registry table

4. **Update docs/request-workflow.md** — keep accurate:
   - Add new sequence diagrams for any new user-facing flows
   - Update existing diagrams if request paths changed (new middleware, new API routes, new DB queries)

5. **Commit on feature branch** — use message:
   `docs: update architecture and workflow diagrams post-PR #N`
   Then open PR → merge develop → main following the standard git workflow.

## What to look for after each PR

| PR type | Docs to update |
|---|---|
| New API route | README project structure + request-workflow.md |
| New page/component | README project structure + features table |
| Schema migration | architecture.md erDiagram + README multi-user model |
| New env var / secret | README environment variables tables |
| Pipeline change | README CI/CD section + architecture.md pipeline diagram + request-workflow.md pipeline sequence |
| Auth change | request-workflow.md login/register flows + architecture.md |
| New workflow_dispatch input | README CI/CD section |
