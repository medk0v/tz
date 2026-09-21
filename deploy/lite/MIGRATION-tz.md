# Existing Lite installation: migrate the runtime namespace to `tz`

This is an operator runbook, **not an executed migration**. Every production
connection and the mutation window require the owner's explicit approval. No
command below authorizes a commit, deployment or secret change by itself. Read
the whole runbook and prepare the new, CI-verified artifacts before the window.
Do not paste these sections into one unattended script.

The intended target is the dedicated Lite host `65.109.95.170`, domain
`tz.cyber-money.org`, Ubuntu 26.04 ARM64. Verify it again at execution time. Never
run this procedure on the standard installation. The old names below identify
existing resources to migrate or archive; the new active runtime uses `tz`.

The migration preserves the project UUID, PostgreSQL data, application encryption
key and key version, database/Redis passwords, OpenClaw token, Firecrawl password,
numeric UID/GID ownership, attachments, OpenClaw auth/config and support workspace.
It does not synchronize the application's stored AI provider token or enable
agent auto-join. Those settings must be verified separately after migration.

## 1. Prepare outside production

1. Commit only after approval and wait for successful CI for that exact commit.
   Build matching ARM64 application and AI archives. Retain their complete SHA,
   both checksums, and the new configuration-only install archive. Do not send
   source code, repository credentials or private deployment keys to the server.
2. Download/stage artifacts without invoking either deployment receiver. The old
   receiver accepts the old artifact prefix, paths and services; it cannot perform
   this migration. The new receiver cannot run until migration and provisioning
   are complete. Keep **Deploy Lite** paused during the maintenance window.
3. Prepare the dedicated GitHub `lite` environment entries `TZ_LITE_DEPLOY_HOST`,
   `TZ_LITE_DEPLOY_USER=tz-deploy`, `TZ_LITE_DEPLOY_SSH_KEY` and
   `TZ_LITE_SSH_HOST_KEY`; migrate `TZ_LITE_BUILD_RUNNER` if a custom runner is
   configured. Copy existing secret values inside the authorized secret manager;
   do not display or rotate them. The private key stays outside production.
   Keep the old entries available for rollback until acceptance, then delete them.
4. Plan an outage and keep a separate administrator SSH session open. The upload
   account cannot administer the server. Reserve disk for a frozen state backup,
   the database dump, new AI images, and a second copy of each persistent Docker
   volume. Backups need a restorable copy outside the destination host as well.

## 2. Preflight and private evidence directory

After approval, use a root administrative shell with tracing disabled. Creating
the private evidence directory writes to the host; the checks themselves are
read-only. `MIGRATION_BACKUP` is a
new private directory outside all active runtime paths; never reuse an earlier
migration directory. A real artifact SHA must replace the placeholder below.

```bash
set -euo pipefail
set +x
umask 077
MIGRATION_SHA='<full 40-character CI-verified SHA>'
[[ "$MIGRATION_SHA" =~ ^[0-9a-f]{40}$ ]]
MIGRATION_INPUT=/root/tz-migration-input
MIGRATION_BUNDLE="$MIGRATION_INPUT/tz-lite-install"
MIGRATION_BACKUP="/root/tz-migration/$(date -u +%Y%m%dT%H%M%SZ)"
[[ ! -e "$MIGRATION_BACKUP" ]]
install -d -m 0700 "$MIGRATION_BACKUP"

[[ "$(uname -m)" == aarch64 ]]
[[ "$(cat /etc/tzomet-lite-installation)" == tz.cyber-money.org ]]
[[ -f /etc/tzomet/Config.toml && ! -L /etc/tzomet/Config.toml ]]
[[ -L /srv/tzomet/current && -L /srv/tzomet-ai/current ]]
for path in /etc/tz /etc/tz-lite-installation /srv/tz /srv/tz-ai \
    /var/lib/tz /var/lib/tz-ai /var/lib/tz-deploy; do
    [[ ! -e "$path" && ! -L "$path" ]]
done
! getent passwd tz >/dev/null
! getent passwd tz-deploy >/dev/null
! getent group tz >/dev/null
! getent group tz-deploy >/dev/null
! getent group tz-openclaw >/dev/null
[[ "$(id -gn tzomet)" == tzomet ]]
[[ "$(id -Gn tzomet-deploy)" == tzomet-deploy ]]

readlink -f /srv/tzomet/current > "$MIGRATION_BACKUP/application-current"
readlink -f /srv/tzomet-ai/current > "$MIGRATION_BACKUP/ai-current"
getent passwd tzomet tzomet-deploy > "$MIGRATION_BACKUP/passwd-records"
getent group tzomet tzomet-deploy tzomet-openclaw > "$MIGRATION_BACKUP/group-records"
systemctl is-enabled tzomet-api.service tzomet-worker.service tzomet-redis.service \
    > "$MIGRATION_BACKUP/service-enable-state"
systemctl cat tzomet-api.service tzomet-worker.service tzomet-redis.service \
    > "$MIGRATION_BACKUP/service-units"
```

Stop if any preflight fails. Verify the captured release paths are real directories
under their expected `releases/<SHA>` trees; record both complete old SHAs.
Confirm `/etc/tzomet/Config.toml` has `product.edition = "lite"`, a valid fixed
project UUID, the expected domain, and the existing database/Redis endpoints.
Use a parser that prints only pass/fail, not the config or credential URLs.
Ensure no standard deployment helpers are installed and no deployment is running.
Do not infer the current SHA from a previous conversation.

Inspect PostgreSQL without printing passwords. The old database and role must
exist, the database owner must be the old application role, and `tz` must not
already exist as a database or role. Check role privileges and external sessions.
Record database size, representative business row counts, and project UUID in a
private preflight report for comparison after activation.

Discover actual Docker resources **before stopping/removing containers**. Do not
assume the browser volume name. Capture only mount/image metadata; full `docker
inspect` and interpolated Compose output contain secrets and must never be pasted
into a task or terminal transcript.

```bash
OLD_AI_RELEASE="$(cat "$MIGRATION_BACKUP/ai-current")"
OLD_PG_CONTAINER="$(docker ps -aq \
    --filter label=com.docker.compose.project=tzomet-firecrawl \
    --filter label=com.docker.compose.service=nuq-postgres)"
OLD_BROWSER_CONTAINER="$(docker ps -aq \
    --filter label=com.docker.compose.project=tzomet-openclaw \
    --filter label=com.docker.compose.service=browser-runner)"
[[ "$OLD_PG_CONTAINER" =~ ^[0-9a-f]{12,64}$ ]]
[[ "$OLD_BROWSER_CONTAINER" =~ ^[0-9a-f]{12,64}$ ]]
OLD_PG_VOLUME="$(docker inspect --format \
    '{{range .Mounts}}{{if eq .Type "volume"}}{{println .Name}}{{end}}{{end}}' \
    "$OLD_PG_CONTAINER")"
OLD_BROWSER_VOLUME="$(docker inspect --format \
    '{{range .Mounts}}{{if eq .Type "volume"}}{{println .Name}}{{end}}{{end}}' \
    "$OLD_BROWSER_CONTAINER")"
[[ "$OLD_PG_VOLUME" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]+$ ]]
[[ "$OLD_BROWSER_VOLUME" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]+$ ]]
docker inspect --format '{{json .Mounts}}' "$OLD_PG_CONTAINER" \
    > "$MIGRATION_BACKUP/firecrawl-mounts.json"
docker inspect --format '{{json .Mounts}}' "$OLD_BROWSER_CONTAINER" \
    > "$MIGRATION_BACKUP/browser-mounts.json"
docker inspect --format '{{.Image}}' "$OLD_PG_CONTAINER" \
    > "$MIGRATION_BACKUP/firecrawl-postgres-image"
printf '%s\n' "$OLD_PG_VOLUME" > "$MIGRATION_BACKUP/firecrawl-volume-name"
printf '%s\n' "$OLD_BROWSER_VOLUME" > "$MIGRATION_BACKUP/browser-volume-name"
docker volume inspect "$OLD_PG_VOLUME" "$OLD_BROWSER_VOLUME" \
    > "$MIGRATION_BACKUP/volume-metadata.json"
```

Expected old Firecrawl volume: `tzomet-firecrawl_firecrawl-postgres`. A different
name requires reconciling it with the captured mounts and old Compose model.
Ensure there are no additional persistent or anonymous mounts in either stack;
include any discovered ones in the backup and reviewed copy plan. Capture network
names, aliases and driver options. The managed stacks use bridge networking and
an external shared network; unexpected custom network settings require review.

Verify checksums and validate both **new** artifacts using the validators from the
new trusted install bundle. Extract only to new private staging directories, not
to `/srv`. Validate ARM64, edition, full SHA and file/image hashes. Read the new
Compose files from those validated artifacts and confirm project names
`tz-openclaw`/`tz-firecrawl`, shared network `tz-ai`, volume keys
`browser-runtime`/`firecrawl-postgres`, and preserved mount destinations. Check
Firecrawl PostgreSQL's major version is compatible with the old volume. Loading
verified images is allowed only in the approved mutation window; no image pulls
or on-server builds are part of this procedure.

## 3. Freeze writers and create restorable backups

Acquire the old application and AI receiver locks for the full migration. Keep
this shell open. Do not acquire the new lock names: the new receiver needs them.

```bash
exec 9>/run/lock/tzomet-lite-deploy.lock
flock -n 9
exec 8>/run/lock/tzomet-lite-ai-deploy.lock
flock -n 8
install -d -m 0700 /var/lib/tzomet/deployment
printf '%s\n' namespace-migration > /var/lib/tzomet/deployment/worker-blocked
systemctl disable --now tzomet-worker.service tzomet-api.service
! systemctl is-active --quiet tzomet-worker.service
! systemctl is-active --quiet tzomet-api.service
systemctl stop nginx.service

"$OLD_AI_RELEASE/openclaw/manage.sh" stop
"$OLD_AI_RELEASE/firecrawl/manage.sh" stop
[[ -z "$(docker ps -q --filter label=com.docker.compose.project=tzomet-openclaw)" ]]
[[ -z "$(docker ps -q --filter label=com.docker.compose.project=tzomet-firecrawl)" ]]
systemctl disable --now tzomet-redis.service
runuser -u postgres -- pg_dump --format=custom tzomet \
    > "$MIGRATION_BACKUP/application.dump"
[[ -s "$MIGRATION_BACKUP/application.dump" ]]
pg_restore --list "$MIGRATION_BACKUP/application.dump" >/dev/null
```

Never use `docker compose down -v`, `docker volume prune`, `docker system prune`
or a broad image prune. The old runtime's `stop` operation removes containers and
networks without deleting volumes. Ensure no other container still mounts either
volume. Do not reboot during this partial migration; old units are disabled and
new units will not have an executable `current` release until activation.

Back up the frozen persistent host state, preserving numeric ownership, ACLs,
extended attributes, symlinks and permissions. Include the dedicated public SSH
key and active infrastructure config. Back up PostgreSQL role metadata privately
if needed for rollback; it may contain password hashes.

```bash
tar --numeric-owner --acls --xattrs --sparse -cpf \
    "$MIGRATION_BACKUP/host-state.tar" -C / \
    etc/tzomet var/lib/tzomet var/lib/tzomet-ai var/lib/tzomet-deploy
tar -tf "$MIGRATION_BACKUP/host-state.tar" >/dev/null
sha256sum "$MIGRATION_BACKUP/application.dump" "$MIGRATION_BACKUP/host-state.tar" \
    > "$MIGRATION_BACKUP/backup.sha256"
```

Back up each verified local Docker volume with the containers stopped. The
commands below require ordinary local volumes with empty driver options; stop
for a plugin, bind-backed, remote or unusual volume instead of accessing it this
way. Socket files in the browser volume are transient and are recreated on start.

```bash
for volume in "$OLD_PG_VOLUME" "$OLD_BROWSER_VOLUME"; do
    [[ "$(docker volume inspect --format '{{.Driver}}' "$volume")" == local ]]
    [[ "$(docker volume inspect --format '{{json .Options}}' "$volume")" == null \
        || "$(docker volume inspect --format '{{json .Options}}' "$volume")" == '{}' ]]
    [[ -z "$(docker ps -aq --filter "volume=$volume")" ]]
    mountpoint="$(docker volume inspect --format '{{.Mountpoint}}' "$volume")"
    [[ -d "$mountpoint" && ! -L "$mountpoint" ]]
    tar --numeric-owner --acls --xattrs --sparse -cpf \
        "$MIGRATION_BACKUP/$volume.tar" -C "$mountpoint" .
    tar -tf "$MIGRATION_BACKUP/$volume.tar" >/dev/null
    sha256sum "$MIGRATION_BACKUP/$volume.tar" >> "$MIGRATION_BACKUP/backup.sha256"
done
```

Verify usable off-host backups before proceeding. Keep old releases and their
`images.tar` files for rollback; do not edit their manifests or repackage them.

## 4. Remove legacy runtime entry points from active locations

Archive old release trees and service/helper configuration beneath the private
backup, preserving relative paths. Do not move old releases into `/srv/tz` or
rewrite them: their binaries, paths, manifests and receiver format are mutually
dependent. A new `/srv/tz/current` must not point to an old release.

```bash
archive_legacy_path() {
    local path="$1"
    if [[ -e "$path" || -L "$path" ]]; then
        install -d -m 0700 "$(dirname "$MIGRATION_BACKUP/legacy-system$path")"
        mv -- "$path" "$MIGRATION_BACKUP/legacy-system$path"
    fi
}
for path in /srv/tzomet /srv/tzomet-ai /var/backups/tzomet \
    /usr/local/lib/tzomet /usr/local/share/tzomet-lite \
    /usr/local/sbin/tzomet-lite-deploy /usr/local/sbin/tzomet-lite-ai-deploy \
    /usr/local/sbin/tzomet-lite-github-ssh /usr/local/sbin/tzomet-lite-enable-https \
    /etc/systemd/system/tzomet-api.service /etc/systemd/system/tzomet-worker.service \
    /etc/systemd/system/tzomet-redis.service /etc/tmpfiles.d/tzomet-ai-runtime.conf \
    /etc/redis/tzomet.conf /etc/redis/tzomet-users.acl \
    /etc/systemd/system/clamav-daemon.socket.d/tzomet-tcp.conf \
    /etc/nginx/conf.d/tzomet-lite-global.conf \
    /etc/nginx/snippets/tzomet-lite-proxy.conf \
    /etc/nginx/snippets/tzomet-lite-cloudflare-real-ip.conf \
    /etc/nginx/sites-enabled/tzomet-lite /etc/nginx/sites-available/tzomet-lite \
    /etc/ssh/sshd_config.d/tzomet-lite-deploy.conf \
    /etc/sudoers.d/tzomet-lite-deploy \
    /etc/letsencrypt/renewal-hooks/deploy/tzomet-lite-nginx \
    /etc/tzomet-lite-installation; do
    archive_legacy_path "$path"
done
systemctl daemon-reload
```

Reconcile any discovered drop-ins, timers, cron entries and enablement symlinks
with this inventory. Archive the relevant Lite entries, never unrelated services.
The standard helper names `tzomet-deploy`, `tzomet-ai-deploy`,
`tzomet-release-deploy`, and `tzomet-github-ssh` are not expected on Lite; stop if
present. Do not rename them to bypass the provisioning guard.

The old account remains blocked from release delivery because its helper is gone;
do not reload SSH until the new forced command is installed and validated. Keep
the administrator connection open. Remove inactive `/run/tzomet-ai` and
`/run/tzomet-redis` transient directories after confirming their users/containers
are stopped; do not migrate live grants, actions, sockets or lock files to new
runtime directories. Close and remove the old lock files only at the very end.

## 5. Rename accounts, persisted paths and PostgreSQL identities

Check again that target accounts, groups and paths are absent. Rename accounts
and groups without `useradd`, `groupadd`, recursive `chown`, or `usermod -m`.
Numeric ownership must remain identical to the saved preflight records.

```bash
usermod --login tz --home /var/lib/tz tzomet
groupmod --new-name tz tzomet
usermod --login tz-deploy --home /var/lib/tz-deploy tzomet-deploy
groupmod --new-name tz-deploy tzomet-deploy
groupmod --new-name tz-openclaw tzomet-openclaw
mv /etc/tzomet /etc/tz
mv /var/lib/tzomet /var/lib/tz
mv /var/lib/tzomet-ai /var/lib/tz-ai
mv /var/lib/tzomet-deploy /var/lib/tz-deploy
```

Validate that all writers and old application connections are gone before
renaming PostgreSQL. Terminate only verified stale connections to the old Lite
database if necessary. Run these statements connected to `postgres`, not to the
database being renamed, and outside a transaction:

```sql
ALTER DATABASE tzomet RENAME TO tz;
ALTER ROLE tzomet RENAME TO tz;
```

Do **not** create a replacement database or restore into an empty one as part of
the normal rename. Ownership and grants follow the PostgreSQL role OID. Renaming
a role can invalidate an MD5 password hash; the subsequent provisioning step
reapplies the **same saved password** to `tz`. Preserve the saved password file
and keep the service stopped until then. Do not emit passwords in SQL arguments,
shell history or logs.

Rename `/etc/tz/tzomet.env` to `/etc/tz/tz.env` and change only the exact variable
name `TZOMET_SECRETS_KEY=` to `TZ_SECRETS_KEY=`. Verify the value bytes are
unchanged. In `Config.toml`, parse TOML and update only these known fields while
retaining every unrelated setting:

| Field | New value |
| --- | --- |
| `pg.url` | Existing URL with username/database `tz`; unchanged password/host/port |
| `redis.url` | Existing URL with ACL username `tz`; unchanged password/host/port |
| `secrets.encryption-key-env` | `TZ_SECRETS_KEY` |
| `attachments.storage-path` | `/var/lib/tz/attachments` |
| `geoip.city-database-path` | `/var/lib/tz/geoip/GeoLite2-City.mmdb` |
| `geoip.country-database-path` | `/var/lib/tz/geoip/GeoLite2-Country.mmdb` |
| `openclaw.grant-directory` | `/run/tz-ai/openclaw/agent-secrets` |
| `openclaw.action-directory` | `/run/tz-ai/openclaw/agent-actions` |
| `logging.filter` | `info,tower_http=info` |

Use a reviewed field-specific edit, not a global substitution over files or
database contents. Preserve `product.project-id`, `secrets.key-version`, tokens,
encrypted data and schema migration checksums. Compare secret values privately
with the frozen backup and report only equality. Never regenerate credentials.
Restore ownership/modes to `root:tz`, `0640` for the four application config/secret
files; OpenClaw and Firecrawl `.env` files stay `root:root`, `0600`.

Keep the complete OpenClaw `runtime/state`, `runtime/workspace` and `runtime/auth`
trees, including support sessions and auth profiles. The support agent ID remains
`support` and its workspace remains `workspace-support`; container paths under
`/home/node/.openclaw` are unchanged. Update only the three known host-directory
variables in the OpenClaw `.env` to `/run/tz-ai/openclaw/{agent-secrets,agent-actions,shell}`
and retain the numeric `OPENCLAW_GRANT_GID`. The new receiver reapplies these values.
Preserve `OPENCLAW_GATEWAY_TOKEN`, `OPENCLAW_TZ`, and all unrelated env values.

Inspect OpenClaw's structured configuration for obsolete operational references:
old plugin-load paths, old wrapper approval patterns, and inactive old agent IDs
or workspaces. Preserve an original copy in the root-only backup. Archive inactive
old agent/workspace trees outside active state and remove only their corresponding
config/approval entries; do not overwrite or merge an existing support agent.
The new manager installs `/opt/tz-plugins/firecrawl`, reconciles `support-*`
approvals and rewrites its managed policy fields. Preserve active support auth,
sessions, provider settings and user content. Historical conversation text is
data, not a configuration namespace; do not search-and-replace it. If only an old
agent exists, prepare and review an explicit agent migration before continuing.

Run the new bundle's `render-config.py` against the renamed directories to
validate credential/config agreement and preservation. A validation failure is a
stop condition; do not delete config to make provisioning generate new secrets.

```bash
python3 "$MIGRATION_BUNDLE/render-config.py" \
    --template "$MIGRATION_BUNDLE/Config.lite.toml.template" \
    --directory /etc/tz --openclaw-directory /var/lib/tz-ai/openclaw
```

## 6. Restore persistent Docker volumes under the new names

The new Compose project names would otherwise create empty volumes. Create and
populate the new volumes **before any new stack starts**. Names below must agree
with the validated new Compose model; no replacement for the existing
Firecrawl `.env` may be generated.

```bash
! docker volume inspect tz-firecrawl_firecrawl-postgres >/dev/null 2>&1
! docker volume inspect tz-openclaw_browser-runtime >/dev/null 2>&1
docker volume create --label com.docker.compose.project=tz-firecrawl \
    --label com.docker.compose.volume=firecrawl-postgres \
    tz-firecrawl_firecrawl-postgres >/dev/null
docker volume create --label com.docker.compose.project=tz-openclaw \
    --label com.docker.compose.volume=browser-runtime \
    tz-openclaw_browser-runtime >/dev/null

restore_volume() {
    local source_volume="$1" target_volume="$2" mountpoint
    mountpoint="$(docker volume inspect --format '{{.Mountpoint}}' "$target_volume")"
    [[ -d "$mountpoint" && ! -L "$mountpoint" ]]
    [[ -z "$(find "$mountpoint" -mindepth 1 -maxdepth 1 -print -quit)" ]]
    tar --numeric-owner --acls --xattrs --same-owner -xpf \
        "$MIGRATION_BACKUP/$source_volume.tar" -C "$mountpoint"
}
restore_volume "$OLD_PG_VOLUME" tz-firecrawl_firecrawl-postgres
restore_volume "$OLD_BROWSER_VOLUME" tz-openclaw_browser-runtime
```

Compare persisted file counts, sizes and checksums against the frozen source,
excluding transient Unix sockets, before allowing a container to mount the new
volumes. Confirm Firecrawl `PG_VERSION` matches the new image's major version.
The first new startup must see the restored database, not execute first-time
database initialization. Preserve Redis/RabbitMQ or other extra persistent mounts
if the earlier inventory discovered any; the standard Lite bundle declares only
the volumes shown here.

Keep old volumes untouched until acceptance. The new manager creates `tz-ai` and
new project networks; recreate any separately reviewed custom options explicitly.
Remove the old external `tzomet-ai` network only after verifying it has zero
endpoints. Do not connect new services to old networks as a shortcut.

## 7. Provision the renamed installation, then activate one matching release

At this point there must be no old active directories, accounts, groups, database
role/database, units or standard helpers detected by `provision`. Do not weaken
its legacy guard. Create the new marker only after the preserved config, state,
database and user identities have all passed the checks above. Existing config
plus existing `tz` database are required; there must be no `current` symlinks yet.

```bash
[[ -f /etc/tz/Config.toml && -f /etc/tz/tz.env ]]
[[ -f /var/lib/tz-ai/firecrawl/.env ]]
[[ ! -e /srv/tz/current && ! -L /srv/tz/current ]]
[[ ! -e /srv/tz-ai/current && ! -L /srv/tz-ai/current ]]
printf '%s\n' tz.cyber-money.org > /etc/tz-lite-installation
chown root:root /etc/tz-lite-installation
chmod 0644 /etc/tz-lite-installation
bash "$MIGRATION_BUNDLE/provision"
```

Provisioning reuses the preserved public deployment key, project UUID and all
saved passwords. It reapplies the database password, installs `tz` units and
helpers, recreates transient runtime directories, and starts Redis/ClamAV/nginx.
API/worker units are enabled but have no application release to start; the worker
block remains present. The public site remains unavailable until activation.
Validate the renamed SSH forced command and sudoers with `sshd -t` and `visudo`
and test the dedicated key from a separate connection before ending the admin
session. It must allow only the new artifact names and one deployment command.

Move only the validated new artifacts into `/var/lib/tz/incoming` with owner
`tz-deploy:tz-deploy` and mode `0600`. Do not rename old archives to the new prefix.
Recheck saved secret equality, database owner/data, numeric account IDs, and the
copied volumes. Then invoke the new application receiver with the exact SHA:

```bash
for suffix in .tar.gz .tar.gz.sha256; do
    install -o tz-deploy -g tz-deploy -m 0600 \
        "$MIGRATION_INPUT/tz-lite-${MIGRATION_SHA}${suffix}" \
        "/var/lib/tz/incoming/tz-lite-${MIGRATION_SHA}${suffix}"
    install -o tz-deploy -g tz-deploy -m 0600 \
        "$MIGRATION_INPUT/tz-lite-ai-${MIGRATION_SHA}${suffix}" \
        "/var/lib/tz/incoming/tz-lite-ai-${MIGRATION_SHA}${suffix}"
done
/usr/local/sbin/tz-lite-deploy "$MIGRATION_SHA"
```

The receiver validates both archives, keeps writers stopped while backing up the
renamed database, activates the new AI stack, runs its health checks, then starts
the matching API and worker. It generates new release `.env` and runtime symlinks
to `/var/lib/tz-ai`; never reuse archived absolute symlinks. Do not change nginx's
trusted `X-Tz-Client-IP` header independently of this API release.

## 8. Acceptance and cleanup

Verify locally and through the public domain:

- `tz-api`, `tz-worker` and `tz-redis` are healthy; both application health ports
  and OpenClaw/Firecrawl readiness succeed. Current symlinks point to the intended
  SHA under `/srv/tz` and `/srv/tz-ai`; no old services or containers are running.
- Application project UUID and business row counts match preflight; attachments
  remain readable. Existing encrypted provider credentials decrypt using the
  same key. OpenClaw auth/support workspace and Firecrawl queue tables/data are
  present, with no first-boot replacement database.
- A real support conversation can attach the selected AI agent and receive an AI
  reply. Check its actual provider token agreement and auto-join configuration;
  namespace migration alone does not correct the previously reported settings.
- Check the named browser login and widget behavior. Session/CSRF cookies become
  `__Host-tz_session` and `__Host-tz_csrf`, so operators must log in again. The
  frontend uses `X-Tz-Project-Id`. The widget uses `window.TzWidget`,
  `tz-widget-loader`, and the `tz` postMessage protocol; replace external snippets
  that invoke `SupportWidget` with newly generated embed snippets. The visitor
  ID is copied once from its old browser key to the `tz` key to retain chat identity.
- Active filenames, service/account/database names, Compose projects/networks,
  container mounts, nginx headers, helper usage text and generated runtime
  configuration use the new namespace. Review filename-only scans and redacted
  structural checks; never dump `.env`, auth files, headers or database secrets.

After acceptance, remove the old unmounted Docker volumes by their **verified
exact names** only after checking the private tar backups again. Remove old empty
networks and individually reviewed old image tags after retaining their immutable
IDs/images archives for rollback. Do not use pruning commands. Remove leftover
old enablement symlinks, cron/drop-in references and temporary migration input
copies after inventory review. Keep the root-only rollback backup under the
retention policy, outside active paths. Close the old locks and remove their
inactive files. Enable **Deploy Lite** with only the new GitHub variable/secret
names after validating its dedicated upload account and host key.

## Rollback boundaries

Before the new receiver starts, rollback is a rename/restore of the frozen old
state and configuration. After the new API or Firecrawl starts, database/schema
or runtime state may have changed: do not point old binaries at the changed data.
Keep all writers stopped and obtain a decision on recovery and any writes made
after the backup. The application receiver intentionally does not roll back SQL.

1. Stop and disable the new API/worker and retain the worker block. Stop new
   OpenClaw/Firecrawl stacks without `-v`, then stop new Redis/nginx. Archive failed
   new runtime/configuration/data for analysis outside active paths; do not mix
   old and new systemd, nginx, SSH or Compose configuration.
2. If no new service wrote data, rename the application database and role back
   while connected to `postgres`, with all sessions stopped. If new migrations or
   writes occurred, restore the validated pre-migration database dump through a
   reviewed recovery procedure; preserve the failed database first and account
   for data written after the snapshot. Reapply the original saved database
   password privately after the role rename, including the MD5-hash case.
3. Restore account/group names and original homes without changing their numeric
   IDs. Restore the frozen `/etc/tzomet` and `/var/lib/tzomet*` state from
   `host-state.tar`, preserving ownership/modes, and restore the archived old
   release trees/configuration to their original paths. Remove the conflicting
   new active units/helpers/sites/marker only after archiving them. Repair and
   verify each original absolute `current`, `.env` and runtime symlink from the
   captured metadata. Do not globally reverse-replace secrets or user data.
4. Reuse untouched old Docker volumes, or recreate the exact old volume names
   and restore the frozen volume archives if cleanup already removed them.
   Restore the old shared network and aliases. Load the verified old image
   archive if necessary, retaining the matching PostgreSQL major version.
   Never attach old Firecrawl to a new volume modified by an incompatible image.
5. Validate old configuration and decryptability using the restored encryption
   key; restore old tmpfiles and reload systemd. Start old Redis, then the old AI
   managers from their restored paths, verify health, start API and confirm data,
   then remove the worker block and start worker. Validate and start nginx and
   reload SSH only after the old forced-command/sudo configuration is restored.
   Verify browser login, widget protocol and a real AI response again.
6. Keep automatic delivery disabled until the recovered state is understood.
   Restore matching GitHub entries only if resuming the old deployment code.
   Report the exact active SHA, restored-backup timestamp and any lost/reconciled
   writes. Neither an old symlink nor a healthy process alone proves recovery.
