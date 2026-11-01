import {
  type ArtifactQuery,
  type SavedResults,
  type SourceTask,
  type Task,
  type PairedApplication,
  type RemoteSnapshot,
} from './paired-domain';
import {
  managerUpdate,
  selectNotices,
  updateFactsSchema,
  type DesktopNotice,
} from './update-domain';
type LibraryState =
  | { kind: 'closed' }
  | { kind: 'loading'; query: ArtifactQuery }
  | { kind: 'ready'; query: ArtifactQuery; results: SavedResults }
  | { kind: 'failed'; query: ArtifactQuery; issue: string };
type SourceState =
  | { kind: 'idle' }
  | { kind: 'loading'; id: Task['id'] }
  | { kind: 'ready'; source: SourceTask }
  | { kind: 'failed'; issue: string };
export type PairedState = {
  library: LibraryState;
  source: SourceState;
  snapshot: RemoteSnapshot | null;
  busy: boolean;
  issue: string | null;
  notice: string | null;
  notices: DesktopNotice[];
  managerVersion: { version: string; result: string } | null;
};
export class PairedController {
  private epoch = 0;
  private pageSerial = 0;
  private sourceSerial = 0;
  private exportSerial = 0;
  private activeExport: number | null = null;
  private stop: (() => void) | null = null;
  private listeners = new Set<() => void>();
  private state: PairedState = {
    snapshot: null,
    busy: false,
    issue: null,
    notice: null,
    library: { kind: 'closed' },
    source: { kind: 'idle' },
    notices: [],
    managerVersion: null,
  };
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
    if (
      snapshot.status !== 'online' ||
      old?.generation !== snapshot.generation ||
      old?.workspace?.manager !== snapshot.workspace?.manager
    )
      this.invalidateLibrary();
    this.set({ snapshot });
  };
  private scope() {
    const snapshot = this.state.snapshot;
    if (snapshot?.status !== 'online' || !snapshot.workspace) return null;
    return {
      epoch: this.epoch,
      generation: snapshot.generation,
      manager: snapshot.workspace.manager,
    };
  }
  private current(scope: ReturnType<PairedController['scope']>) {
    const now = this.scope();
    return (
      scope !== null &&
      now !== null &&
      scope.epoch === now.epoch &&
      scope.generation === now.generation &&
      scope.manager === now.manager
    );
  }
  private invalidateExport() {
    this.exportSerial += 1;
    if (this.activeExport !== null) {
      this.activeExport = null;
      this.set({ busy: false });
    }
  }
  private invalidateLibrary() {
    this.pageSerial += 1;
    this.sourceSerial += 1;
    this.invalidateExport();
    this.set({
      library: { kind: 'closed' },
      source: { kind: 'idle' },
      notice: null,
      issue: null,
    });
  }
  closeLibrary = () => this.invalidateLibrary();
  async library(query: ArtifactQuery = { bot: null, name: '', after: null, limit: 32 }) {
    const scope = this.scope();
    if (!scope) return;
    const serial = ++this.pageSerial;
    this.sourceSerial += 1;
    this.invalidateExport();
    this.set({
      library: { kind: 'loading', query },
      source: { kind: 'idle' },
      notice: null,
      issue: null,
    });
    try {
      const results = await this.app.savedResults(query);
      if (
        this.current(scope) &&
        serial === this.pageSerial &&
        results.generation === scope.generation &&
        results.manager === scope.manager
      )
        this.set({ library: { kind: 'ready', query, results } });
    } catch (error: unknown) {
      if (this.current(scope) && serial === this.pageSerial)
        this.set({
          library: {
            kind: 'failed',
            query,
            issue: error instanceof Error ? error.message : 'native_unavailable',
          },
        });
    }
  }
  async sourceTask(id: Task['id']) {
    const scope = this.scope();
    if (!scope) return;
    const serial = ++this.sourceSerial;
    this.set({ source: { kind: 'loading', id } });
    try {
      const source = await this.app.sourceTask(id);
      if (
        this.current(scope) &&
        serial === this.sourceSerial &&
        source.generation === scope.generation &&
        source.manager === scope.manager &&
        source.task.id === id
      )
        this.set({ source: { kind: 'ready', source } });
    } catch (error: unknown) {
      if (this.current(scope) && serial === this.sourceSerial)
        this.set({
          source: {
            kind: 'failed',
            issue: error instanceof Error ? error.message : 'native_unavailable',
          },
        });
    }
  }
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
    this.invalidateLibrary();
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
    const scope = this.scope();
    if (!scope) return;
    const serial = ++this.exportSerial;
    this.activeExport = serial;
    const current = () => this.current(scope) && serial === this.exportSerial;
    this.set({ busy: true, issue: null, notice: null });
    try {
      const saved = await action();
      if (current())
        this.set({ notice: saved ? 'The verified file was saved.' : 'Export cancelled.' });
    } catch (error: unknown) {
      if (current())
        this.set({ issue: error instanceof Error ? error.message : 'native_unavailable' });
    } finally {
      if (current()) {
        this.activeExport = null;
        this.set({ busy: false });
      }
    }
  }
  private acceptUpdate(facts: ReturnType<typeof updateFactsSchema.parse>) {
    const vaultLocked = this.state.snapshot?.workspace?.vault_unlocked === false;
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
    if (!this.app.updateFacts) return;
    try {
      this.acceptUpdate(updateFactsSchema.parse(await this.app.updateFacts()));
    } catch {
      return;
    }
  }
  async applyServerUpdate() {
    if (!this.app.applyServerUpdate) return;
    try {
      this.acceptUpdate(updateFactsSchema.parse(await this.app.applyServerUpdate()));
    } catch {
      return;
    }
  }
  async restartToUpdate() {
    await this.app.restartToUpdate?.();
  }
  async signOut() {
    this.epoch += 1;
    this.invalidateLibrary();
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
