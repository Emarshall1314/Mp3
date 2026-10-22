// Context-menu actions shared by every list of tracks and albums.
import { api } from './api';
import { attempt, useStore, type MenuItem } from './store';
import type { AlbumRow, TrackRow } from './types';

const nav = () => useStore.getState().navigate;
const toast = (m: string) => useStore.getState().showToast(m);

export function trackMenu(t: TrackRow): MenuItem[] {
  const items: MenuItem[] = [
    {
      label: 'Play Next',
      run: () =>
        void attempt(api.playNext([t.id]).then(() => toast(`“${t.title}” will play next`))),
    },
    {
      label: 'Play Later',
      run: () =>
        void attempt(api.playLater([t.id]).then(() => toast(`“${t.title}” added to the queue`))),
    },
  ];
  if (t.albumId !== null) {
    const id = t.albumId;
    items.push({ label: 'Go to Album', run: () => nav()({ name: 'album', id }) });
  }
  if (t.albumartist)
    items.push({
      label: 'Go to Artist',
      run: () => nav()({ name: 'artist', artist: t.albumartist }),
    });
  return items;
}

async function albumTrackIds(id: number): Promise<number[]> {
  return (await api.album(id)).tracks.map((t) => t.id);
}

export function albumMenu(a: AlbumRow): MenuItem[] {
  return [
    { label: 'Play', run: () => void attempt(api.playAlbum(a.id, 0)) },
    { label: 'Shuffle', run: () => void attempt(api.playAlbum(a.id, 0, true)) },
    {
      label: 'Play Next',
      run: () =>
        void attempt(
          albumTrackIds(a.id)
            .then(api.playNext)
            .then(() => toast(`“${a.title}” will play next`)),
        ),
    },
    {
      label: 'Play Later',
      run: () =>
        void attempt(
          albumTrackIds(a.id)
            .then(api.playLater)
            .then(() => toast(`“${a.title}” added to the queue`)),
        ),
    },
    { label: 'Go to Artist', run: () => nav()({ name: 'artist', artist: a.albumartist }) },
  ];
}

/// Right-click and keyboard context-menu key both open the menu at the pointer / element.
export function menuHandler(items: () => MenuItem[]) {
  return (e: React.MouseEvent) => {
    e.preventDefault();
    useStore.getState().openMenu(e.clientX, e.clientY, items());
  };
}
