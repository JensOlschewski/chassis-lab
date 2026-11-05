# License Server

`clab-license-server` decides who run `clab` Client and server share token
format via crate `clab-license`.

## Model

- One license = one seat = one installation (random `install_id`). Two ways in:
  - **Automatic:** first start, `clab` calls `register`. Server makes license
    named after hostname (`auto`). User asked nothing.
  - **Manual:** admin runs `create --person`. User binds key with `activate`.
- Machine fingerprint = salted hash of OS machine ID. `revoke` also blocks the
  fingerprint, so delete + reinstall gives no new license. `register` and
  `activate` refuse blocked fingerprints. IP and hostname are only recorded for
  admin. They never decide anything.
- Server answers with signed token. `valid_until` = now + grace (`--grace-days`,
  default 7). Client asks server on every run. Server unreachable: client runs
  until `valid_until`, then fails. Each online run resets the clock.
- `revoke` bites at the client's next online check. Offline client notices only
  after reconnect or after `valid_until`.

## API

All bodies JSON.

| Request               | Body                                                | Success              |
| --------------------- | --------------------------------------------------- | -------------------- |
| `POST /v1/register`   | `{install_id, fingerprint, hostname, version}`      | `200` `{key, token}` |
| `POST /v1/activate`   | `{key, install_id, fingerprint, hostname, version}` | `200` token          |
| `POST /v1/validate`   | `{key, install_id, hostname, version}`              | `200` token          |
| `POST /v1/deactivate` | `{key, install_id}`                                 | `204`                |

`register` from known machine returns its license on new `install_id` and new
key (keys stored hashed only).

Token = `{payload, signature}`. `payload` = JSON text of
`{license_id, licensee, install_id, issued_at, valid_until}` (Unix seconds).
`signature` = base64 Ed25519 over exactly those bytes.

- `403 {"error": code}`: license not usable, client drops token. Codes:
  `invalid_key` (unknown, revoked, blocked machine), `seat_in_use`,
  `not_activated`, `registration_closed`.
- `400` malformed. `429` `rate_limited`: over 20 registrations per address per
  hour.
- Anything else, or no connection: server gave no answer. Client keeps token
  until `valid_until`.

## Client

`clab convert|analyze|compare|inspect` check license first
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
