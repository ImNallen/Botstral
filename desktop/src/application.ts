import { PairedController } from './paired-application';
import {
  ApplicationError,
  botIdSchema,
  type Application,
  type BotId,
  type Creation,
  type ErrorCode,
  type Snapshot,
} from './domain';
import {
  managerUpdate,
  selectNotices,
  updateFactsSchema,
  type DesktopNotice,
} from './update-domain';

type View = { kind: 'starting' } | { kind: 'ready'; snapshot: Snapshot } | { kind: 'signing_out' };
type CreateState =
  | { kind: 'closed' }
  | { kind: 'editing' }
  | {
      kind: 'pending';
      draft: Creation;
      endpoint: string;
      status: 'sending' | 'uncertain';
      open: boolean;
    };
export type Activity = 'idle' | 'inspect' | 'sign_in' | 'refresh' | 'create_bot' | 'sign_out';
export type ViewState = {
  view: View;
  activity: Activity;
  error: ErrorCode | null;
  creation: CreateState;
  selected: BotId | null;
  notice: string | null;
  notices: DesktopNotice[];
  managerVersion: { version: string; result: string } | null;
};

export class ApplicationController {
  private epoch = 0;
  private listeners = new Set<() => void>();
  private state: ViewState = {
    view: { kind: 'starting' },
    activity: 'idle',
    error: null,
    creation: { kind: 'closed' },
    selected: null,
    notice: null,
    notices: [],
    managerVersion: null,
  };
  readonly paired: PairedController | null;
  constructor(private readonly app: Application) {
    this.paired = app.paired ? new PairedController(app.paired) : null;
  }
  getState = (): ViewState => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private set(change: Partial<ViewState>) {
    this.state = { ...this.state, ...change };
    for (const listener of this.listeners) listener();
  }
  private accept(snapshot: Snapshot) {
    const previous = this.state.view;
    if (
      previous.kind === 'ready' &&
      (snapshot.generation < previous.snapshot.generation ||
        (snapshot.generation === previous.snapshot.generation &&
          snapshot.revision < previous.snapshot.revision))
    )
      return;
    this.set({ view: { kind: 'ready', snapshot } });
  }
  private async run(activity: Activity, action: () => Promise<Snapshot>): Promise<Snapshot | null> {
    const epoch = this.epoch;
    this.set({ activity, error: null, notice: null });
    try {
      const result = await action();
      if (epoch !== this.epoch) return null;
      this.accept(result);
      return result;
    } catch (error: unknown) {
      if (epoch !== this.epoch) return null;
      const failure =
        error instanceof ApplicationError ? error : new ApplicationError('native_unavailable');
      if (failure.snapshot) this.accept(failure.snapshot);
      this.set({ error: failure.code });
      return null;
    } finally {
      if (epoch === this.epoch) this.set({ activity: 'idle' });
    }
  }
  async start() {
    const result = await this.run('inspect', () => this.app.inspect());
    if (result) await this.refresh();
  }
  async signIn(input: { endpoint: string; password: string }) {
    this.epoch += 1;
    const changedManager =
      this.state.creation.kind === 'pending' && this.state.creation.endpoint !== input.endpoint;
    if (changedManager) this.set({ creation: { kind: 'closed' } });
    await this.run('sign_in', () => this.app.signIn(input));
    if (changedManager)
      this.set({
        notice:
          'The saved creation was cleared when you changed managers. A previous request may still finish.',
      });
  }
  async refresh() {
    await this.run('refresh', () => this.app.refresh());
  }
  async signOut() {
    this.epoch += 1;
    this.set({ view: { kind: 'signing_out' }, selected: null, creation: { kind: 'closed' } });
    const pairedSignOut = this.paired?.signOut();
    const result = await this.run('sign_out', () => this.app.signOut());
    await pairedSignOut;
    if (result)
      this.set({
        notice:
          'Signed out on this device. Server revocation is unconfirmed. In-flight requests may still finish.',
      });
  }
  select(selected: BotId) {
    this.set({ selected });
  }
  openCreate() {
    this.set({
      error: null,
      creation:
        this.state.creation.kind === 'pending'
          ? { ...this.state.creation, open: true }
          : { kind: 'editing' },
    });
  }
  closeCreate() {
    if (this.state.creation.kind === 'pending')
      this.set({ creation: { ...this.state.creation, open: false } });
    else this.set({ creation: { kind: 'closed' } });
  }
  async create(name: string) {
    const previous = this.state.creation;
    const draft =
      previous.kind === 'pending'
        ? previous.draft
        : { id: botIdSchema.parse(crypto.randomUUID()), name };
    const epoch = this.epoch;
    const endpoint = this.state.view.kind === 'ready' ? this.state.view.snapshot.endpoint : '';
    this.set({ creation: { kind: 'pending', draft, endpoint, status: 'sending', open: true } });
    const result = await this.run('create_bot', () => this.app.createBot(draft));
    if (epoch !== this.epoch) return;
    if (
      result?.kind === 'signed_in' &&
      result.bots.some((bot) => bot.id === draft.id && bot.name === draft.name)
    )
      this.set({ creation: { kind: 'closed' }, selected: draft.id });
    else
      this.set({ creation: { kind: 'pending', draft, endpoint, status: 'uncertain', open: true } });
  }
  private acceptUpdate(facts: ReturnType<typeof updateFactsSchema.parse>) {
    const snapshot = this.state.view.kind === 'ready' ? this.state.view.snapshot : null;
    const vaultLocked = snapshot?.kind === 'signed_in' && snapshot.details.vault === 'locked';
    this.set({
      notices: selectNotices({
        restartVersion: facts.restart_version,
        report: facts.report,
        available: facts.available,
        protocolMatch: facts.protocol_match,
        vaultLocked,
      }),
      managerVersion: managerUpdate(facts.report),
    });
  }
  async checkUpdates() {
    try {
      this.acceptUpdate(updateFactsSchema.parse(await this.app.updateFacts()));
    } catch {
      return;
    }
  }
  async applyServerUpdate() {
    try {
      this.acceptUpdate(updateFactsSchema.parse(await this.app.applyServerUpdate()));
    } catch {
      return;
    }
  }
  async restartToUpdate() {
    await this.app.restartToUpdate();
  }
  async testNotification() {
    this.set({ notice: null });
    try {
      const result = await this.app.testNotification();
      this.set({
        notice:
          result === 'submitted'
            ? 'Test submitted to the operating system. Delivery is unconfirmed. Check system notification settings if it does not appear.'
            : result === 'denied'
              ? 'Notifications are denied. Review system notification settings.'
              : 'The operating system could not accept this test notification.',
      });
    } catch {
      this.set({ notice: 'The native app could not submit the test notification.' });
    }
  }
}
