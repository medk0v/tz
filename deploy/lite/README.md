# Lite at tz.cyber-money.org

Lite uses the same application code with a neutral login, no product logo or
landing page, and one internal project. Project selection/creation is unavailable
in both the UI and API. Departments, roles and their access rules remain available.
The custom widget opening animation is disabled in Lite; standard widgets retain
their independent appearance checkbox.

The destination is the Lite host named by the `TZ_LITE_DEPLOY_HOST` secret
(Ubuntu 26.04, ARM64). It receives compiled Linux binaries, static frontend assets, runtime images and installation configuration.
It never receives the application repository and never compiles the application.
The standard `Deploy production` workflow and its server helpers are unchanged.

Lite sends OpenClaw neutral runtime instructions: `support-telegram-notify`,
`support-schedule-reminder`, the other `support-*` capability commands, and
`support_*` context blocks. Requests use the dedicated `openclaw/support` agent
and its `workspace-support` directory. Protected curl uses `--support-grant`; isolated shell
variables use `SUPPORT_VAR_*`. The image installs copies of the same checked
wrappers and the generated approval policy permits only their neutral names.
The migration removes legacy command approvals before activating the renamed runtime. Grant files,
permissions, notification recipients and timer semantics are unchanged.
Deploy the application and AI artifacts together through `Deploy Lite`.
Lite uses the `tz` namespace for host paths, application/database accounts,
services, Redis pub/sub, cookies and browser storage. The deployment account is
`tz-deploy`; runtime services are `tz-api`, `tz-worker` and `tz-redis`.
Its encryption key is saved as `TZ_SECRETS_KEY` in `/etc/tz/tz.env`.

## Delivery

`Deploy Lite` starts on its own as soon as the `CI` workflow completes
successfully for a push to `master`, and it releases that exact commit rather than
whatever the branch head happens to be. A CI run that failed, or one from a pull
request, is skipped before any build starts.

It can still be started by hand: open **Actions → Deploy Lite → Run workflow**,
select `master`, and run. The selected commit must already have a successful push
CI run.

Application artifacts include the committed `GeoLite2-City.mmdb` and
`GeoLite2-Country.mmdb` databases. Both are required and covered by manifest
checksums. The receiver installs them as `root:tz` with mode `0640` under
`/var/lib/tz/geoip` before restarting the services.
Before the first deployment with GeoIP artifacts, existing installations must
update `/usr/local/lib/tz/lite-release.py` and `/usr/local/sbin/tz-lite-deploy`
from the configuration-only `package-provision` bundle (root-owned, modes `0644`
and `0755` respectively). The old validator rejects the new `geoip/` entries;
normal application delivery does not update these privileged helpers.

Application and AI builds run in separate jobs, followed by a delivery job that
requires both artifacts. Builds default to GitHub's native
[`ubuntu-24.04-arm` runner](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
To use a prepared build runner on the main server, set the
repository variable `TZ_LITE_BUILD_RUNNER` to a JSON label array, for example
`["self-hosted","linux","ARM64","lite-builder"]`. Register only the intended
builder with those labels; never register the Lite destination as a build runner.
On an x86 build host, configure an ARM64 Buildx node or emulation first. The
application build always targets `linux/arm64`.

The builder needs Git, Python 3, Docker/Buildx and the GitHub CLI. It must have
enough RAM/disk for the Rust and AI image builds. Build jobs run only a verified
`master` commit. Installing/configuring a runner on the main server is a separate
administrative action; this repository does not do that automatically. AI builds
use a dedicated temporary Buildx builder and clear only its build cache before
packing the images. On ephemeral GitHub runners the workflow first frees unused
SDK space. A larger runner can be selected using the same variable if available
storage is insufficient; a failed build does not start the delivery job.

Configure these secrets in the GitHub `lite` environment:

| Secret | Value |
| --- | --- |
| `TZ_LITE_DEPLOY_HOST` | The Lite host's address, as a bare host or IP |
| `TZ_LITE_DEPLOY_USER` | `tz-deploy` |
| `TZ_LITE_DEPLOY_SSH_KEY` | A dedicated deployment private key |
| `TZ_LITE_SSH_HOST_KEY` | The verified server public key, `ssh-ed25519 …`, without hostname/comment |

Use a new deployment key; the personal `pere` key is only for initial
administration and must not be uploaded to GitHub. The upload account is restricted
to release files and one validated deployment command. It has no interactive shell
or Docker access.

The Lite server is third-party infrastructure. Never copy GitHub tokens, personal
SSH private keys, repository credentials or the deployment private key there.
Only the dedicated deployment **public** key is installed on the server. GitHub
Actions initiates the connection, with SSH agent forwarding disabled; the target
server does not need access to GitHub or the source repository.

## First installation

The commands below are a runbook, not evidence of completed server changes.
Obtain the owner's authorization before executing server changes or publishing
the workflow.
An existing installation with the former namespace must first undergo the
explicit [namespace migration](MIGRATION-tz.md). Provisioning refuses its directories, accounts and database
instead of creating an empty replacement or rotating persisted credentials.

1. Generate a dedicated Ed25519 deployment key and retain its private half outside
   the repository. Package configuration on the local/build machine:

   ```sh
   python3 deploy/lite/package-provision /tmp/tz-lite-install
   ```

2. Transfer only `tz-lite-install.tar.gz`, its checksum and the dedicated
   **public** key to the server using the administrator connection. Verify the
   checksum, extract the configuration bundle into a private temporary directory,
   and run `sudo bash tz-lite-install/provision /absolute/path/deploy-key.pub`.
   This installs runtime packages, creates service users and a separate database,
   generates private credentials, and installs the artifact receiver plus nginx.
   It does not deploy the application or request a TLS certificate.
   Lite pins Docker config digests and uses the classic Docker image store.
   Provisioning disables the containerd image store introduced as the Docker 29
   default. It refuses to switch a containerd store containing images or
   containers, so an existing installation must be reviewed before switching.

3. Confirm the Cloudflare DNS record targets that same Lite host and HTTP ACME
   challenges can reach this origin. Public DNS returns Cloudflare addresses when
   proxying is enabled, so DNS resolution alone cannot verify the origin IP.
   Issue the certificate and enable HTTPS. A contact email is optional:

   ```sh
   sudo /usr/local/sbin/tz-lite-enable-https
   ```

   Use Cloudflare [**Full (strict)**](https://developers.cloudflare.com/ssl/origin-configuration/ssl-modes/full-strict/)
   after the origin certificate is installed.
   The helper retains the old vhost if nginx configuration validation fails.

4. Configure the dedicated GitHub secrets and run **Deploy Lite** for a commit
   that passed CI. Then create the initial administrator with an owner-provided
   email. The compiled helper creates the internal project, current roles, a
   support department and an inbox; migrations are embedded in the binary:

   ```sh
   sudo /srv/tz/current/bin/bootstrap-admin \
     --config /etc/tz/Config.toml \
     --email administrator@example.com \
     --credentials /root/tz-lite-admin-credentials
   ```

   The generated password is saved only in that new mode-0600 file. Repeating a
   successful bootstrap does not rotate it. The helper refuses to repurpose a
   database containing non-demo application records.

The internal UUID is generated once in `/etc/tz/Config.toml`:

```toml
[product]
edition = "lite"
project-id = "<installation UUID>"
```

Rerunning provisioning preserves the UUID, database/Redis credentials and the
application encryption key. Database, attachment and AI state stay outside release
directories. The readable `/etc/tz-lite-installation` marker prevents the build
script from running on this destination and prevents accidental artifact
activation on a standard installation.

## Artifact validation and recovery

The application archive contains `bin/api`, `bin/worker`, `bin/bootstrap-admin`,
three static frontend bundles and a manifest. The receiver verifies the release
SHA, edition, ARM64 ELF headers, archive checksum and every file digest. It rejects
source files, source maps, hidden paths, links, special files and path traversal.
It does not execute code from an archive before validation.

The AI archive contains prebuilt OpenClaw, browser, Firecrawl, Redis, RabbitMQ and
PostgreSQL images plus runtime configuration. Image IDs, ARM64 architecture,
archive paths and file digests are validated before Docker loads them. Runtime
Compose files contain no build sections and forbid image pulls. Firecrawl's
pinned plugin is installed in the OpenClaw image on the builder.
The installer uses `/opt/tz-plugin-build` inside that image, then the complete
managed npm project (including hoisted dependencies) is baked into
`/opt/tz-plugins/firecrawl`. These are image/container paths, not directories
created at `/opt` on the destination host. The temporary installer directory is
removed from the final image filesystem. Host releases live under `/srv/tz`
and `/srv/tz-ai`; persistent state lives under `/var/lib/tz` and
`/var/lib/tz-ai`.

Before activation the receiver validates both archives, stops API/worker writers
and creates a PostgreSQL dump. It starts and smoke-tests the matching AI runtime,
then the API applies embedded migrations; the worker starts only after the release
is ready. An interrupted release leaves the worker blocked across reboots.
On an activation failure the previous application pointer is restored and services
remain stopped for review: a database migration is **not** automatically rolled
back. AI state and Firecrawl's own PostgreSQL volume are also not automatically
rolled back. Inspect the error and `/var/backups/tz` before recovery.

An identical artifact can be retried for the same SHA. A different artifact for an
existing SHA is rejected; publish a new commit for changed build inputs. Successful
releases and database backups are retained for operator-managed cleanup.

## Local checks

```sh
python3 -m unittest discover -s deploy/lite -p 'test_*.py'
bash -n deploy/lite/build-release deploy/lite/tz-lite-deploy deploy/lite/provision
```

Build the application from a clean, committed checkout on the builder:

```sh
bash deploy/lite/build-release "$(git rev-parse HEAD)" /tmp/tz-lite-release
```

No command in this runbook is part of the automatic standard deployment.

## Binary hardening and obfuscation

Lite ships the backend to a customer-controlled server, so the release build
protects the code that ships:

- **Profile `release-hardened`** (`backend/Cargo.toml`) inherits `release` and
  adds `strip`, fat LTO and `codegen-units = 1` — no symbol names, no
  symbolicated backtraces, a smaller binary. Our own hosting build
  (`deploy/server`) stays on plain `--release` and is unaffected.
- **Feature `obfuscate`** turns `crate::obf!("…")` literals into XOR-encoded
  blobs decoded at runtime, so `strings` on the binary does not reveal them.
  Currently applied to the customer-facing safety policies in
  `provider_reply.rs`; wrap more sensitive literals in `obf!` as needed (a
  literal built with `format!` interpolation must keep its `{}` placeholders
  outside the `obf!`). See `backend/src/obfs.rs`.
- The agent-test **runtime revision** hashes compile-time source *digests*
  instead of embedding source text (`obf!` alone cannot help while whole files
  are embedded verbatim). This is gated on the same feature.

`deploy/lite/Dockerfile.release` builds with
`--profile release-hardened --features obfuscate`. Set `TZ_OBFUSCATION_KEY` in
the environment before `build-release` to give a deployment a distinct keystream
(this trades away byte-reproducibility); leaving it unset keeps a fixed,
reproducible keystream.

Limits: obfuscation defeats casual inspection and `strings`, not a determined
reverse engineer on a machine they control (memory dumps, debuggers). It does
**not** protect secrets in `Config.toml` on the customer's server — those need a
different approach (e.g. proxying provider calls so the customer never holds the
raw key).

Verify a hardened build hides a wrapped string:

```sh
cd backend && cargo build --features obfuscate --bin api
strings target/debug/api | grep -c "This application-managed confidentiality"  # -> 0
```
