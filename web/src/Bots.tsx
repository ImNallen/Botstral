import { useCallback, useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { api, errorMessage, formatDate } from "./api";
import type { Bot, CSRFToken, Load } from "./api";

export function Bots({
  csrf,
  connected,
  refreshKey,
  onError,
  onReachable,
}: {
  csrf: CSRFToken;
  connected: boolean;
  refreshKey: number;
  onError: (error: unknown) => void;
  onReachable: () => void;
}) {
  const [bots, setBots] = useState<Load<Bot[]>>({ kind: "loading" });
  const [draft, setDraft] = useState("");
  const [message, setMessage] = useState("");
  const [pending, setPending] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [reload, setReload] = useState(0);
  const nameLength = Array.from(draft.trim()).length;
  const validName = nameLength > 0 && nameLength <= 80;
  const generation = useRef(0);
  const mutation = useRef<AbortController | null>(null);
  useEffect(
    () => () => {
      generation.current++;
      mutation.current?.abort();
    },
    [],
  );
  const refresh = useCallback(() => setReload((value) => value + 1), []);

  useEffect(() => {
    if (!connected) return;
    const controller = new AbortController();
    const current = generation.current;
    api
      .listBots(controller.signal)
      .then((value) => {
        if (controller.signal.aborted || current !== generation.current) return;
        setBots({ kind: "ready", value });
        setUncertain(false);
        onReachable();
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted || current !== generation.current) return;
        setBots((previous) =>
          previous.kind === "ready"
            ? previous
            : { kind: "failed", message: errorMessage(error) },
        );
        onError(error);
      });
    return () => controller.abort();
  }, [connected, refreshKey, reload, onError, onReachable]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!connected || pending || uncertain || !validName) return;
    const controller = new AbortController();
    mutation.current = controller;
    const current = generation.current;
    setPending(true);
    setMessage("");
    try {
      const bot = await api.createBot(
        { name: draft.trim() },
        csrf,
        controller.signal,
      );
      if (controller.signal.aborted || current !== generation.current) return;
      setBots((previous) =>
        previous.kind === "ready"
          ? { kind: "ready", value: [...previous.value, bot] }
          : previous,
      );
      setDraft("");
      setMessage(`${bot.name} is saved.`);
      onReachable();
      refresh();
    } catch (error) {
      if (controller.signal.aborted || current !== generation.current) return;
      setMessage(errorMessage(error));
      setUncertain(true);
      onError(error);
      refresh();
    } finally {
      if (!controller.signal.aborted && current === generation.current)
        setPending(false);
    }
  }

  return (
    <>
      <div className="page-heading">
        <div>
          <p className="eyebrow">YOUR WORKSPACE</p>
          <h1>
            Your bots<span className="accent">.</span>
          </h1>
          <p>A name, an identity, and a place of its own.</p>
        </div>
        <span className="count">
          {bots.kind === "ready"
            ? `${bots.value.length} saved`
            : "Bot identities"}
        </span>
      </div>
      {!connected && (
        <p className="notice" role="status">
          Showing saved data from this browser. The manager is disconnected.
          Creation is paused until it reconnects.
        </p>
      )}
      <div className="bot-layout">
        <section
          className="bot-collection"
          aria-label="Saved bots"
          aria-busy={bots.kind === "loading"}
        >
          {bots.kind === "loading" && (
            <div className="empty-state">
              <p>Loading your bots…</p>
            </div>
          )}
          {bots.kind === "failed" && (
            <div className="empty-state">
              <h2>Could not load your bots</h2>
              <p className="error" role="alert">
                {bots.message}
              </p>
              <button
                className="secondary"
                onClick={refresh}
                disabled={!connected}
              >
                Try again
              </button>
            </div>
          )}
          {bots.kind === "ready" && bots.value.length === 0 && (
            <div className="empty-state">
              <div className="empty-art" aria-hidden="true">
                <span />
                <span />
                <span />
              </div>
              <p className="eyebrow">A FRESH START</p>
              <h2>Start with a name.</h2>
              <p>
                A researcher, a writing partner, a builder.
                <br />
                Create a bot identity and make it yours.
              </p>
              <span className="quiet-tag">No bots yet</span>
            </div>
          )}
          {bots.kind === "ready" && bots.value.length > 0 && (
            <div className="bot-grid">
              {bots.value.map((bot) => (
                <article
                  className="bot-card"
                  key={bot.id}
                  data-testid="bot-card"
                  data-bot-id={bot.id}
                >
                  <div className="bot-card-top">
                    <span className="bot-avatar" aria-hidden="true">
                      {Array.from(bot.name)[0]?.toLocaleUpperCase()}
                    </span>
                    <span className="saved-mark">Identity saved</span>
                  </div>
                  <h2>{bot.name}</h2>
                  <p className="bot-description">A saved bot identity.</p>
                  <dl>
                    <div>
                      <dt>Created</dt>
                      <dd>
                        <time dateTime={bot.createdAt}>
                          {formatDate(bot.createdAt)}
                        </time>
                      </dd>
                    </div>
                    <div>
                      <dt>Bot ID</dt>
                      <dd className="identifier" title={bot.id}>
                        {bot.id}
                      </dd>
                    </div>
                  </dl>
                </article>
              ))}
            </div>
          )}
        </section>
        <aside className="create-panel" aria-labelledby="create-title">
          <span className="panel-icon" aria-hidden="true">
            +
          </span>
          <h2 id="create-title">Make it yours</h2>
          <p>Give your bot a name. You can create more than one.</p>
          <form onSubmit={submit}>
            <label htmlFor="bot-name">Bot name</label>
            <input
              id="bot-name"
              name="name"
              placeholder="e.g. Research"
              required
              maxLength={160}
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              disabled={pending}
              aria-describedby="bot-name-help"
              aria-invalid={nameLength > 80}
            />
            <p id="bot-name-help" className="field-help">
              {nameLength > 80
                ? "Use at most 80 characters."
                : "1 to 80 characters. Names can be reused."}
            </p>
            <button
              className="primary full"
              type="submit"
              disabled={!connected || pending || uncertain || !validName}
            >
              {pending ? "Creating…" : "Create bot"}
            </button>
          </form>
          {message && (
            <p className="form-message" role="status">
              {message}
            </p>
          )}
          {uncertain && (
            <p className="field-help">
              The result may have been saved. Refresh the bot list before
              creating again.
              <button
                className="text-button"
                onClick={refresh}
                disabled={!connected}
              >
                Refresh list
              </button>
            </p>
          )}
          <div className="scope-callout">
            <span aria-hidden="true">◇</span>
            <p>
              For now, bots have an identity. Conversations and task execution
              are not available yet.
            </p>
          </div>
        </aside>
      </div>
    </>
  );
}
