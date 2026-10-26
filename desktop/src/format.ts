import type { Snapshot } from './domain';

export function connectionText(snapshot: Snapshot | null) {
  if (!snapshot) return 'Checking manager';
  switch (snapshot.connection) {
    case 'online':
      return 'Manager reachable';
    case 'offline':
      return 'Manager offline';
    case 'error':
      return 'Connection needs attention';
    case 'unknown':
      return 'Not connected';
  }
}
export function formatDate(seconds: number) {
  return new Date(seconds * 1000).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });
}
