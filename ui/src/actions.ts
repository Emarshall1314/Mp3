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
  items.push({ label: 'Get Info…', run: () => useStore.getState().openSongInfo([t.id]) });
  items.push({
    label: t.favourite ? 'Remove from Favourites' : 'Add to Favourites',
    run: () => void setFavourite([t.id], !t.favourite),
  });
  items.push({ label: 'Move to Trash', run: () => void moveToTrash([t.id], `“${t.title}”`) });
  return items;
}

export async function setFavourite(ids: number[], on: boolean) {
  await attempt(api.setFavourite(ids, on));
}

/// Trashes songs, with Undo in the toast. With sync on, other devices' copies go too.
export async function moveToTrash(ids: number[], what: string) {
  const items = await attempt(api.trash(ids));
  if (!items?.length) return;
  useStore.getState().libraryChanged();
  useStore.getState().showToast(`Moved ${what} to the Trash`, {
    label: 'Undo',
    run: () =>
      void Promise.all(items.map((i) => attempt(api.restore(i.id)))).then(() =>
        useStore.getState().libraryChanged(),
      ),
  });
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
    {
      label: 'Get Info…',
      run: () =>
        void attempt(albumTrackIds(a.id).then((ids) => useStore.getState().openSongInfo(ids))),
    },
  ];
}

/// Right-click and keyboard context-menu key both open the menu at the pointer / element.
export function menuHandler(items: () => MenuItem[]) {
  return (e: React.MouseEvent) => {
    e.preventDefault();
    useStore.getState().openMenu(e.clientX, e.clientY, items());
  };
}
