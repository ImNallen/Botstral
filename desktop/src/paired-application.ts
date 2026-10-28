import { type PairedApplication, type RemoteSnapshot } from './paired-domain';
export type PairedState = {
  snapshot: RemoteSnapshot | null;
  busy: boolean;
  issue: string | null;
  notice: string | null;
};
export class PairedController {
  private epoch = 0;
  private stop: (() => void) | null = null;
  private listeners = new Set<() => void>();
  private state: PairedState = { snapshot: null, busy: false, issue: null, notice: null };
  constructor(readonly app: PairedApplication) {}
  getState = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private set(change: Partial<PairedState>) {
    this.state = { ...this.state, ...change };
    for (const listener of this.listeners) listener();
  }
  accept = (snapshot: RemoteSnapshot) => {
    const old = this.state.snapshot;
    if (
      old &&
      (snapshot.generation < old.generation ||
        (snapshot.generation === old.generation && snapshot.revision < old.revision))
    )
      return;
    this.set({ snapshot });
  };
  async start() {
    const epoch = this.epoch;
    let stop: () => void;
    try {
      stop = await this.app.observe((snapshot) => {
        if (epoch === this.epoch) this.accept(snapshot);
      });
    } catch {
      this.set({ issue: 'native_unavailable' });
      return;
    }
    if (epoch !== this.epoch) {
      stop();
      return;
    }
    this.stop?.();
    this.stop = stop;
    await this.run(() => this.app.inspect());
  }
  dispose() {
    this.epoch += 1;
    this.stop?.();
    this.stop = null;
  }
  async run(action: () => Promise<RemoteSnapshot>) {
    const epoch = this.epoch;
    this.set({ busy: true, issue: null, notice: null });
    try {
      const snapshot = await action();
      if (epoch === this.epoch) this.accept(snapshot);
    } catch (error: unknown) {
      if (epoch === this.epoch) {
        this.set({ issue: error instanceof Error ? error.message : 'native_unavailable' });
      }
    } finally {
      if (epoch === this.epoch) this.set({ busy: false });
    }
  }
  async export(action: () => Promise<boolean>) {
    const epoch = this.epoch;
    this.set({ busy: true, issue: null, notice: null });
    try {
      const saved = await action();
      if (epoch === this.epoch)
        this.set({ notice: saved ? 'The verified file was saved.' : 'Export cancelled.' });
    } catch (error: unknown) {
      if (epoch === this.epoch)
        this.set({ issue: error instanceof Error ? error.message : 'native_unavailable' });
    } finally {
      if (epoch === this.epoch) this.set({ busy: false });
    }
  }
  async signOut() {
    this.epoch += 1;
    this.stop?.();
    this.stop = null;
    this.set({
      snapshot: null,
      busy: true,
      issue: null,
      notice: 'Signed out locally. Admitted effects may finish. Server revocation is separate.',
    });
    await this.run(() => this.app.signOut());
    await this.start();
  }
}
