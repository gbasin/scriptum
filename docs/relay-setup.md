# Relay Setup (GitHub OAuth)

This guide covers local relay auth setup from scratch for both Docker and direct `cargo run` workflows.

## 1. Create a GitHub OAuth App

1. Go to GitHub: `Settings -> Developer settings -> OAuth Apps`.
2. Click `New OAuth App`.
3. Use these local development values:
   - Application name: `Scriptum Local` (or any name)
   - Homepage URL: `http://localhost:5173`
   - Authorization callback URL: `http://localhost:8080/v1/auth/oauth/github/callback`
4. Save the app, then note:
   - Client ID
   - Client Secret

## 2. Required environment variables

Use these user-facing variables in local setup:

- `GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET`
- `JWT_SECRET`
- `DATABASE_URL`

Generate `JWT_SECRET`:

```bash
openssl rand -base64 32
```

Relay binary env mapping:

- `GITHUB_CLIENT_ID` -> `SCRIPTUM_RELAY_GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET` -> `SCRIPTUM_RELAY_GITHUB_CLIENT_SECRET`
- `JWT_SECRET` -> `SCRIPTUM_RELAY_JWT_SECRET`
- `DATABASE_URL` -> `SCRIPTUM_RELAY_DATABASE_URL`

## 3. Docker Compose path

1. Copy env template:

```bash
cp docker/.env.example docker/.env
```

2. Edit `docker/.env` and set real OAuth/JWT values.

3. Start relay + Postgres:

```bash
docker compose -f docker/compose.yml up --build
```

4. Verify relay health:

```bash
curl http://localhost:8080/healthz
```

## 4. Direct cargo run path

1. Export required env vars:

```bash
export GITHUB_CLIENT_ID="replace-with-github-client-id"
export GITHUB_CLIENT_SECRET="replace-with-github-client-secret"
export JWT_SECRET="$(openssl rand -base64 32)"
export DATABASE_URL="postgres://scriptum:scriptum@localhost:5432/scriptum"

export SCRIPTUM_RELAY_GITHUB_CLIENT_ID="$GITHUB_CLIENT_ID"
export SCRIPTUM_RELAY_GITHUB_CLIENT_SECRET="$GITHUB_CLIENT_SECRET"
export SCRIPTUM_RELAY_JWT_SECRET="$JWT_SECRET"
export SCRIPTUM_RELAY_DATABASE_URL="$DATABASE_URL"

# Optional runtime config
export RELAY_HOST="0.0.0.0"
export RELAY_PORT="8080"
export LOG_FILTER="info"
export WS_BASE_URL="ws://localhost:8080"
export SHARE_LINK_BASE_URL="http://localhost:3000/share"

export SCRIPTUM_RELAY_HOST="$RELAY_HOST"
export SCRIPTUM_RELAY_PORT="$RELAY_PORT"
export SCRIPTUM_RELAY_LOG_FILTER="$LOG_FILTER"
export SCRIPTUM_RELAY_WS_BASE_URL="$WS_BASE_URL"
export SCRIPTUM_RELAY_SHARE_LINK_BASE_URL="$SHARE_LINK_BASE_URL"
```

2. Start relay:

```bash
cargo run -p scriptum-relay
```

3. Verify relay health:

```bash
curl http://localhost:8080/healthz
```
