import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';
import type { CredentialUseReview, UseDecision } from '@botstral/link';
import { reviewSchema } from './login-domain';

export function LoginReviews({ online }: { online: boolean }) {
  const [reviews, setReviews] = useState<CredentialUseReview[]>([]);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  useEffect(() => {
    let current = true;
    let pending = false;
    async function refresh() {
      if (!online || pending) return;
      pending = true;
      try {
        const next = z.array(reviewSchema).parse(await invoke<unknown>('credential_uses'));
        if (current) setReviews(next);
      } catch {
        if (current) setReviews([]);
      } finally {
        pending = false;
      }
    }
    if (!online) setReviews([]);
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1000);
    return () => {
      current = false;
      window.clearInterval(timer);
    };
  }, [online]);
  async function decide(review: CredentialUseReview, decision: UseDecision) {
    setBusy(true);
    try {
      await invoke('decide_credential_use', { review, decision });
      setReviews((current) => current.filter((item) => item.id !== review.id));
      setMessage(
        decision === 'deny'
          ? 'Denied. This exact destination and field contract stays denied until its rules are cleared.'
          : 'Decision recorded. The live destination must still match before use.',
      );
    } catch {
      setMessage(
        'The review changed, expired or lost authority. Refresh and review a new request.',
      );
    } finally {
      setBusy(false);
    }
  }
  const pending = reviews.filter(
    (review) => review.state === 'awaiting_approval' && review.expires_at * 1000 > Date.now(),
  );
  return (
    <section aria-label="Protected login reviews">
      {pending.map((review) => (
        <article key={review.id} className="task-card">
          <strong>
            Approve{' '}
            {review.purpose === 'totp' ? 'one current TOTP code' : 'one username and password fill'}
          </strong>
          <p>
            {review.item.metadata.label}. Revision {review.item.revision}. Submit the native login
            form by POST to {review.destination.plan.origin}
            {review.destination.plan.action}.
          </p>
          <p>
            Page: {review.destination.top_origin}
            {review.destination.plan.route}. Frame origin: {review.destination.frame_origin}.
            Expires {new Date(review.expires_at * 1000).toLocaleTimeString()}.
          </p>
          <p>
            {review.destination.certain
              ? 'Supported native form.'
              : 'Uncertain page classification requires this fresh approval.'}{' '}
            Website scripts can read filled values. Existing website sessions remain active after
            Vault revocation.
          </p>
          <details>
            <summary>Exact fields and execution authority</summary>
            <pre>{JSON.stringify(review, null, 2)}</pre>
          </details>
          <div className="action-row">
            <button disabled={busy || !online} onClick={() => void decide(review, 'allow_once')}>
              Allow once
            </button>
            <button disabled={busy || !online} onClick={() => void decide(review, 'deny')}>
              Deny and remember
            </button>
            {review.purpose === 'password' &&
              review.item.metadata.password_policy === 'remembered_origins' &&
              review.destination.certain && (
                <button
                  disabled={busy || !online}
                  onClick={() => void decide(review, 'remember_password')}
                >
                  Remember this password destination
                </button>
              )}
          </div>
        </article>
      ))}
      {message && <p role="status">{message}</p>}
      {reviews.some((review) => review.state !== 'awaiting_approval') && (
        <details>
          <summary>Recent login outcomes</summary>
          {reviews
            .filter((review) => review.state !== 'awaiting_approval')
            .slice(0, 8)
            .map((review) => (
              <p key={review.id}>
                {review.item.metadata.label}: {review.state}.{' '}
                {review.state === 'uncertain'
                  ? 'A release may have reached the browser. It cannot be replayed; inspect the website before starting fresh work.'
                  : review.state === 'applied'
                    ? 'The approved form submission was invoked. Website authentication is a separate result.'
                    : ''}
              </p>
            ))}
        </details>
      )}
    </section>
  );
}
