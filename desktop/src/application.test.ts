import { describe, expect, it } from 'vitest';
import { ApplicationController } from './application';
import { ApplicationError, type Snapshot } from './domain';
import { application, deferred, signedIn, signedOut } from './fixtures.test-support';

describe('application lifecycle', () => {
  it('discards a late initial load after sign-out', async () => {
    const app = application();
    const waiting = deferred<Snapshot>();
    app.inspect.mockReturnValue(waiting.promise);
    const controller = new ApplicationController(app);
    const started = controller.start();
    await controller.signOut();
    waiting.resolve(signedIn());
    await started;
    expect(controller.getState().view).toEqual({ kind: 'ready', snapshot: signedOut() });
    expect(app.refresh).not.toHaveBeenCalled();
  });
  it('discards late creation results and clears protected drafts on explicit sign-out', async () => {
    const app = application();
    const controller = new ApplicationController(app);
    await controller.start();
    const waiting = deferred<Snapshot>();
    app.createBot.mockReturnValue(waiting.promise);
    controller.openCreate();
    const creating = controller.create('New bot');
    await controller.signOut();
    waiting.resolve(signedIn({ revision: 4 }));
    await creating;
    expect(controller.getState().creation).toEqual({ kind: 'closed' });
    expect(controller.getState().view).toEqual({ kind: 'ready', snapshot: signedOut() });
  });
  it('keeps the UUID and name through uncertain creation and refresh', async () => {
    const app = application();
    const controller = new ApplicationController(app);
    await controller.start();
    app.createBot.mockRejectedValueOnce(
      new ApplicationError('offline', signedIn({ revision: 3, connection: 'offline' })),
    );
    controller.openCreate();
    await controller.create('Uncertain bot');
    const first = app.createBot.mock.calls[0]?.[0];
    expect(first?.name).toBe('Uncertain bot');
    expect(first?.id).toMatch(/^[0-9a-f-]{36}$/);
    controller.closeCreate();
    await controller.refresh();
    controller.openCreate();
    await controller.create('A different name');
    expect(app.createBot.mock.calls[1]?.[0]).toEqual(first);
    expect(controller.getState().creation.kind).toBe('pending');
  });
  it('does not retry an uncertain creation on a different manager', async () => {
    const app = application();
    const controller = new ApplicationController(app);
    await controller.start();
    app.createBot.mockRejectedValueOnce(new ApplicationError('unauthorized', signedOut()));
    controller.openCreate();
    await controller.create('Old manager bot');
    expect(controller.getState().creation.kind).toBe('pending');
    await controller.signIn({ endpoint: '127.0.0.1:43122', password: 'SYNTHETIC password' });
    expect(controller.getState().creation.kind).toBe('closed');
  });
  it('does not replace a current snapshot with an older revision', async () => {
    const app = application(signedIn({ revision: 10 }));
    const controller = new ApplicationController(app);
    await controller.start();
    app.refresh.mockResolvedValueOnce(signedIn({ revision: 4 }));
    await controller.refresh();
    const view = controller.getState().view;
    expect(view.kind === 'ready' && view.snapshot.revision).toBe(10);
  });
});
