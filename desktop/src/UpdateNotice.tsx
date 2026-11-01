import { updateCopy, type DesktopNotice } from './update-domain';

export function UpdateNotice({
  notices,
  busy,
  onRestart,
  onApply,
}: {
  notices: DesktopNotice[];
  busy: boolean;
  onRestart: () => void;
  onApply: () => void;
}) {
  return (
    <>
      {notices.map((notice) => {
        if (notice.type === 'restart_ready') {
          return (
            <div className="banner notice-banner" role="status" key="restart">
              <span>
                {updateCopy.restart} {notice.version}
              </span>
              <button disabled={busy} onClick={onRestart}>
                {updateCopy.restart}
              </button>
            </div>
          );
        }
        if (notice.type === 'server_available') {
          return (
            <div className="banner notice-banner" role="status" key="server">
              <span>
                {updateCopy.server} {notice.available.version}
              </span>
              <button disabled={busy} onClick={onApply}>
                {updateCopy.apply}
              </button>
            </div>
          );
        }
        if (notice.type === 'update_deferred') {
          return (
            <div className="banner notice-banner" role="status" key="deferred">
              {updateCopy.deferred}
            </div>
          );
        }
        if (notice.type === 'vault_locked') {
          return (
            <div className="banner notice-banner" role="status" key="vault">
              {updateCopy.vault}
            </div>
          );
        }
        return (
          <div className="banner error-banner" role="alert" key="protocol">
            {updateCopy.protocol}
          </div>
        );
      })}
    </>
  );
}
