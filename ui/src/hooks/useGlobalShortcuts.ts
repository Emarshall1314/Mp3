import { useEffect } from 'react';
import { api, isAndroid, onGlobalShortcut } from '../api';
import { runAction } from '../shortcuts';
import { useLook } from './useAppearance';

/// Registers the owner's system-wide shortcuts (main window, desktop) and runs their actions.
export function useGlobalShortcuts() {
  const bindings = useLook((s) => s.look.globalShortcuts);
  const key = JSON.stringify(bindings);

  useEffect(() => {
    if (isAndroid) return;
    const unlisten = onGlobalShortcut(runAction);
    return () => void unlisten.then((f) => f());
  }, []);

  useEffect(() => {
    if (isAndroid) return;
    const pairs = Object.entries(JSON.parse(key) as Record<string, string>).filter(
      ([, combo]) => combo,
    );
    api.setGlobalShortcuts(pairs).then(
      (result) => useLook.setState({ globalShortcuts: result }),
      () => useLook.setState({ globalShortcuts: null }),
    );
  }, [key]);
}
