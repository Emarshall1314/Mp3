import { useSyncExternalStore } from 'react';

/// Phone-sized screens get the phone layout: tabs, a mini player and full-screen Now Playing.
/// By width, so a narrow desktop window gets it too, and a tablet in landscape doesn't.
const QUERY = '(max-width: 700px)';

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
