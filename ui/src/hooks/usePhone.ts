import { useSyncExternalStore } from 'react';

/// The phone layout (tabs, a mini player, full-screen Now Playing): always on Android, in any
/// orientation (a phone turned sideways is still a phone), and on the desktop in a narrow
/// window.
const QUERY = /Android/.test(navigator.userAgent) ? '(min-width: 0px)' : '(max-width: 700px)';

function subscribe(changed: () => void) {
  const m = matchMedia(QUERY);
  m.addEventListener('change', changed);
  return () => m.removeEventListener('change', changed);
}

export function usePhone(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => matchMedia(QUERY).matches,
    () => false,
  );
}
