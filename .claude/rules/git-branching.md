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

## Submodules

`SovEmu.Android` and `SovEmu.iOS` are their own repositories, checked out here as submodules so
the boundary between server and client is a git boundary. As in Sovereign, they are **not
pinned**: `.gitmodules` gives each `branch = main` and `ignore = all`, nothing in this
repository builds from them, and they are updated with `git submodule update --remote`. Branch a
submodule from its freshly fetched `origin/main`, never from the frozen pointer. A change that spans repos gets a branch, and a PR, in every
repo it touches; a rule about one submodule lives in that submodule's `.claude/rules/`.
