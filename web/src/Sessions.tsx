import { useEffect, useRef, useState } from "react";
import { api, errorMessage, formatDate } from "./api";
import type { CSRFToken, Load, SessionItem } from "./api";

export function Sessions({
  csrf,
  connected,
  refreshKey,
  onError,
  onReachable,
  onCurrentRevoked,
}: {
  csrf: CSRFToken;
  connected: boolean;
  refreshKey: number;
  onError: (error: unknown) => void;
  onReachable: () => void;
  onCurrentRevoked: () => void;
}) {
  const [sessions, setSessions] = useState<Load<SessionItem[]>>({
    kind: "loading",
  });
  const [selected, setSelected] = useState<SessionItem | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [reload, setReload] = useState(0);
  const generation = useRef(0);
  const mutation = useRef<AbortController | null>(null);
  const confirmButton = useRef<HTMLButtonElement | null>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  useEffect(
    () => () => {
      generation.current++;
      mutation.current?.abort();
    },
    [],
  );
  useEffect(() => {
    if (!selected) return;
    if (document.activeElement instanceof HTMLElement)
      previousFocus.current = document.activeElement;
    confirmButton.current?.focus();
    return () => previousFocus.current?.focus();
  }, [selected]);
  useEffect(() => {
    if (!connected) return;
    const controller = new AbortController();
    const current = generation.current;
    api
      .listSessions(controller.signal)
      .then((value) => {
        if (controller.signal.aborted || current !== generation.current) return;
        setSessions({ kind: "ready", value });
        onReachable();
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted || current !== generation.current) return;
        setSessions((previous) =>
          previous.kind === "ready"
            ? previous
            : { kind: "failed", message: errorMessage(error) },
        );
        onError(error);
      });
    return () => controller.abort();
  }, [connected, refreshKey, reload, onError, onReachable]);

  async function revoke(target: SessionItem) {
    if (!connected || pending) return;
    const controller = new AbortController();
    mutation.current = controller;
    const current = generation.current;
    setPending(true);
    setError("");
    try {
      await api.revokeSession(target.id, csrf, controller.signal);
      if (controller.signal.aborted || current !== generation.current) return;
      if (target.current) {
        onCurrentRevoked();
        return;
      }
      setSelected(null);
      setSessions((previous) =>
        previous.kind === "ready"
          ? {
              kind: "ready",
              value: previous.value.filter((value) => value.id !== target.id),
            }
          : previous,
      );
      setReload((value) => value + 1);
      onReachable();
    } catch (error) {
      if (controller.signal.aborted || current !== generation.current) return;
      setError(errorMessage(error));
      onError(error);
    } finally {
      if (!controller.signal.aborted && current === generation.current)
        setPending(false);
    }
  }

  return (
    <>
      <div className="page-heading">
        <div>
          <p className="eyebrow">ACCOUNT ACCESS</p>
          <h1>
            Your sessions<span className="accent">.</span>
          </h1>
          <p>See where you are signed in. Revoke access whenever you need.</p>
        </div>
      </div>
      {!connected && (
        <p className="notice" role="status">
          The manager is disconnected. This session list may be out of date.
          Revocation is paused.
        </p>
      )}
      <section className="sessions-panel" aria-label="Active sessions">
        <div className="section-heading">
          <h2>Active sessions</h2>
          <button
            className="secondary compact"
            onClick={() => setReload((value) => value + 1)}
            disabled={!connected || pending}
          >
            Refresh
          </button>
        </div>
        <p className="session-help">
          Sessions expire seven days after sign-in. Each browser signs in
          independently.
        </p>
        {sessions.kind === "loading" && <p>Loading sessions…</p>}
        {sessions.kind === "failed" && (
          <p className="error" role="alert">
            {sessions.message}
          </p>
        )}
        {sessions.kind === "ready" && (
          <ul className="session-list">
            {sessions.value.map((value) => (
              <li key={value.id}>
                <div className="session-icon" aria-hidden="true">
                  ▣
                </div>
                <div className="session-details">
                  <div className="session-title">
                    <h3>Browser session</h3>
                    {value.current && (
                      <span className="current-badge">This session</span>
                    )}
                  </div>
                  <p className="identifier">{value.id}</p>
                  <p>
                    Signed in{" "}
                    <time dateTime={value.createdAt}>
                      {formatDate(value.createdAt)}
                    </time>
                  </p>
                  <p>
                    Expires{" "}
                    <time dateTime={value.expiresAt}>
                      {formatDate(value.expiresAt)}
                    </time>
                  </p>
                </div>
                <button
                  className="secondary compact danger"
                  onClick={() => {
                    setSelected(value);
                    setError("");
                  }}
                  disabled={!connected || pending}
                  aria-label={`Revoke ${value.current ? "this session" : value.id}`}
                >
                  Revoke
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
      {selected && (
        <div
          className="confirm-panel"
          role="region"
          aria-labelledby="revoke-title"
          onKeyDown={(event) => {
            if (event.key === "Escape" && !pending) setSelected(null);
          }}
        >
          <div>
            <h2 id="revoke-title">
              {selected.current
                ? "Revoke this session?"
                : "Revoke the selected session?"}
            </h2>
            <p>
              {selected.current
                ? "You will return to sign-in immediately."
                : "That browser will need to sign in again."}
            </p>
            <p className="identifier">{selected.id}</p>
          </div>
          <div className="confirm-actions">
            <button
              className="secondary"
              onClick={() => setSelected(null)}
              disabled={pending}
            >
              Cancel
            </button>
            <button
              className="primary"
              ref={confirmButton}
              onClick={() => void revoke(selected)}
              disabled={!connected || pending}
            >
              {pending ? "Revoking…" : "Confirm revocation"}
            </button>
          </div>
          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
        </div>
      )}
    </>
  );
}
