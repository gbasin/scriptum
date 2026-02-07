import { useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import {
  isShareLinkRedeemable,
  loadShareLinkRecord,
  redeemShareLink,
  sharePermissionLabel,
} from "./share-links";

function formatExpiration(expiresAt: string | null): string {
  if (!expiresAt) {
    return "Never";
  }

  const parsed = new Date(expiresAt);
  if (Number.isNaN(parsed.valueOf())) {
    return "Invalid";
  }
  return parsed.toISOString();
}

export function ShareRedeemRoute() {
  const { shareToken } = useParams<{ shareToken: string }>();
  const [record, setRecord] = useState(() =>
    shareToken ? loadShareLinkRecord(shareToken) : null,
  );
  const [redeemed, setRedeemed] = useState(false);

  const redeemable = useMemo(
    () => (record ? isShareLinkRedeemable(record) : false),
    [record],
  );

  const onRedeem = () => {
    if (!shareToken) {
      return;
    }

    const next = redeemShareLink(shareToken);
    if (!next) {
      return;
    }
    setRecord(next);
    setRedeemed(true);
  };

  if (!record) {
    return (
      <main aria-label="Share link redemption">
        <h1 data-testid="share-redeem-title">Share link redemption</h1>
        <p data-testid="share-redeem-invalid">Share link is invalid.</p>
      </main>
    );
  }

  return (
    <main aria-label="Share link redemption">
      <h1 data-testid="share-redeem-title">Share link redemption</h1>
      <p data-testid="share-redeem-target">
        Target: {record.targetType}/{record.targetId}
      </p>
      <p data-testid="share-redeem-permission">
        Permission: {sharePermissionLabel(record.permission)}
      </p>
      <p data-testid="share-redeem-expiration">
        Expires: {formatExpiration(record.expiresAt)}
      </p>
      <p data-testid="share-redeem-max-uses">
        Max uses: {record.maxUses === null ? "Unlimited" : record.maxUses}
      </p>
      <p data-testid="share-redeem-use-count">Use count: {record.useCount}</p>

      {!redeemable ? (
        <p data-testid="share-redeem-unavailable">
          Share link is no longer redeemable.
        </p>
      ) : (
        <button
          data-testid="share-redeem-submit"
          onClick={onRedeem}
          type="button"
        >
          Redeem link
        </button>
      )}

      {redeemed ? (
        <p data-testid="share-redeem-success">Share link redeemed.</p>
      ) : null}
    </main>
  );
}
