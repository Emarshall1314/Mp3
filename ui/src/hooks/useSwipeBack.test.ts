import { describe, expect, it } from 'vitest';
import { swipeAxis, swipeDone } from './useSwipeBack';

describe('swipes', () => {
  it('wait to see which way a drag goes, favouring up and down', () => {
    expect(swipeAxis(5, 3)).toBeNull();
    expect(swipeAxis(30, 4)).toBe('x');
    expect(swipeAxis(20, 18)).toBe('y');
    expect(swipeAxis(-30, 2)).toBe('x');
  });

  it('finish when far enough across or flicked', () => {
    expect(swipeDone(140, 900, 412)).toBe(true);
    expect(swipeDone(60, 600, 412)).toBe(false);
    expect(swipeDone(60, 60, 412)).toBe(true);
    expect(swipeDone(20, 10, 412)).toBe(false);
  });
});
