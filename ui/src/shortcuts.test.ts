import { beforeEach, describe, expect, it, vi } from 'vitest';

const calls: string[] = [];
vi.mock('./api', () => ({
  isAndroid: false,
  api: new Proxy(
    {},
    {
      get:
        (_, name: string) =>
        (...args: unknown[]) => {
          calls.push(`${name}(${args.join(',')})`);
          return Promise.resolve(null);
        },
    },
  ),
}));

const { handleShortcut } = await import('./shortcuts');
const { useStore } = await import('./store');

function press(key: string, init: KeyboardEventInit = {}, target: HTMLElement = document.body) {
  const e = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  target.addEventListener('keydown', handleShortcut, { once: true });
  target.dispatchEvent(e);
  return e;
}

describe('shortcuts', () => {
  beforeEach(() => {
    calls.length = 0;
    useStore.setState({
      player: {
        status: 'playing',
        trackId: 1,
        positionMs: 30_000,
        volume: 0.5,
      } as never,
    });
  });

  it('plays and pauses with Space, but not while typing or on a button', () => {
    press(' ');
    expect(calls).toEqual(['toggle()']);
    const input = document.body.appendChild(document.createElement('input'));
    const button = document.body.appendChild(document.createElement('button'));
    expect(press(' ', {}, input).defaultPrevented).toBe(false);
    expect(press(' ', {}, button).defaultPrevented).toBe(false);
    expect(calls).toEqual(['toggle()']);
  });

  it('skips tracks, seeks and changes volume with arrows', () => {
    press('ArrowRight', { ctrlKey: true });
    press('ArrowLeft', { ctrlKey: true });
    press('ArrowRight', { shiftKey: true });
    press('ArrowLeft', { shiftKey: true });
    press('ArrowUp', { ctrlKey: true });
    expect(calls).toEqual([
      'next()',
      'previous()',
      'seek(40000)',
      'seek(20000)',
      'setVolume(0.55)',
    ]);
  });

  it('navigates and opens views', () => {
    press(',', { ctrlKey: true });
    expect(useStore.getState().route).toEqual({ name: 'settings' });
    press('ArrowLeft', { altKey: true });
    expect(useStore.getState().route).toEqual({ name: 'home' });
    press('l', { ctrlKey: true });
    expect(useStore.getState().nowPlaying).toBe(true);
    expect(useStore.getState().nowPlayingPanel).toBe('lyrics');
  });
});
