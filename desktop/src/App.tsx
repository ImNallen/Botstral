import { useEffect, useState, useSyncExternalStore } from 'react';
import { ApplicationController } from './application';
import { errorMessages } from './domain';
import { Avatar } from './Avatar';
import { Icon } from './Icon';
import { SignIn } from './SignIn';
import { Details } from './Details';
import { CreateDialog } from './CreateDialog';
import { formatDate } from './format';
import { Roster } from './Roster';

export function App({ controller }: { controller: ApplicationController }) {
  const state = useSyncExternalStore(controller.subscribe, controller.getState);
  const snapshot = state.view.kind === 'ready' ? state.view.snapshot : null;
  const signedIn = snapshot?.kind === 'signed_in' ? snapshot : null;
  const [details, setDetails] = useState(false);
  const [theme, setTheme] = useState<'light' | 'dark'>(() =>
    window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
  );
  const selected =
    signedIn?.bots.find((bot) => bot.id === state.selected) ?? signedIn?.bots[0] ?? null;
  const busy = state.activity !== 'idle';
  useEffect(() => {
    void controller.start();
  }, [controller]);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  useEffect(() => {
    if (!signedIn || busy) return;
    const timer = window.setInterval(() => {
      void controller.refresh();
    }, 15000);
    return () => window.clearInterval(timer);
  }, [controller, signedIn, busy]);
  const createOpen =
    state.creation.kind === 'editing' || (state.creation.kind === 'pending' && state.creation.open);
  return (
    <div className={`workspace${details ? ' with-details' : ''}`}>
      <Roster
        key={signedIn?.endpoint ?? 'signed-out'}
        controller={controller}
        snapshot={snapshot}
        selected={selected}
        busy={busy}
        savedCreation={state.creation.kind === 'pending' && !state.creation.open}
        theme={theme}
        onToggleTheme={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
        onShowDetails={() => setDetails(true)}
      />
      <main className="main-panel">
        <header className="conversation-header" data-tauri-drag-region>
          <button
            className="conversation-title"
            aria-label={details ? 'Hide details' : 'Show details'}
            aria-expanded={details}
            title="Workspace details"
            onClick={() => setDetails(!details)}
          >
            {selected && (
              <Avatar seed={selected.id} primary={selected.id === signedIn?.bots[0]?.id} />
            )}
            <span>{selected?.name ?? 'Botstral'}</span>
          </button>
        </header>
        <div className="conversation">
          {state.error && !(createOpen && signedIn) && (
            <div className="banner error-banner" role="alert">
              <span>{errorMessages[state.error]}</span>
              {!busy && (
                <button
                  onClick={() => {
                    void (snapshot ? controller.refresh() : controller.start());
                  }}
                >
                  Retry
                </button>
              )}
            </div>
          )}
          {state.notice && (
            <div className="banner notice-banner" role="status">
              {state.notice}
            </div>
          )}
          {signedIn && snapshot?.connection === 'offline' && (
            <div className="banner offline-banner" role="status">
              Showing the last successful snapshot. Refresh when the manager is available.
            </div>
          )}
          {signedIn ? (
            <>
              {selected ? (
                <p className="conversation-status" role="status">
                  Messaging is not available yet. You can browse bots and view their details.
                </p>
              ) : (
                <section className="empty-roster">
                  <h1>Create your first bot</h1>
                  <p>Your bots will appear in the sidebar.</p>
                  <button
                    className="primary-button"
                    disabled={busy}
                    onClick={() => controller.openCreate()}
                  >
                    <Icon name="plus" size={16} />
                    Create your first bot
                  </button>
                </section>
              )}
            </>
          ) : (
            <SignIn
              key={state.view.kind}
              controller={controller}
              busy={busy}
              signingOut={state.view.kind === 'signing_out'}
              initial={state.view.kind === 'starting'}
              snapshot={snapshot}
            />
          )}
        </div>
        {signedIn && (
          <div className="composer" aria-label="Messaging unavailable">
            <button className="composer-plus" aria-label="Attachments unavailable" disabled>
              <Icon name="plus" size={18} />
            </button>
            <input
              aria-label="Message"
              disabled
              placeholder={selected ? `Message ${selected.name}` : 'Create a bot to get started'}
            />
            <button className="composer-mic" aria-label="Microphone unavailable" disabled>
              <Icon name="mic" size={17} />
            </button>
            <button className="composer-voice" aria-label="Voice conversation unavailable" disabled>
              <Icon name="waveform" size={18} />
            </button>
          </div>
        )}
      </main>
      {details && (
        <aside className="details-panel" aria-label="Workspace details">
          <header className="details-header">
            <strong>Details</strong>
            <button
              className="icon-button"
              aria-label="Close details"
              onClick={() => setDetails(false)}
            >
              <Icon name="close" size={16} />
            </button>
          </header>
          <div className="details-content">
            {selected && (
              <div className="profile">
                <Avatar seed={selected.id} primary={selected.id === signedIn?.bots[0]?.id} large />
                <h2>{selected.name}</h2>
                <span>Local bot</span>
                <p>Created {formatDate(selected.created_at)}</p>
              </div>
            )}
            <Details snapshot={snapshot} signedIn={signedIn} />
            <section className="details-section">
              <h3>Desktop</h3>
              <button
                className="detail-action"
                onClick={() => {
                  void controller.testNotification();
                }}
              >
                <Icon name="bell" size={16} />
                Test notification
              </button>
              <p className="muted small-copy">
                An explicit test only. System settings control delivery.
              </p>
              {signedIn && (
                <button
                  className="detail-action"
                  onClick={() => {
                    void controller.signOut();
                  }}
                >
                  <Icon name="logout" size={16} />
                  Sign out of this device
                </button>
              )}
              <p className="muted small-copy">
                Closing the window keeps Botstral in the menu bar. Use Quit Botstral to exit.
              </p>
            </section>
          </div>
          <footer className="details-footer">
            Botstral <span>0.1.0</span>
          </footer>
        </aside>
      )}
      {createOpen && signedIn && (
        <CreateDialog controller={controller} creation={state.creation} error={state.error} />
      )}
    </div>
  );
}
