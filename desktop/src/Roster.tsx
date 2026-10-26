import { useEffect, useRef, useState } from 'react';
import { ApplicationController } from './application';
import type { Bot, Snapshot } from './domain';
import { Avatar } from './Avatar';
import { Icon } from './Icon';
import { connectionText, formatDate } from './format';

type Popover =
  | { kind: 'closed' }
  | { kind: 'search'; query: string }
  | { kind: 'account' }
  | { kind: 'connections' };

export function Roster({
  controller,
  snapshot,
  selected,
  busy,
  savedCreation,
  theme,
  onToggleTheme,
  onShowDetails,
}: {
  controller: ApplicationController;
  snapshot: Snapshot | null;
  selected: Bot | null;
  busy: boolean;
  savedCreation: boolean;
  theme: 'light' | 'dark';
  onToggleTheme: () => void;
  onShowDetails: () => void;
}) {
  const signedIn = snapshot?.kind === 'signed_in' ? snapshot : null;
  const [popover, setPopover] = useState<Popover>({ kind: 'closed' });
  const rosterRef = useRef<HTMLElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const searchButtonRef = useRef<HTMLButtonElement>(null);
  const ownerButtonRef = useRef<HTMLButtonElement>(null);
  const connectionsButtonRef = useRef<HTMLButtonElement>(null);
  const endpoint = signedIn?.endpoint;
  useEffect(() => {
    if (popover.kind === 'search') searchRef.current?.focus();
    else popoverRef.current?.querySelector('button')?.focus();
  }, [popover.kind]);
  useEffect(() => {
    const keyboard = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k' && endpoint) {
        event.preventDefault();
        setPopover({ kind: 'search', query: '' });
        searchRef.current?.focus();
      }
      if (event.key === 'Escape' && popover.kind !== 'closed') {
        event.preventDefault();
        const trigger =
          popover.kind === 'search'
            ? searchButtonRef
            : popover.kind === 'account'
              ? ownerButtonRef
              : connectionsButtonRef;
        setPopover({ kind: 'closed' });
        trigger.current?.focus();
      }
    };
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !rosterRef.current?.contains(event.target)) {
        setPopover({ kind: 'closed' });
      }
    };
    window.addEventListener('keydown', keyboard);
    window.addEventListener('pointerdown', outside);
    return () => {
      window.removeEventListener('keydown', keyboard);
      window.removeEventListener('pointerdown', outside);
    };
  }, [endpoint, popover.kind]);
  const query = popover.kind === 'search' ? popover.query : '';
  const visibleBots =
    signedIn?.bots.filter((bot) =>
      bot.name.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
    ) ?? [];
  const close = () => setPopover({ kind: 'closed' });
  return (
    <aside ref={rosterRef} className="roster" aria-label="Bot roster">
      <header className="roster-header" data-tauri-drag-region>
        <button
          ref={searchButtonRef}
          className="circle-button"
          aria-label="Search bots"
          aria-expanded={popover.kind === 'search'}
          title="Search bots (Command or Control K)"
          disabled={!signedIn}
          onClick={() =>
            setPopover(
              popover.kind === 'search' ? { kind: 'closed' } : { kind: 'search', query: '' },
            )
          }
        >
          <Icon name="search" size={18} />
        </button>
        <button
          className="circle-button"
          aria-label="Create bot"
          title={signedIn ? 'Create bot' : 'Sign in to create a bot'}
          disabled={!signedIn || busy}
          onClick={() => {
            close();
            controller.openCreate();
          }}
        >
          <Icon name="plus" size={20} />
        </button>
      </header>
      {popover.kind === 'search' && (
        <div className="search-popover" role="search" aria-label="Bot search">
          <Icon name="search" size={16} />
          <input
            ref={searchRef}
            aria-label="Search bots"
            placeholder="Search bots"
            value={popover.query}
            onChange={(event) => setPopover({ kind: 'search', query: event.target.value })}
          />
          <button className="icon-button" aria-label="Close search" onClick={close}>
            <Icon name="close" size={15} />
          </button>
        </div>
      )}
      <nav className="bot-list" aria-label="Bots">
        {visibleBots.map((bot) => {
          const primary = bot.id === signedIn?.bots[0]?.id;
          return (
            <button
              key={bot.id}
              aria-label={bot.name}
              className={`${primary ? 'bot-tile' : 'bot-row'}${selected?.id === bot.id ? ' selected' : ''}`}
              aria-current={selected?.id === bot.id ? 'true' : undefined}
              onClick={() => controller.select(bot.id)}
            >
              <Avatar seed={bot.id} primary={primary} />
              <span className="bot-row-text">
                <strong>{bot.name}</strong>
                {!primary && <span>Created {formatDate(bot.created_at)}</span>}
              </span>
            </button>
          );
        })}
        {!signedIn && <p className="roster-hint">Sign in to see your bots.</p>}
        {signedIn && signedIn.bots.length === 0 && (
          <p className="roster-hint">Create your first bot with the plus button.</p>
        )}
        {signedIn && signedIn.bots.length > 0 && visibleBots.length === 0 && (
          <p className="roster-hint">No bots match "{query}".</p>
        )}
        {signedIn && savedCreation && (
          <button className="saved-creation" onClick={() => controller.openCreate()}>
            Review saved creation
            <Icon name="chevron" size={14} />
          </button>
        )}
      </nav>
      <footer className="roster-footer">
        {popover.kind === 'account' && (
          <div
            ref={popoverRef}
            className="sidebar-popover"
            role="dialog"
            aria-label="Workspace menu"
          >
            <div className="menu-identity">
              <strong>{signedIn?.owner.name ?? 'Botstral'}</strong>
              <span>{connectionText(snapshot)}</span>
            </div>
            <button
              disabled={busy}
              onClick={() => {
                close();
                void controller.refresh();
              }}
            >
              <Icon name="refresh" size={16} />
              Refresh manager
            </button>
            <button onClick={onToggleTheme}>
              <Icon name={theme === 'dark' ? 'sun' : 'moon'} size={16} />
              {theme === 'dark' ? 'Use light theme' : 'Use dark theme'}
            </button>
            <button
              onClick={() => {
                close();
                onShowDetails();
              }}
            >
              <Icon name="panel" size={16} />
              Workspace details
            </button>
            <button
              onClick={() => {
                close();
                void controller.testNotification();
              }}
            >
              <Icon name="bell" size={16} />
              Test notification
            </button>
            {signedIn && (
              <button
                onClick={() => {
                  close();
                  void controller.signOut();
                }}
              >
                <Icon name="logout" size={16} />
                Sign out of this device
              </button>
            )}
          </div>
        )}
        {popover.kind === 'connections' && (
          <div
            ref={popoverRef}
            className="sidebar-popover connections-popover"
            role="dialog"
            aria-labelledby="connections-title"
          >
            <div className="popover-heading">
              <strong id="connections-title">Connect apps</strong>
              <button className="icon-button" aria-label="Close connections" onClick={close}>
                <Icon name="close" size={15} />
              </button>
            </div>
            <p>
              App connections are not available yet. No external accounts are connected through
              Botstral.
            </p>
          </div>
        )}
        <button
          ref={ownerButtonRef}
          className="owner-button"
          aria-label="Open workspace menu"
          aria-expanded={popover.kind === 'account'}
          aria-haspopup="dialog"
          onClick={() =>
            setPopover(popover.kind === 'account' ? { kind: 'closed' } : { kind: 'account' })
          }
        >
          {signedIn?.owner.name.slice(0, 1).toLocaleUpperCase() ?? 'B'}
        </button>
        <button
          ref={connectionsButtonRef}
          className="connect-button"
          aria-expanded={popover.kind === 'connections'}
          aria-haspopup="dialog"
          onClick={() =>
            setPopover(
              popover.kind === 'connections' ? { kind: 'closed' } : { kind: 'connections' },
            )
          }
        >
          Connect apps
        </button>
      </footer>
    </aside>
  );
}
