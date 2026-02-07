# Deployment Strategy Runbook

This runbook implements the deployment strategy from `SPEC.md` (Part 6, Deployment Strategy).

## Hosted Relay

### Preconditions

1. Feature flags for the release are created and validated.
2. Database migration is additive-only for this release (expand phase first).
3. New relay image tag is published.

### Rollout Sequence

1. Deploy relay to **5% canary**.
2. Observe for **30 minutes**.
3. Promote to **50% rollout**.
4. Observe for **60 minutes**.
5. Promote to **100% rollout**.

### Auto-rollback Gates

Rollback immediately when either condition is true:

1. Crash rate exceeds **2x baseline**.
2. Error rate exceeds **1%**.

The gate logic is implemented in:

- `scripts/release/evaluate_relay_rollout_gate.sh`

And wired into:

- `.github/workflows/deployment-strategy.yml`

## Desktop/CLI Ring Deployment

### Ring Sequence

1. Internal ring
2. Beta ring
3. GA ring

### Kill-switch

Stop promotion and keep the current ring when crash regression exceeds **2x baseline**.

The ring gate logic is implemented in:

- `scripts/release/evaluate_desktop_ring_gate.sh`

And wired into:

- `.github/workflows/deployment-strategy.yml`

## GitHub Actions Trigger

Run workflow manually:

1. Open `Deployment Strategy` workflow.
2. Provide relay image tag and baseline/current crash + error rates.
3. Confirm feature flag readiness input is `true`.
4. Execute workflow.

The workflow uses environment stages (`relay-canary-5`, `relay-rollout-50`, `relay-rollout-100`, and ring environments) so teams can enforce approvals and observation timing.

## Release Workflow Secrets

The `Release` workflow publishes npm packages via `changesets/action` and requires an `NPM_TOKEN` repository secret.

### Configure `NPM_TOKEN`

1. Create an npm automation or granular access token with publish permissions for the `@scriptum` scope.
2. In GitHub, open `Settings` > `Secrets and variables` > `Actions`.
3. Create a new repository secret named `NPM_TOKEN`.
4. Paste the npm token value and save.

### Verify

1. Trigger the `Release` workflow (`workflow_dispatch`) or push to `main`.
2. Confirm the `Validate npm publish credentials` step passes.
3. Confirm package publish step no longer fails with `ENEEDAUTH`.
