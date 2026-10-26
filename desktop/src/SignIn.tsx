import { useState } from 'react';
import { ApplicationController } from './application';
import type { Snapshot } from './domain';
import { Icon } from './Icon';

export function SignIn({
  controller,
  busy,
  signingOut,
  initial,
  snapshot,
}: {
  controller: ApplicationController;
  busy: boolean;
  signingOut: boolean;
  initial: boolean;
  snapshot: Snapshot | null;
}) {
  const [endpoint, setEndpoint] = useState(snapshot?.endpoint ?? '127.0.0.1:43120');
  const [password, setPassword] = useState('');
  const [validation, setValidation] = useState<string | null>(null);
  return (
    <section className="sign-in">
      <h1>Sign in to Botstral</h1>
      <p>Connect to the manager on this computer.</p>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          const submittedPassword = password;
          setPassword('');
          const bytes = new TextEncoder().encode(submittedPassword).length;
          if (bytes < 16 || bytes > 1024) {
            setValidation('Use your manager password, between 16 and 1,024 bytes.');
            return;
          }
          setValidation(null);
          void controller.signIn({ endpoint, password: submittedPassword });
        }}
      >
        <label htmlFor="manager-address">Manager address</label>
        <input
          id="manager-address"
          autoComplete="off"
          spellCheck={false}
          value={endpoint}
          onChange={(event) => setEndpoint(event.target.value)}
          placeholder="127.0.0.1:43120"
          required
          disabled={busy || signingOut}
        />
        <label htmlFor="owner-password">Owner password</label>
        <input
          id="owner-password"
          type="password"
          autoComplete="off"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          required
          disabled={busy || signingOut}
        />
        {validation && (
          <p className="field-error" role="alert">
            {validation}
          </p>
        )}
        <button className="primary-button" type="submit" disabled={busy || signingOut}>
          {signingOut
            ? 'Clearing this device…'
            : busy
              ? initial
                ? 'Checking manager…'
                : 'Connecting…'
              : 'Sign in'}
          <Icon name="chevron" size={15} />
        </button>
      </form>
      <p className="sign-in-note">
        Use the owner you set up in the local manager. Your password and session stay in memory on
        this device.
      </p>
      <span className="local-label">
        <span className={`status-dot ${snapshot?.connection ?? 'unknown'}`} />
        Local connection only
      </span>
    </section>
  );
}
