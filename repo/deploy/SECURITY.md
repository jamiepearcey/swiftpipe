# Deployment Security Policy

## Docker base-image digests

All Docker base images must be pinned as `tag@sha256:<digest>` in
`Dockerfile`. The tag keeps the intended upstream release readable; the digest
makes the build reproducible and prevents mutable tag republishing from
silently changing the base.

To refresh a base image digest:

```bash
docker pull node:22-bookworm-slim
docker image inspect node:22-bookworm-slim --format '{{index .RepoDigests 0}}'
```

Repeat for each base image, update the matching `FROM` line, then run:

```bash
docker build .
```

Digest refreshes should be reviewed like dependency upgrades: confirm the tag,
the resolved digest, and the build result in the change description.
