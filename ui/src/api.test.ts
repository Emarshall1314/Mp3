import { describe, expect, it, vi } from 'vitest';

// The core opens after the page starts (as on Android): `ready` says no three times first.
const calls: string[] = [];
let checks = 0;
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string) => {
    calls.push(command);
    if (command === 'ready') return Promise.resolve(++checks > 3);
    if (checks <= 3) return Promise.reject(new Error('state not managed for field `app`'));
    return Promise.resolve({ appearance: 'saved' });
  },
}));

describe('calls into the core', () => {
  it('wait until the core is open', async () => {
    const { api } = await import('./api');
    const [a, b] = await Promise.all([api.settings(), api.stats()]);
    expect(a).toEqual({ appearance: 'saved' });
    expect(b).toEqual({ appearance: 'saved' });
    // Four checks, shared by both calls, then the calls themselves.
    expect(calls).toEqual(['ready', 'ready', 'ready', 'ready', 'settings', 'stats']);
  });
});

describe('artwork sizes', () => {
  it('snap up to a few cached sizes', async () => {
    const { artSize } = await import('./api');
    expect(artSize(48, 1)).toBe(64);
    expect(artSize(181, 2.625)).toBe(512);
    expect(artSize(170, 2.625)).toBe(512);
    expect(artSize(48, 2.625)).toBe(128);
    expect(artSize(1024, 2.625)).toBe(2048);
  });
});
