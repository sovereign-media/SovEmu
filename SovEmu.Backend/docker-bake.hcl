# CI's container build (.github/workflows/ci.yml): the test suite and the shipped image in one
# BuildKit session, so their compiles run side by side. Run from the repository root:
#
#   docker buildx bake -f SovEmu.Backend/docker-bake.hcl ci
#
# Locally, `docker build` and `docker compose build` work as before.

group "ci" {
  targets = ["test", "image"]
}

# The same, and the dependency layers exported to BuildKit's GitHub cache: main runs this when the
# cache for its lockfile, manifests and Dockerfile is missing (ci.yml). Every other build reads them.
group "ci-deps" {
  targets = ["test", "image", "deps-release", "deps-test"]
}

# The dependencies alone (Dockerfile `deps-release`, `deps-test`: cargo-chef from the recipe). Only
# these are exported in full (`mode=max`): the layers after them hold SovEmu's own crates, which
# change with every commit, and exporting them each run cost as much as it saved (#245).
target "deps-release" {
  inherits = ["_common"]
  target   = "deps-release"
  output   = ["type=cacheonly"]
  cache-to = ["type=gha,scope=deps-release,mode=max"]
}

target "deps-test" {
  inherits = ["_common"]
  target   = "deps-test"
  output   = ["type=cacheonly"]
  cache-to = ["type=gha,scope=deps-test,mode=max"]
}

target "_common" {
  context    = "."
  dockerfile = "SovEmu.Backend/Dockerfile"
  platforms  = ["linux/amd64"]
}

# SovEmu's ScreenScraper developer credentials, as BuildKit secrets from the environment
# (ci.yml sets them from the repository's secrets; unset, none are built in). Both `test` and
# `image` pass them: the test suite waits on the same `build` stage the image ships, and BuildKit
# runs that compile once only while the two targets ask for it identically.
variable "SCREENSCRAPER_SECRETS" {
  default = [
    "id=screenscraper_devid,env=SOVEMU_BUILD_SCREENSCRAPER_DEVID",
    "id=screenscraper_devpassword,env=SOVEMU_BUILD_SCREENSCRAPER_DEVPASSWORD",
  ]
}

# The whole test suite inside the image's Debian (Mesa's llvmpipe for the GPU tests). Nothing
# is kept: its dependencies come from `deps-test`'s cache, the rest changes with every commit.
target "test" {
  inherits   = ["_common"]
  target     = "test"
  secret     = SCREENSCRAPER_SECRETS
  output     = ["type=cacheonly"]
  cache-from = ["type=gha,scope=deps-test", "type=gha,scope=deps-release"]
}

# Set by GitHub Actions; empty in a local bake.
variable "GITHUB_SHA" {
  default = ""
}

# The commit's time (ISO 8601, UTC), set by ci.yml; empty in a local bake.
variable "SOVEMU_BUILT" {
  default = ""
}

# The shipped image (every core probed while building it), loaded for the smoke test and, on
# main, published to GHCR as it is (ci.yml). The labels say which commit it is and link the
# package to the repository.
target "image" {
  inherits   = ["_common"]
  target     = "runtime"
  tags       = ["sovemu:ci"]
  labels = {
    "org.opencontainers.image.source"   = "https://github.com/sovereign-media/SovEmu"
    "org.opencontainers.image.revision" = GITHUB_SHA
  }
  # The commit and its time, written beside sovemu-server for the admin About page. Only the
  # runtime stage reads them, after the compile, which therefore stays the same as `test`'s.
  args = {
    SOVEMU_REVISION = GITHUB_SHA
    SOVEMU_BUILT    = SOVEMU_BUILT
  }
  secret = SCREENSCRAPER_SECRETS
  output     = ["type=docker"]
  # Every stage it builds (the web client, the cores, the runtime's packages): `build` leaves only
  # the two binaries in its layer, so this exports no compiled crates.
  cache-from = ["type=gha,scope=image", "type=gha,scope=deps-release"]
  cache-to   = ["type=gha,scope=image,mode=max"]
}

# The test suite's line coverage (Dockerfile `coverage`): the reports, written to ./coverage
# (`summary.txt`, `lcov.info`, `html/`). CI's Coverage job runs it on main; locally,
# scripts/coverage.sh is quicker.
target "coverage" {
  inherits   = ["_common"]
  target     = "coverage"
  output     = ["type=local,dest=coverage"]
  cache-from = ["type=gha,scope=coverage", "type=gha,scope=deps-test"]
  cache-to   = ["type=gha,scope=coverage,mode=max"]
}
