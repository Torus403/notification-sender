# AGENTS.md

Rules for coding agents working in this repo. They override your defaults.

## No tests

Do not write, add, extend or generate tests of any kind: unit, integration, property, snapshot, end-to-end or SQL. Do not create test files, test directories, fixtures or test helpers, and do not add test dependencies. When a task would usually come with tests, skip them and don't mention it.

## Commits

Commit straight to `main`. Do not create branches or pull requests.

Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):

```
<type>(<scope>): <summary>
```

- **type**: `feat`, `fix`, `refactor`, `perf`, `docs`, `chore`, `build`, `ci`, `style` or `revert`.
- **scope**: the folder you changed: `engine`, `platform`, `web`, `docs`, `infra`, `packages`. Use `repo` for root files. Leave the scope out when a change spans several folders.
- **summary**: imperative and lowercase, with no trailing period, at most 72 characters. For example: `feat(engine): add fcm token refresh handling`.
- Add a body only when the reason for the change isn't obvious from the diff.
- Mark a breaking change with `!` after the scope and a `BREAKING CHANGE:` footer.

Make one logical change per commit.

## Tooling

Each app owns its formatter, linter and build. Don't add language tooling at the repo root.

- **Rust** (`apps/engine`): `rustfmt.toml` and `[workspace.lints]` in `Cargo.toml`.
- **TypeScript**: every new app or package adds `@biomejs/biome` (pinned to `2.6.2`) as a devDependency, `"lint": "biome check ."` and `"format": "biome check --write ."` scripts, and this `biome.json`:

  ```json
  {
  	"$schema": "https://biomejs.dev/schemas/2.6.2/schema.json",
  	"vcs": { "enabled": true, "clientKind": "git", "useIgnoreFile": true },
  	"formatter": { "lineWidth": 120 },
  	"assist": { "actions": { "source": { "organizeImports": "on" } } }
  }
  ```

No Turborepo. CI is one GitHub Actions workflow per app, triggered by `paths:` on that app's folder, plus `packages/**` for TypeScript apps.
