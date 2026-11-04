# License Server

`clab-license-server` decides who may run `clab`. It is deployed at
`https://clab.license.mariecolette.de`, as the Docker container `clab-license`
behind Traefik. Client and server share the token format through the
`clab-license` crate.

## Model

- A license has one seat, held by one installation (a random `install_id`).
  There are two ways to get one:
  - **Automatic:** on first start `clab` calls `register`. The server creates a
    license named after the hostname (`auto`), so no one is asked anything.
  - **Manual:** the admin runs `create --person`, and the user binds the key
    with `activate`.
- Every machine has a fingerprint: a salted hash of the OS machine ID. Revoking
  a license also blocks its fingerprint, so deleting and reinstalling `clab`
  does not give a new license. `register` and `activate` refuse blocked
  fingerprints. IP address and hostname are only recorded for the admin; they
  never decide anything.
- The server answers with a signed token. `valid_until` is the end of the grace
  period (`--grace-days`, default 7): a client that cannot reach the server may
  keep running until then. Clients ask again once their token is older than one
  day.
- Revoking a license makes `validate` fail at the client's next online check.

## API

All bodies are JSON.

| Request | Body | Success |
| --- | --- | --- |
| `POST /v1/register` | `{install_id, fingerprint, hostname, version}` | `200` `{key, token}` |
| `POST /v1/activate` | `{key, install_id, fingerprint, hostname, version}` | `200` token |
| `POST /v1/validate` | `{key, install_id, hostname, version}` | `200` token |
| `POST /v1/deactivate` | `{key, install_id}` | `204` |

`register` for a machine that registered before gives it its license back on
the new `install_id` and a new key, because keys are stored only hashed.

A token is `{payload, signature}`. `payload` is the JSON text of `{license_id,
licensee, install_id, issued_at, valid_until}` (Unix seconds) and `signature`
is the base64 Ed25519 signature of exactly those bytes.

`403 {"error": code}` means the license is not usable and the client must drop
its token: `invalid_key` (unknown, revoked, or a blocked machine), `seat_in_use`,
`not_activated`, `registration_closed`. `400` is a malformed request, `429`
(`rate_limited`) means more than 5 registrations from one address in an hour. Anything else (other statuses, no connection)
means only that the server could not answer, so a client keeps its token until
`valid_until`.

## Administration

Run on the server:

```sh
alias licenses='docker exec clab-license clab-license-server --data /data'
licenses create --person "Ada Lovelace"   # manual license; prints the key once
licenses list                             # person, state, auto/manual, host, IP, last seen, version
licenses name 3 "Ada Lovelace"            # name an automatic license
licenses revoke 3                         # stops it and blocks its machine
licenses unblock 3                        # lets that machine register again (new license)
licenses unbind 3                         # frees the seat after a machine change
licenses delete 3                         # removes a license made by mistake; prefer revoke
```

Keys are stored as SHA-256 hashes and cannot be shown again. A machine block
belongs to the license id and survives `delete`; lift it with `unblock <id>`
before deleting. `serve --open-registration=false` stops new registrations;
installations that already have a license keep working.

## Data

The server stores, per license: the person or hostname, the hostname the client
last reported, the machine fingerprint (a salted hash, not the machine ID), the
IP address of the last request (only the last one, from `X-Forwarded-For`, which
Traefik sets), first and last seen, and the `clab` version. This is personal
data; tell users what `clab` sends. Nothing about their files or results is
sent.

The database migrates itself on start (`PRAGMA user_version`); existing rows are
kept.

## Deployment

The state lives in `/srv/docker/clab-license/data`: `licenses.db` and
`signing.key`. Clients embed the matching public key, so a lost or replaced
`signing.key` invalidates every installed client. Back up both files.

First deployment, from the repository root:

```sh
git archive HEAD | ssh mariecolette.de \
  'docker build -t clab-license -f crates/clab-license-server/Dockerfile -'
ssh mariecolette.de 'mkdir -p /srv/docker/clab-license/data'
scp crates/clab-license-server/docker-compose.yml mariecolette.de:/srv/docker/clab-license/
ssh mariecolette.de 'cd /srv/docker/clab-license &&
  docker run --rm -v "$PWD/data:/data" clab-license keygen &&
  docker compose up -d'
```

`keygen` prints the public key for the client. Updating is the same `docker
build` followed by `docker compose up -d`.

Nightly backup of the database (cron on the host):

```sh
sqlite3 /srv/docker/clab-license/data/licenses.db \
  ".backup '/srv/backups/clab-license/licenses-$(date +%F).db'"
```
