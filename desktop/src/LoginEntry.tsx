import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';
import type { CredentialWriteStatus, CredentialWriteTarget, LoginItemView } from '@botstral/link';

const revision = z.number().int().positive().safe();
const metadataSchema = z.strictObject({
  label: z.string(),
  bots: z.discriminatedUnion('type', [
    z.strictObject({ type: z.literal('all_owner_bots') }),
    z.strictObject({ type: z.literal('selected'), bots: z.array(z.uuid()) }),
  ]),
  allowed_origins: z.array(z.string()),
  password_policy: z.enum(['every_fill', 'remembered_origins']),
});
const itemSchema = z.strictObject({
  id: z.uuid(),
  revision,
  epoch: revision,
  state: z.enum(['active', 'removed']),
  metadata: metadataSchema,
  has_totp: z.boolean(),
}) satisfies z.ZodType<LoginItemView>;
const statusSchema = z.discriminatedUnion('type', [
  z.strictObject({ type: z.literal('committed'), request: z.uuid(), item: itemSchema }),
  z.strictObject({ type: z.literal('closed_without_commit'), request: z.uuid() }),
  z.strictObject({ type: z.literal('unresolved'), request: z.uuid() }),
]) satisfies z.ZodType<CredentialWriteStatus>;

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
    const status = statusSchema.parse(await invoke<unknown>('open_login_entry', { target }));
    if (target === null && status.type === 'closed_without_commit') setMessage('Vault unlocked.');
    else receipt(status);
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
                {item.metadata.label}. Revision {item.revision}. {item.state}.
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
