import { beforeEach, describe, expect, it } from 'vitest';
import { useStore } from './store';

describe('where music came from', () => {
  beforeEach(() => {
    useStore.setState({ route: { name: 'home' }, back: [], forward: [], playedFrom: null });
  });

  it('returns to the album, with the grid behind it, after wandering off', () => {
    const s = useStore.getState();
    s.navigate({ name: 'albums' });
    s.navigate({ name: 'album', id: 7 });
    s.notePlaySource();
    s.navigate({ name: 'settings' });
    s.navigate({ name: 'home' });
    useStore.getState().goToPlaySource();
    expect(useStore.getState().route).toEqual({ name: 'album', id: 7 });
    useStore.getState().goBack();
    expect(useStore.getState().route).toEqual({ name: 'albums' });
  });

  it('counts an album played from a grid as coming from that album', () => {
    const s = useStore.getState();
    s.navigate({ name: 'albums' });
    s.notePlaySource(3);
    s.navigate({ name: 'home' });
    useStore.getState().goToPlaySource();
    expect(useStore.getState().route).toEqual({ name: 'album', id: 3 });
    useStore.getState().goBack();
    expect(useStore.getState().route).toEqual({ name: 'albums' });
  });
});
