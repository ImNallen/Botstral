import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { api, errorMessage } from "./api";
import type { SessionView } from "./api";

export function SignIn({
  disabled,
  notice,
  onSuccess,
  onError,
}: {
  disabled: boolean;
  notice: string;
  onSuccess: (session: SessionView) => void;
  onError: (error: unknown) => void;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const pending = useRef<AbortController | null>(null);
  useEffect(() => () => pending.current?.abort(), []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (disabled || submitting) return;
    const controller = new AbortController();
    pending.current = controller;
    setSubmitting(true);
    setError("");
    try {
      const session = await api.signIn(
        { username, password },
        controller.signal,
      );
      if (controller.signal.aborted) return;
      setPassword("");
      onSuccess(session);
    } catch (error) {
      if (controller.signal.aborted) return;
      setPassword("");
      setError(errorMessage(error));
      onError(error);
    } finally {
      if (!controller.signal.aborted) setSubmitting(false);
    }
  }

  return (
    <main className="welcome">
      <section className="welcome-copy">
        <p className="eyebrow">YOUR OWN CORNER OF THE INTERNET</p>
        <h1>
          A little space.
          <br />A lot of possibility.
        </h1>
        <p className="intro">
          A home for your bots, on infrastructure you control.
        </p>
        <div className="welcome-note">
          <span className="small-mark" aria-hidden="true">
            ✳
          </span>
          <p>
            Start with an identity.
            <br />
            Your bot names stay here, ready for what comes next.
          </p>
        </div>
        <p className="scope-note">
          Local manager · Single owner · Task execution is not available yet
        </p>
      </section>
      <section className="signin-card" aria-labelledby="signin-title">
        <p className="eyebrow">WELCOME HOME</p>
        <h2 id="signin-title">Sign in to Botstral</h2>
        <p>Use the owner account for this manager.</p>
        {notice && (
          <p className="notice" role="status">
            {notice}
          </p>
        )}
        <form onSubmit={submit}>
          <label htmlFor="username">Username</label>
          <input
            id="username"
            name="username"
            autoComplete="username"
            autoCapitalize="none"
            spellCheck={false}
            required
            minLength={3}
            maxLength={64}
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            disabled={submitting}
          />
          <label htmlFor="password">Password</label>
          <input
            id="password"
            name="password"
            type="password"
            autoComplete="current-password"
            required
            minLength={12}
            maxLength={256}
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            disabled={submitting}
            aria-describedby={error ? "signin-error" : undefined}
            aria-invalid={Boolean(error)}
          />
          {error && (
            <p className="error" id="signin-error" role="alert">
              {error}
            </p>
          )}
          <button
            className="primary full"
            type="submit"
            disabled={disabled || submitting}
          >
            {submitting ? "Signing in…" : "Sign in"}
          </button>
        </form>
        <p className="setup-hint">
          First visit? Complete owner setup using the separate link printed in
          the manager terminal.
        </p>
      </section>
    </main>
  );
}
