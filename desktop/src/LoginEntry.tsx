import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';
import { itemSchema, statusSchema } from './login-domain';
import type { CredentialWriteStatus, CredentialWriteTarget, LoginItemView } from '@botstral/link';

export function LoginEntry({ bot }: { bot: string }) {
  const [open, setOpen] = useState(false);
  const [label, setLabel] = useState('Login');
  const [origin, setOrigin] = useState('');
  const [allBots, setAllBots] = useState(false);
  const [remember, setRemember] = useState(false);
  const [items, setItems] = useState<LoginItemView[]>([]);
  const [message, setMessage] = useState('');
  const [busy, setBusy] = useState(false);
  async function action(work: () => Promise<void>) {
    setBusy(true);
    try {
      await work();
    } catch {
      setMessage(
        'The protected operation was refused or is unresolved. Reconcile its receipt before entering values again.',
      );
    } finally {
      setBusy(false);
    }
  }
  function receipt(status: CredentialWriteStatus) {
    switch (status.type) {
      case 'committed':
        setMessage(`Saved ${status.item.metadata.label}. Revision ${status.item.revision}.`);
        break;
      case 'closed_without_commit':
        setMessage('This request is closed without a login write.');
        break;
      case 'unresolved':
        setMessage(
          'The outcome is unresolved. Reconcile the receipt before entering values again.',
        );
        break;
    }
  }
  async function enter(target: CredentialWriteTarget | null) {
    try {
      const status = statusSchema.parse(await invoke<unknown>('open_login_entry', { target }));
      if (target === null && status.type === 'closed_without_commit') setMessage('Vault unlocked.');
      else receipt(status);
    } catch (error) {
      if (error !== 'cancelled') throw error;
      setMessage('Protected entry cancelled. No values were sent.');
    }
  }
  return (
    <div>
      <button onClick={() => setOpen(!open)}>Protected login entry</button>
      {open && (
        <div aria-label="Login metadata">
          <p>
            Private username, password and optional TOTP seed are entered in native secure controls.
            This form contains only public metadata.
          </p>
          <label>
            Login label
            <input value={label} onChange={(e) => setLabel(e.target.value)} />
          </label>
          <label>
            Exact allowed origin
            <input
              placeholder="https://example.com"
              value={origin}
              onChange={(e) => setOrigin(e.target.value)}
            />
          </label>
          <label>
            <input
              type="checkbox"
              checked={allBots}
              onChange={(e) => setAllBots(e.target.checked)}
            />
            Allow all owner bots
          </label>
          <label>
            <input
              type="checkbox"
              checked={remember}
              onChange={(e) => setRemember(e.target.checked)}
            />
            Permit remembered password rules for this exact origin
          </label>
          <button disabled={busy} onClick={() => void action(() => enter(null))}>
            Unlock Vault
          </button>
          <button
            disabled={busy}
            onClick={() =>
              void action(async () => {
                await invoke('lock_vault');
                setMessage(
                  'Vault locked. Pending uses were revoked; any possible release remains protected.',
                );
              })
            }
          >
            Lock Vault
          </button>
          <button
            disabled={busy || !origin || !label}
            onClick={() =>
              void action(() =>
                enter({
                  type: 'create',
                  metadata: {
                    label,
                    allowed_origins: [origin],
                    bots: allBots ? { type: 'all_owner_bots' } : { type: 'selected', bots: [bot] },
                    password_policy: remember ? 'remembered_origins' : 'every_fill',
                  },
                }),
              )
            }
          >
            Enter private login
          </button>
          <button
            disabled={busy}
            onClick={() =>
              void action(async () => {
                const status = statusSchema
                  .nullable()
                  .parse(await invoke<unknown>('credential_write_status'));
                if (status) receipt(status);
                else setMessage('No protected request is pending.');
              })
            }
          >
            Reconcile receipt
          </button>
          <button
            disabled={busy}
            onClick={() =>
              void action(async () =>
                setItems(z.array(itemSchema).parse(await invoke<unknown>('login_items'))),
              )
            }
          >
            List login metadata
          </button>
          {items.map((item) => (
            <div key={item.id}>
              <p>
                {item.metadata.label}. Revision {item.revision}. {item.state}.{' '}
                {item.metadata.allowed_origins.join(', ')}.{' '}
                {item.metadata.password_policy.replaceAll('_', ' ')}.
              </p>
              {item.state === 'active' && (
                <>
                  <button
                    disabled={busy}
                    onClick={() =>
                      void action(() =>
                        enter({ type: 'rotate', id: item.id, expected_revision: item.revision }),
                      )
                    }
                  >
                    Rotate {item.metadata.label}
                  </button>
                  <button
                    disabled={busy}
                    onClick={() =>
                      void action(async () => {
                        await invoke('update_login', {
                          id: item.id,
                          expectedRevision: item.revision,
                          metadata: item.metadata,
                        });
                        setItems([]);
                        setMessage('Remembered rules cleared. Pending uses were revoked.');
                      })
                    }
                  >
                    Forget rules for {item.metadata.label}
                  </button>
                  <button
                    disabled={busy || !origin || !label}
                    onClick={() =>
                      void action(async () => {
                        await invoke('update_login', {
                          id: item.id,
                          expectedRevision: item.revision,
                          metadata: {
                            label,
                            allowed_origins: [origin],
                            bots: allBots
                              ? { type: 'all_owner_bots' }
                              : { type: 'selected', bots: [bot] },
                            password_policy: remember ? 'remembered_origins' : 'every_fill',
                          },
                        });
                        setItems([]);
                        setMessage(
                          'Metadata replaced with the form values. Remembered rules cleared and pending uses revoked.',
                        );
                      })
                    }
                  >
                    Replace metadata for {item.metadata.label}
                  </button>
                  <button
                    disabled={busy}
                    onClick={() =>
                      void action(async () => {
                        await invoke('update_login', {
                          id: item.id,
                          expectedRevision: item.revision,
                          metadata: null,
                        });
                        setMessage(
                          'Removed. Retired encrypted values remain for disclosure screening. Existing website sessions remain active.',
                        );
                        setItems([]);
                      })
                    }
                  >
                    Remove {item.metadata.label}
                  </button>
                </>
              )}
            </div>
          ))}
          <p role="status">{message}</p>
        </div>
      )}
    </div>
  );
}
