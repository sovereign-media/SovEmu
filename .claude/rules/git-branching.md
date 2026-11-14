# Git Branching

SovEmu follows the same model as `sovereign-media/sovereign`: **trunk-based on `main`**.

- `main` is the default branch, the integration branch and every PR's target.
- Branch from the fresh tip of `main`, never from whatever happens to be checked out:

  ```bash
  git fetch --prune origin
  git checkout -b feat/thing origin/main
  ```

- Name branches `feat/`, `fix/`, `chore/` or `docs/`.
- Never commit straight onto `main`.

## Submodules (planned)

The clients (`SovEmu.Web`, `SovEmu.Android`, `SovEmu.iOS`) will each be their own repository,
checked out here as a submodule so the boundary between server and client is a git boundary.
As in Sovereign, they will **not be pinned**: `.gitmodules` will give each `branch = main` and
`ignore = all`, nothing in this repository builds from them, and they are updated with
`git submodule update --remote`. A change that spans repos gets a branch, and a PR, in every
repo it touches; a rule about one submodule lives in that submodule's `.claude/rules/`.
