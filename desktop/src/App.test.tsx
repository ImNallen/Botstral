import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { ApplicationController } from './application';
import { application, signedIn, signedOut, deferred } from './fixtures.test-support';
import type { Snapshot } from './domain';

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn(() => ({ matches: false })),
});
HTMLDialogElement.prototype.showModal = function () {
  this.open = true;
};
HTMLDialogElement.prototype.close = function () {
  this.open = false;
};

describe('desktop flows', () => {
  it('preserves a non-default manager address after signing out', async () => {
    const user = userEvent.setup();
    const endpoint = '127.0.0.1:54321';
    const app = application(signedIn({ endpoint }));
    const waiting = deferred<Snapshot>();
    app.signOut.mockReturnValue(waiting.promise);
    render(<App controller={new ApplicationController(app)} />);
    await screen.findByRole('button', { name: 'Research' });
    await user.click(screen.getByRole('button', { name: 'Open workspace menu' }));
    await user.click(screen.getByRole('button', { name: 'Sign out of this device' }));
    waiting.resolve({ ...signedOut(), endpoint });
    await screen.findByRole('button', { name: 'Sign in' });
    expect(screen.getByLabelText('Manager address').getAttribute('value')).toBe(endpoint);
  });
  it('switches real bot records and opens their observed details from the title', async () => {
    const user = userEvent.setup();
    const app = application(
      signedIn({
        details: {
          vault: 'locked',
          provider: null,
          workers: [{ id: '00000000-0000-4000-8000-000000000003', revoked: false, phase: 'ready' }],
        },
      }),
    );
    render(<App controller={new ApplicationController(app)} />);
    const research = await screen.findByRole('button', { name: 'Research' });
    expect(research.getAttribute('aria-current')).toBe('true');
    await user.click(screen.getByRole('button', { name: 'Writing' }));
    expect(screen.getByRole('button', { name: 'Writing' }).getAttribute('aria-current')).toBe(
      'true',
    );
    expect(screen.getByRole('button', { name: 'Show details' }).textContent).toBe('Writing');
    await user.click(screen.getByRole('button', { name: 'Show details' }));
    expect(screen.getByRole('heading', { name: 'Writing' })).toBeTruthy();
    expect(screen.getByText('1 ready · 1 enrolled')).toBeTruthy();
    expect(screen.getByText('Not configured')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Hide details' }));
    expect(screen.queryByRole('complementary', { name: 'Workspace details' })).toBeNull();
  });
  it('reveals search by button and keyboard, filters bots, and dismisses with Escape', async () => {
    const user = userEvent.setup();
    render(<App controller={new ApplicationController(application())} />);
    await screen.findByRole('button', { name: 'Research' });
    expect(screen.queryByRole('textbox', { name: 'Search bots' })).toBeNull();
    await user.click(screen.getByRole('button', { name: 'Search bots' }));
    const search = screen.getByRole('textbox', { name: 'Search bots' });
    expect(document.activeElement).toBe(search);
    await user.type(search, 'Research');
    expect(screen.queryByRole('button', { name: 'Writing' })).toBeNull();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('textbox', { name: 'Search bots' })).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Search bots' }));
    expect(screen.getByRole('button', { name: 'Writing' })).toBeTruthy();
    fireEvent.keyDown(window, { key: 'k', metaKey: true });
    expect(screen.getByRole('textbox', { name: 'Search bots' })).toBe(document.activeElement);
    await user.keyboard('{Escape}');
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    expect(screen.getByRole('textbox', { name: 'Search bots' })).toBe(document.activeElement);
  });
  it('clears search when a different manager snapshot is accepted and on sign-out', async () => {
    const user = userEvent.setup();
    const app = application();
    const controller = new ApplicationController(app);
    render(<App controller={controller} />);
    await screen.findByRole('button', { name: 'Research' });
    await user.click(screen.getByRole('button', { name: 'Search bots' }));
    await user.type(screen.getByRole('textbox', { name: 'Search bots' }), 'Writing');
    app.refresh.mockResolvedValue(signedIn({ endpoint: '127.0.0.1:43121', revision: 3 }));
    await controller.refresh();
    await waitFor(() => expect(screen.queryByRole('textbox', { name: 'Search bots' })).toBeNull());
    expect(screen.getByRole('button', { name: 'Research' })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Search bots' }));
    await user.type(screen.getByRole('textbox', { name: 'Search bots' }), 'Writing');
    await controller.signOut();
    await screen.findByRole('button', { name: 'Sign in' });
    expect(screen.queryByRole('textbox', { name: 'Search bots' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Writing' })).toBeNull();
    app.signIn.mockResolvedValue(signedIn({ generation: 4, revision: 6 }));
    await user.type(screen.getByLabelText('Owner password'), 'SYNTHETIC owner password{Enter}');
    await screen.findByRole('button', { name: 'Research' });
    await user.click(screen.getByRole('button', { name: 'Search bots' }));
    expect(screen.getByRole('textbox', { name: 'Search bots' }).getAttribute('value')).toBe('');
    expect(screen.getByRole('button', { name: 'Writing' })).toBeTruthy();
  });
  it('provides theme, refresh, details, notification, and sign-out in the owner menu', async () => {
    const user = userEvent.setup();
    const app = application();
    render(<App controller={new ApplicationController(app)} />);
    await screen.findByRole('button', { name: 'Research' });
    await user.click(screen.getByRole('button', { name: 'Open workspace menu' }));
    const menu = screen.getByRole('dialog', { name: 'Workspace menu' });
    expect(within(menu).getByText('Synthetic owner')).toBeTruthy();
    await user.click(within(menu).getByRole('button', { name: 'Use dark theme' }));
    expect(document.documentElement.dataset.theme).toBe('dark');
    await user.click(within(menu).getByRole('button', { name: 'Use light theme' }));
    expect(document.documentElement.dataset.theme).toBe('light');
    const refreshes = app.refresh.mock.calls.length;
    await user.click(within(menu).getByRole('button', { name: 'Refresh manager' }));
    await waitFor(() => expect(app.refresh).toHaveBeenCalledTimes(refreshes + 1));
    await user.click(screen.getByRole('button', { name: 'Open workspace menu' }));
    await user.click(screen.getByRole('button', { name: 'Workspace details' }));
    expect(screen.getByRole('complementary', { name: 'Workspace details' })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Close details' }));
    await user.click(screen.getByRole('button', { name: 'Open workspace menu' }));
    await user.click(screen.getByRole('button', { name: 'Test notification' }));
    expect(await screen.findByText(/Delivery is unconfirmed/)).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Open workspace menu' }));
    await user.click(screen.getByRole('button', { name: 'Sign out of this device' }));
    await screen.findByRole('button', { name: 'Sign in' });
    expect(app.signOut).toHaveBeenCalledOnce();
    expect(screen.queryByRole('dialog', { name: 'Workspace menu' })).toBeNull();
  });
  it('explains unavailable app connections and disables every messaging action', async () => {
    const user = userEvent.setup();
    render(<App controller={new ApplicationController(application())} />);
    await screen.findByRole('button', { name: 'Research' });
    expect(screen.getByText(/Messaging is not available yet/)).toBeTruthy();
    for (const name of [
      'Attachments unavailable',
      'Microphone unavailable',
      'Voice conversation unavailable',
    ]) {
      expect(screen.getByRole('button', { name }).hasAttribute('disabled')).toBe(true);
    }
    expect(screen.getByRole('textbox', { name: 'Message' }).hasAttribute('disabled')).toBe(true);
    await user.click(screen.getByRole('button', { name: 'Connect apps' }));
    const dialog = screen.getByRole('dialog', { name: 'Connect apps' });
    expect(within(dialog).getByText(/No external accounts are connected/)).toBeTruthy();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog', { name: 'Connect apps' })).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Connect apps' }));
  });
  it('submits sign-in with the keyboard and clears the password while awaiting native work', async () => {
    const user = userEvent.setup();
    const app = application();
    app.inspect.mockResolvedValue(signedOut());
    app.refresh.mockResolvedValue(signedOut());
    const waiting = deferred<Snapshot>();
    app.signIn.mockReturnValue(waiting.promise);
    render(<App controller={new ApplicationController(app)} />);
    await screen.findByRole('button', { name: 'Sign in' });
    const password = screen.getByLabelText('Owner password');
    await user.type(password, 'SYNTHETIC owner password{Enter}');
    await waitFor(() =>
      expect(app.signIn).toHaveBeenCalledWith({
        endpoint: '127.0.0.1:43120',
        password: 'SYNTHETIC owner password',
      }),
    );
    expect(password.getAttribute('value')).toBe('');
    waiting.resolve(signedIn({ revision: 6, generation: 4 }));
    await screen.findByRole('button', { name: 'Research' });
  });
  it('creates a bot by keyboard and retains its identity for a failed retry', async () => {
    const user = userEvent.setup();
    const app = application();
    render(<App controller={new ApplicationController(app)} />);
    await screen.findByRole('button', { name: 'Research' });
    await user.click(screen.getByRole('button', { name: 'Create bot' }));
    const name = screen.getByRole('textbox', { name: 'Bot name' });
    await user.type(name, 'Synthetic bot');
    fireEvent.submit(name.closest('form') ?? name);
    await screen.findByRole('button', { name: 'Retry same bot' });
    const draft = app.createBot.mock.calls[0]?.[0];
    await user.click(screen.getByRole('button', { name: 'Retry same bot' }));
    await waitFor(() => expect(app.createBot).toHaveBeenCalledTimes(2));
    expect(app.createBot.mock.calls[1]?.[0]).toEqual(draft);
  });
  it('offers creation when the actual manager roster is empty', async () => {
    const user = userEvent.setup();
    render(<App controller={new ApplicationController(application(signedIn({ bots: [] })))} />);
    await user.click(await screen.findByRole('button', { name: 'Create your first bot' }));
    expect(screen.getByRole('dialog', { name: 'Meet your next bot.' })).toBeTruthy();
  });
});
