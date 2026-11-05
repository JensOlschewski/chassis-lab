# License Server

`clab-license-server` decides who run `clab` Client and server share token
format via crate `clab-license`.

## Model

- One license = one seat = one installation (random `install_id`)

Two ways in:

- **Automatic:** first start, `clab` calls `register`. Server makes license
  named after hostname (`auto`). User asked nothing.
- **Manual:** admin runs `create --person`. User binds key with `activate`.

- Server answers with signed token. `valid_until` = now + grace (`--grace-days`,
  default 7). Client asks server on every run. Server unreachable: client runs
  until `valid_until`, then fails. Each online run resets the clock.

- `revoke` bites at the client's next online check. Offline client notices only
  after reconnect or after `valid_until`.

## API

`GET /` is a public status page (`src/index.html`): server up, database ok,
registration accepted or closed, version. No licenses, names or counts.

| Request               | Body                                                | Success              |
| --------------------- | --------------------------------------------------- | -------------------- |
| `POST /v1/register`   | `{install_id, fingerprint, hostname, version}`      | `200` `{key, token}` |
| `POST /v1/activate`   | `{key, install_id, fingerprint, hostname, version}` | `200` token          |
| `POST /v1/validate`   | `{key, install_id, hostname, version}`              | `200` token          |
| `POST /v1/deactivate` | `{key, install_id}`                                 | `204`                |


## Client

These commands are gated with license check.

`clab convert|analyze|compare|inspect`
(`crates/clab-cli/src/license.rs`). `--help`, `--version`, `clab license ...`
need none.

- State: `license.json` in user config dir (`~/.config/clab` on Linux), mode
  `0600`. Holds install ID, key, token.
- No state: register. Offline, this fails with clear error.
- Every run calls `validate`, stores renewed token. `403` removes token, command
  fails: `no valid license: run 'clab license activate <KEY>'`. Next run asks
  server again, so `unrevoke` heals the client.
- `clab license activate <KEY> | status | deactivate`. `status` asks server,
  falls back to stored token offline.
- Offline check uses local clock.

## Administration

Run on server:

```sh
alias licenses='docker exec clab-license clab-license-server --data /data'
licenses create --person "Ada Lovelace"   # manual license; prints key once
licenses list                             # person, state, auto/manual, host, IP, last seen, version
licenses name 3 "Ada Lovelace"            # name an automatic license
licenses revoke 3                         # stop it, block its machine
licenses unrevoke 3                       # undo revoke: license works, machine unblocked
licenses unbind 3                         # free seat after machine change
licenses delete 3                         # remove license made by mistake; prefer revoke
```

Keys stored as SHA-256 hashes, never shown again. Machine block belongs to
license id and survives `delete`; `unrevoke <id>` before deleting lifts it.
`serve --open-registration=false` stops new registrations; existing
installations keep working.

## Data

Per license the server stores: person or hostname, last reported hostname,
fingerprint (salted hash, not machine ID), IP of last request only (from
`X-Forwarded-For`, set by Traefik), first and last seen, `clab` version.
Personal data: tell users what `clab` sends. Nothing about their files or
results is sent.

Database migrates itself on start (`PRAGMA user_version`). Rows kept.

## Deployment

State in `/srv/docker/clab-license/data`: `licenses.db`, `signing.key`. Clients
embed the matching public key. Lost or replaced `signing.key` invalidates every
installed client. Back up both files.

First deployment, from repo root:

```sh
git archive HEAD | ssh mariecolette.de \
  'docker build -t clab-license -f crates/clab-license-server/Dockerfile -'
ssh mariecolette.de 'mkdir -p /srv/docker/clab-license/data'
scp crates/clab-license-server/docker-compose.yml mariecolette.de:/srv/docker/clab-license/
ssh mariecolette.de 'cd /srv/docker/clab-license &&
  docker run --rm -v "$PWD/data:/data" clab-license keygen &&
  docker compose up -d'
```

`keygen` prints the public key for the client. Update = same `docker build`,
then `docker compose up -d`.

Nightly database backup (cron on host):

```sh
sqlite3 /srv/docker/clab-license/data/licenses.db \
  ".backup '/srv/backups/clab-license/licenses-$(date +%F).db'"
```
