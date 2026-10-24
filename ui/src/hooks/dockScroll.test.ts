import { describe, expect, it } from 'vitest';
import { dockAfterScroll, START, type Travel } from './dockScroll';

/// Scrolls through `tops`, 16 ms apart, from scrollTop `from`.
function scroll(from: number, tops: number[], collapsed = false, travel: Travel = START, t0 = 0) {
  let state = { collapsed, travel };
  let before = from;
  let now = t0;
  const seen: boolean[] = [];
  for (const top of tops) {
    now += 16;
    state = dockAfterScroll(state.collapsed, state.travel, before, top, now);
    before = top;
    seen.push(state.collapsed);
  }
  return { ...state, seen, now };
}

describe('the dock and scrolling', () => {
  it('shrinks after a clear run down, not at the first pixels', () => {
    const r = scroll(100, [110, 120, 130, 140, 150, 160]);
    expect(r.seen).toEqual([false, false, false, false, true, true]);
  });

  it('ignores small reversals in a long scroll', () => {
    const r = scroll(100, [150, 200, 195, 250, 245, 300]);
    expect(r.seen.slice(1)).toEqual([true, true, true, true, true]);
  });

  it("doesn't flip back while it is still animating", () => {
    const down = scroll(100, [160]);
    expect(down.collapsed).toBe(true);
    // Straight back up within the animation: it stays shrunk until the animation is done.
    const up = scroll(160, [100, 90], true, down.travel, down.now);
    expect(up.collapsed).toBe(true);
    const later = scroll(90, [80, 20], true, up.travel, up.now + 600);
    expect(later.collapsed).toBe(false);
  });

  it('always opens at the top of the page', () => {
    const r = scroll(30, [10], true, { dir: 1, run: 0, flippedAt: 0 }, 1);
    expect(r.collapsed).toBe(false);
  });
});
