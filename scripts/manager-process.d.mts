export type ManagerProcess = {
  mainURL: string;
  setupURL?: string;
  stop(): Promise<void>;
};

export function startManager(options: {
  binary: string;
  dataDir: string;
  port?: number;
  setup?: boolean;
  cwd?: string;
}): Promise<ManagerProcess>;
