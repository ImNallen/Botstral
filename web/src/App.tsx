import { useCallback, useEffect, useRef, useState } from "react";
import { api, ApiError, errorMessage } from "./api";
import type { Auth, Connection, SessionView } from "./api";
import { SignIn } from "./SignIn";
import { Bots } from "./Bots";
import { Sessions } from "./Sessions";

export function App() {
  const [auth, setAuth] = useState<Auth>({ kind: "checking" });
  const [connection, setConnection] = useState<Connection>({
    kind: "connecting",
  });
  const [view, setView] = useState<"bots" | "sessions">("bots");
  const [refreshKey, setRefreshKey] = useState(0);
  const [signingOut, setSigningOut] = useState(false);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const authRef = useRef(auth);
  const connectionRef = useRef(connection);
  const action = useRef<AbortController | null>(null);
  authRef.current = auth;
  connectionRef.current = connection;

  const updateConnection = useCallback((kind: Connection["kind"]) => {
    connectionRef.current = { kind };
    setConnection((previous) => (previous.kind === kind ? previous : { kind }));
  }, []);

  const clearAuth = useCallback((reason: "ended" | "signedOut" | "revoked") => {
    generation.current++;
    action.current?.abort();
    authRef.current = { kind: "signedOut", reason };
    setAuth(authRef.current);
    setView("bots");
    setSigningOut(false);
    setError("");
  }, []);

  const handleError = useCallback(
    (error: unknown) => {
      if (error instanceof ApiError && error.status === 401) {
        updateConnection("connected");
        if (authRef.current.kind === "signedIn") clearAuth("ended");
        return;
      }
      if (
        !(error instanceof ApiError) ||
        error.status >= 500 ||
        error.code === "invalid_response"
      )
        updateConnection("disconnected");
    },
    [clearAuth, updateConnection],
  );

  const reachable = useCallback(() => {
    if (connectionRef.current.kind !== "disconnected")
      updateConnection("connected");
  }, [updateConnection]);

  const installSession = useCallback(
    (session: SessionView) => {
      generation.current++;
      authRef.current = { kind: "signedIn", session };
      setAuth(authRef.current);
      setRefreshKey((value) => value + 1);
      updateConnection("connected");
      setError("");
    },
    [updateConnection],
  );

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let controller: AbortController | undefined;
    let delay = 10000;
    async function poll() {
      clearTimeout(timer);
      controller?.abort();
      const currentController = new AbortController();
      controller = currentController;
      const current = generation.current;
      const valid = () =>
        !disposed &&
        !currentController.signal.aborted &&
        current === generation.current;
      try {
        if (authRef.current.kind === "signedOut") {
          await api.health(currentController.signal);
          if (!valid()) return;
          updateConnection("connected");
        } else {
          const session = await api.session(currentController.signal);
          if (!valid()) return;
          const reconnecting = connectionRef.current.kind !== "connected";
          authRef.current = { kind: "signedIn", session };
          setAuth(authRef.current);
          updateConnection("connected");
          if (reconnecting) setRefreshKey((value) => value + 1);
        }
        delay = 10000;
      } catch (error) {
        if (!valid()) return;
        if (error instanceof ApiError && error.status === 401) {
          updateConnection("connected");
          if (authRef.current.kind === "checking") {
            authRef.current = { kind: "signedOut", reason: "initial" };
            setAuth(authRef.current);
          } else {
            clearAuth("ended");
          }
          delay = 10000;
        } else {
          updateConnection("disconnected");
          delay = Math.min(delay * 1.5, 30000);
        }
      } finally {
        if (!disposed && !currentController.signal.aborted)
          timer = setTimeout(() => void poll(), delay);
      }
    }
    const wake = () => void poll();
    void poll();
    window.addEventListener("focus", wake);
    window.addEventListener("online", wake);
    return () => {
      disposed = true;
      clearTimeout(timer);
      controller?.abort();
      window.removeEventListener("focus", wake);
      window.removeEventListener("online", wake);
    };
  }, [auth.kind, clearAuth, updateConnection]);

  useEffect(() => () => action.current?.abort(), []);

  async function signOut() {
    if (
      auth.kind !== "signedIn" ||
      connection.kind !== "connected" ||
      signingOut
    )
      return;
    const current = generation.current;
    const controller = new AbortController();
    action.current = controller;
    setSigningOut(true);
    setError("");
    try {
      await api.signOut(auth.session.csrfToken, controller.signal);
      if (controller.signal.aborted || current !== generation.current) return;
      clearAuth("signedOut");
    } catch (error) {
      if (controller.signal.aborted || current !== generation.current) return;
      setError(errorMessage(error));
      handleError(error);
    } finally {
      if (!controller.signal.aborted && current === generation.current)
        setSigningOut(false);
    }
  }

  let connectionLabel: string;
  switch (connection.kind) {
    case "connecting":
      connectionLabel = "Connecting to manager";
      break;
    case "connected":
      connectionLabel = "Manager connected";
      break;
    case "disconnected":
      connectionLabel = "Manager disconnected";
      break;
    default: {
      const exhaustive: never = connection;
      return exhaustive;
    }
  }
  const connected = connection.kind === "connected";
  const notice =
    auth.kind === "signedOut" && auth.reason !== "initial"
      ? auth.reason === "signedOut"
        ? "You have signed out."
        : auth.reason === "revoked"
          ? "This session was revoked. Sign in to continue."
          : "Your session ended. Sign in to continue."
      : "";

  return (
    <div className="app">
      <a className="skip-link" href="#main-content">
        Skip to content
      </a>
      <header className="app-header">
        <div className="brand">
          <svg className="brand-icon" viewBox="0 0 32 32" aria-hidden="true">
            <path d="M4 4h10v10H4zM18 4h10v10H18zM4 18h10v10H4z" />
            <path className="brand-accent" d="M18 18h10v10H18z" />
          </svg>
          <span>
            botstral<span className="brand-dot">.</span>
          </span>
        </div>
        <div className="header-actions">
          <span
            className={`connection ${connection.kind}`}
            role="status"
            aria-live="polite"
          >
            <span aria-hidden="true" />
            {connectionLabel}
          </span>
          {auth.kind === "signedIn" && (
            <button
              className="secondary compact"
              disabled={!connected || signingOut}
              onClick={() => void signOut()}
            >
              {signingOut ? "Signing out…" : "Sign out"}
            </button>
          )}
        </div>
      </header>
      <div id="main-content" tabIndex={-1}>
        {auth.kind === "checking" && (
          <main className="checking">
            <p className="eyebrow">BOTSTRAL</p>
            <h1>
              {connection.kind === "disconnected"
                ? "Waiting for your manager."
                : "Opening your workspace."}
            </h1>
            <p>
              {connection.kind === "disconnected"
                ? "Check that the local manager is running. This page reconnects automatically."
                : "Checking your session…"}
            </p>
          </main>
        )}
        {auth.kind === "signedOut" && (
          <SignIn
            disabled={!connected}
            notice={notice}
            onSuccess={installSession}
            onError={handleError}
          />
        )}
        {auth.kind === "signedIn" && (
          <div className="workspace" key={auth.session.session.id}>
            <aside className="sidebar">
              <div>
                <p className="eyebrow">PERSONAL MANAGER</p>
                <nav aria-label="Workspace">
                  <button
                    className={view === "bots" ? "nav-item active" : "nav-item"}
                    aria-current={view === "bots" ? "page" : undefined}
                    onClick={() => setView("bots")}
                  >
                    <span aria-hidden="true">▦</span> Your bots
                  </button>
                  <button
                    className={
                      view === "sessions" ? "nav-item active" : "nav-item"
                    }
                    aria-current={view === "sessions" ? "page" : undefined}
                    onClick={() => setView("sessions")}
                  >
                    <span aria-hidden="true">◫</span> Sessions
                  </button>
                </nav>
              </div>
              <div className="owner-card">
                <span className="owner-avatar" aria-hidden="true">
                  {auth.session.owner.username.slice(0, 1).toUpperCase()}
                </span>
                <div>
                  <strong>{auth.session.owner.username}</strong>
                  <span>Installation owner</span>
                </div>
              </div>
            </aside>
            <main className="workspace-main" inert={signingOut}>
              {error && (
                <p className="error global-error" role="alert">
                  {error}
                </p>
              )}
              {view === "bots" ? (
                <Bots
                  csrf={auth.session.csrfToken}
                  connected={connected}
                  refreshKey={refreshKey}
                  onError={handleError}
                  onReachable={reachable}
                />
              ) : (
                <Sessions
                  csrf={auth.session.csrfToken}
                  connected={connected}
                  refreshKey={refreshKey}
                  onError={handleError}
                  onReachable={reachable}
                  onCurrentRevoked={() => clearAuth("revoked")}
                />
              )}
              <footer>Stored on your manager. Yours to build on.</footer>
            </main>
          </div>
        )}
      </div>
    </div>
  );
}
