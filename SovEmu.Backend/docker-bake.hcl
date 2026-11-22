# CI's container build (.github/workflows/ci.yml): the test suite and the shipped image in one
# BuildKit session, so their compiles run side by side. Run from the repository root:
#
#   docker buildx bake -f SovEmu.Backend/docker-bake.hcl ci
#
# Locally, `docker build` and `docker compose build` work as before.

group "ci" {
  targets = ["test", "image"]
}

target "_common" {
  context    = "."
  dockerfile = "SovEmu.Backend/Dockerfile"
  platforms  = ["linux/amd64"]
}

# The whole test suite inside the image's Debian (Mesa's llvmpipe for the GPU tests). Nothing
# to keep but its cache.
target "test" {
  inherits   = ["_common"]
  target     = "test"
  output     = ["type=cacheonly"]
  cache-from = ["type=gha,scope=test"]
  cache-to   = ["type=gha,scope=test,mode=max"]
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
  # The commit and its time, built into sovemu-server for the admin About page.
  args = {
    SOVEMU_REVISION = GITHUB_SHA
    SOVEMU_BUILT    = SOVEMU_BUILT
  }
  output     = ["type=docker"]
  cache-from = ["type=gha,scope=image", "type=gha,scope=test"]
  cache-to   = ["type=gha,scope=image,mode=max"]
}

# The test suite's line coverage (Dockerfile `coverage`): the reports, written to ./coverage
# (`summary.txt`, `lcov.info`, `html/`). CI's Coverage job runs it on main; locally,
# scripts/coverage.sh is quicker.
target "coverage" {
  inherits   = ["_common"]
  target     = "coverage"
  output     = ["type=local,dest=coverage"]
  cache-from = ["type=gha,scope=coverage", "type=gha,scope=test"]
  cache-to   = ["type=gha,scope=coverage,mode=max"]
}
