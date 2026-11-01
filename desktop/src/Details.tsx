import type { SignedIn, Snapshot } from './domain';
import { connectionText, formatDate } from './format';

export function Details({
  snapshot,
  signedIn,
  managerVersion,
}: {
  snapshot: Snapshot | null;
  signedIn: SignedIn | null;
  managerVersion?: { version: string; result: string } | null;
}) {
  const workers = signedIn?.details.workers.filter((worker) => !worker.revoked) ?? [];
  const ready = workers.filter((worker) => worker.phase === 'ready').length;
  return (
    <>
      <section className="details-section">
        <h3>Connection</h3>
        <dl>
          <dt>Manager</dt>
          <dd>
            <span className={`status-dot ${snapshot?.connection ?? 'unknown'}`} />
            {connectionText(snapshot)}
          </dd>
          <dt>Address</dt>
          <dd className="mono">{snapshot?.endpoint ?? '127.0.0.1:43120'}</dd>
          <dt>Owner session</dt>
          <dd>
            {signedIn
              ? snapshot?.connection === 'online'
                ? 'Authenticated'
                : 'Last authenticated'
              : 'Signed out'}
          </dd>
          {signedIn && (
            <>
              <dt>Session expires</dt>
              <dd>{formatDate(signedIn.expires_at)}</dd>
            </>
          )}
        </dl>
      </section>
      <section className="details-section">
        <h3>Manager capabilities</h3>
        <dl>
          <dt>Workers</dt>
          <dd>{signedIn ? `${ready} ready · ${workers.length} enrolled` : 'Sign in to inspect'}</dd>
          <dt>Vault</dt>
          <dd>
            {signedIn
              ? signedIn.details.vault === 'locked'
                ? 'Locked'
                : 'Unlocked'
              : 'Sign in to inspect'}
          </dd>
          <dt>Mistral provider</dt>
          <dd>
            {signedIn
              ? signedIn.details.provider
                ? 'Configured'
                : 'Not configured'
              : 'Sign in to inspect'}
          </dd>
          <dt>Inference</dt>
          <dd>Not implemented</dd>
          <dt>Remote access</dt>
          <dd>Disabled</dd>
          <dt>Manager version</dt>
          <dd>{managerVersion?.version ?? 'Unavailable'}</dd>
          <dt>Last update</dt>
          <dd>{managerVersion?.result ?? 'No update yet'}</dd>
        </dl>
        <p className="muted small-copy">
          Worker state comes from the manager. Provider metadata does not establish a working model
          connection.
        </p>
      </section>
      <section className="details-section">
        <h3>Coming later</h3>
        <p className="muted">
          Chat, tasks, computer access, connectors, and routines are not available yet.
        </p>
      </section>
    </>
  );
}
