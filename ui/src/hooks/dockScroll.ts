// When scrolling shrinks or opens the phone's dock. A flick back and forth, or the small
// reversals in a long scroll, shouldn't make it flip: it needs a clear run one way, and once it
// has flipped it waits for its animation to finish before it can flip back.

export interface Travel {
  /// The way the page has been going: 1 down, −1 up, 0 not yet.
  dir: 1 | -1 | 0;
  /// How far it has gone that way without turning back, in px.
  run: number;
  /// When the dock last flipped, in ms.
  flippedAt: number;
}

export const START: Travel = { dir: 0, run: 0, flippedAt: -Infinity };

/// How far the page must go one way before the dock follows.
export const RUN = 48;
/// How long after a flip before it can flip again (its animation).
export const SETTLE = 480;

/// The dock's next state after a scroll from `before` to `top` (scrollTop) at `now`.
export function dockAfterScroll(
  collapsed: boolean,
  travel: Travel,
  before: number,
  top: number,
  now: number,
): { collapsed: boolean; travel: Travel } {
  const delta = top - before;
  const dir: 1 | -1 | 0 = delta > 0 ? 1 : delta < 0 ? -1 : 0;
  const run =
    dir === 0 ? travel.run : dir === travel.dir ? travel.run + Math.abs(delta) : Math.abs(delta);
  const going: Travel = { dir: dir || travel.dir, run, flippedAt: travel.flippedAt };
  const flip = (to: boolean) => ({
    collapsed: to,
    travel: { dir: going.dir, run: 0, flippedAt: now },
  });
  // Back at the top, the dock is always open.
  if (top < 24) return collapsed ? flip(false) : { collapsed, travel: going };
  if (now - travel.flippedAt < SETTLE) return { collapsed, travel: going };
  if (!collapsed && going.dir === 1 && run >= RUN && top > 80) return flip(true);
  if (collapsed && going.dir === -1 && run >= RUN) return flip(false);
  return { collapsed, travel: going };
}
