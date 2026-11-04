# License Server

`clab-license-server` decides who may run `clab`. It is deployed at
`https://clab.license.mariecolette.de`, as the Docker container `clab-license`
behind Traefik. Client and server share the token format through the
`clab-license` crate.

## Model

- A license belongs to a person and has one seat. `activate` binds the seat to
  an installation (a random `install_id`) and records its hostname.
- The server answers with a signed token. `valid_until` is the end of the grace
  period (`--grace-days`, default 7): a client that cannot reach the server may
  keep running until then. Clients ask again once their token is older than one
  day.
- Revoking a license makes `validate` fail at the client's next online check.

## API

All bodies are JSON.

| Request | Body | Success |
| --- | --- | --- |
| `POST /v1/activate` | `{key, install_id, hostname}` | `200` token |
| `POST /v1/validate` | `{key, install_id}` | `200` token |
| `POST /v1/deactivate` | `{key, install_id}` | `204` |

A token is `{payload, signature}`. `payload` is the JSON text of `{license_id,
licensee, install_id, issued_at, valid_until}` (Unix seconds) and `signature`
is the base64 Ed25519 signature of exactly those bytes.

`403 {"error": code}` means the license is not usable and the client must drop
its token: `invalid_key` (unknown or revoked), `seat_in_use`, `not_activated`.
`400` is a malformed request. Anything else (other statuses, no connection)
means only that the server could not answer, so a client keeps its token until
`valid_until`.

## Administration

Run on the server:

```sh
alias licenses='docker exec clab-license clab-license-server --data /data'
licenses create --person "Ada Lovelace"   # prints the key once
licenses list
licenses revoke 3
licenses unbind 3                         # frees the seat after a machine change
```

Keys are stored as SHA-256 hashes and cannot be shown again.

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
