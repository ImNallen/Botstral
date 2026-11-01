import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { ApplicationController } from './application';
import { application, signedIn } from './fixtures.test-support';
import type { UpdateFacts } from './update-domain';

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn(() => ({ matches: false })),
});

const identity = {
  version: '0.4.2',
  channel: 'stable',
  revision: 'a'.repeat(40),
  build_id: 'installed',
};
const available = { ...identity, version: '0.4.3', build_id: 'next' };
const report = {
  installed: {
    identity,
    protocols: { worker: 4, owner: 3 },
    link_schema_sha256: 'abc',
    component: 'manager',
  },
  last: null,
};

function facts(overrides: Partial<UpdateFacts> = {}): UpdateFacts {
  return {
    restart_version: null,
    report,
    available: null,
    protocol_match: true,
    ...overrides,
  };
}

describe('update notices', () => {
  it('restarts only after the restart banner is clicked', async () => {
    const user = userEvent.setup();
    const app = application(signedIn());
    app.updateFacts.mockResolvedValue(facts({ restart_version: '0.4.3' }));
    const controller = new ApplicationController(app);
    render(<App controller={controller} />);
    await screen.findByRole('button', { name: 'Research' });
    expect(screen.queryByRole('button', { name: 'Restart to update' })).toBeNull();
    expect(app.restartToUpdate).not.toHaveBeenCalled();
    await controller.checkUpdates();
    await user.click(await screen.findByRole('button', { name: 'Restart to update' }));
    expect(app.restartToUpdate).toHaveBeenCalledOnce();
    expect(screen.getByText(/0\.4\.3/)).toBeTruthy();
  });

  it('runs the server update when Update now is clicked', async () => {
    const user = userEvent.setup();
    const app = application(signedIn());
    const ready = facts({ available });
    app.updateFacts.mockResolvedValue(ready);
    app.applyServerUpdate.mockResolvedValue(
      facts({
        available,
        report: {
          ...report,
          last: {
            started_at: 1,
            finished_at: 2,
            trigger: 'now',
            outcome: { type: 'applied', previous: identity, current: available },
          },
        },
      }),
    );
    const controller = new ApplicationController(app);
    render(<App controller={controller} />);
    await screen.findByRole('button', { name: 'Research' });
    await controller.checkUpdates();
    await user.click(await screen.findByRole('button', { name: 'Update now' }));
    expect(app.applyServerUpdate).toHaveBeenCalledOnce();
    await user.click(screen.getByRole('button', { name: 'Show details' }));
    const details = screen.getByRole('complementary', { name: 'Workspace details' });
    expect(within(details).getByText('Updated')).toBeTruthy();
    expect(within(details).getByText('0.4.2')).toBeTruthy();
  });

  it('says the update was deferred', async () => {
    const app = application(signedIn());
    app.updateFacts.mockResolvedValue(
      facts({
        available,
        report: {
          ...report,
          last: {
            started_at: 1,
            finished_at: 2,
            trigger: 'now',
            outcome: { type: 'deferred', busy: { state: 'busy', reason: 'active_task' } },
          },
        },
      }),
    );
    const controller = new ApplicationController(app);
    render(<App controller={controller} />);
    await screen.findByRole('button', { name: 'Research' });
    await controller.checkUpdates();
    expect(await screen.findByText(/update was deferred/)).toBeTruthy();
  });

  it('shows the installed manager version in details', async () => {
    const user = userEvent.setup();
    const app = application(signedIn());
    app.updateFacts.mockResolvedValue(facts());
    const controller = new ApplicationController(app);
    render(<App controller={controller} />);
    await screen.findByRole('button', { name: 'Research' });
    await controller.checkUpdates();
    await user.click(screen.getByRole('button', { name: 'Show details' }));
    const details = screen.getByRole('complementary', { name: 'Workspace details' });
    expect(within(details).getByText('0.4.2')).toBeTruthy();
    expect(within(details).getByText('No update yet')).toBeTruthy();
  });
});
