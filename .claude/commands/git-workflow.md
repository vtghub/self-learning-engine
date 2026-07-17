# Git Branching Workflow

This repository follows a three-tier branching strategy. Apply it for every
code change — no exceptions.

## Branch roles

| Branch | Purpose |
|--------|---------|
| `main` | Default branch. Production-ready, released code only. Never commit here directly. |
| `develop` | Integration branch. All feature branches merge here first. |
| `feature/*` | Short-lived branches for individual features or fixes. Created from `develop`. |

**Feature branches are always retained after merging — never delete them.**

## Step-by-step workflow

### Start a new feature
```bash
git checkout develop
git pull origin develop
git checkout -b <descriptive-branch-name>
# work, commit…
git push -u origin <descriptive-branch-name>
```
Then open a PR targeting **`develop`** (not `main`).

### Finish a feature (merge into develop)
1. Push the feature branch to origin.
2. Create a PR: `<feature-branch>` → `develop`.
3. Merge the PR. **Do not delete the branch on GitHub.**
4. Pull the updated `develop` locally:
   ```bash
   git checkout develop && git pull origin develop
   ```

### Release (merge develop into main)
1. Create a PR: `develop` → `main`.
2. Merge the PR.
3. Pull both branches locally:
   ```bash
   git checkout main    && git pull origin main
   git checkout develop && git pull origin develop
   ```

## Enforcement rule (applies to ALL change requests, not just /git-workflow)

**Every change — no matter how small — must follow this exact sequence:**

1. `feature/*` branch (from `develop`)
2. Commit changes to the feature branch
3. Push and open a PR targeting `develop`
4. Merge the PR into `develop`
5. Merge `develop` into `main`

**Never commit directly to `develop` or `main`.** No exceptions for docs, hotfixes, or "small" changes.

## How to use this skill

When invoked as `/git-workflow [action]`, determine intent from `$ARGUMENTS`
or conversation context and execute the appropriate steps above using shell
tools. Run `git branch` and `git status` first to confirm the current state.

| `$ARGUMENTS` | Action |
|---|---|
| `start <name>` | Checkout develop, pull, create & push `<name>` branch |
| `finish` | Create PR from current branch → `develop`; retain branch |
| `release` | Create PR from `develop` → `main`; pull both branches |
| *(empty)* | Ask the user: start a feature, finish a feature, or release? |

If the current branch is not `develop` or a feature branch (e.g., someone is
working directly on `main`), warn the user before proceeding.
