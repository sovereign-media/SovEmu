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

# The shipped image (every core probed while building it), loaded for the smoke test.
target "image" {
  inherits   = ["_common"]
  target     = "runtime"
  tags       = ["sovemu:ci"]
  output     = ["type=docker"]
  cache-from = ["type=gha,scope=image", "type=gha,scope=test"]
  cache-to   = ["type=gha,scope=image,mode=max"]
}
