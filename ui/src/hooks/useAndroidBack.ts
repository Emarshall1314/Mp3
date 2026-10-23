// Android's back button and back gesture, handled by the app rather than closing it: they undo
// one step at a time, as in other Android apps, and from Home the app goes to the background
// with the music still playing.

import { useEffect } from 'react';
import { onBackButtonPress } from '@tauri-apps/api/app';
import { api } from '../api';
import { useStore } from '../store';

export type BackAction = 'menu' | 'songInfo' | 'nowPlaying' | 'page' | 'home' | 'background';

/// What one press of back does: closes what's on top, else goes back a page, else to Home,
/// else leaves the app.
export function backAction(s: {
  menu: unknown;
  songInfo: unknown;
  nowPlaying: boolean;
  back: unknown[];
  route: { name: string };
}): BackAction {
  if (s.menu) return 'menu';
  if (s.songInfo) return 'songInfo';
  if (s.nowPlaying) return 'nowPlaying';
  if (s.back.length > 0) return 'page';
  if (s.route.name !== 'home') return 'home';
  return 'background';
}

export function useAndroidBack() {
  useEffect(() => {
    let live = true;
    let stop: (() => void) | null = null;
    onBackButtonPress(() => {
      const s = useStore.getState();
      switch (backAction(s)) {
        case 'menu':
          return s.closeMenu();
        case 'songInfo':
          return s.closeSongInfo();
        case 'nowPlaying':
          return s.setNowPlaying(false);
        case 'page':
          return s.goBack();
        case 'home':
          return s.navigate({ name: 'home' });
        case 'background':
          return void api.toBackground().catch(() => undefined);
      }
    }).then(
      (listener) => {
        if (live) stop = () => void listener.unregister();
        else void listener.unregister();
      },
      // Not Android, or an older shell: the system's own back stays.
      () => undefined,
    );
    return () => {
      live = false;
      stop?.();
    };
  }, []);
}
