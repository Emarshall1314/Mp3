import { useEffect } from 'react';

/// How long a press must last to open the menu, and how far the finger may wander.
const HOLD_MS = 500;
const SLOP_PX = 10;

/// Long-pressing anything with a context menu opens it, as right-clicking does: the press sends
/// the same `contextmenu` event the menus already handle. Touch only; the system's own
/// long-press menu (if the WebView sends one) is ignored for a moment after, so it doesn't open
/// twice.
export function useLongPress() {
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let start: { x: number; y: number } | null = null;
    let firedAt = 0;
    const cancel = () => {
      clearTimeout(timer);
      start = null;
    };
    const down = (e: PointerEvent) => {
      if (e.pointerType !== 'touch') return;
      cancel();
      const target = e.target as Element;
      start = { x: e.clientX, y: e.clientY };
      timer = setTimeout(() => {
        if (!start) return;
        firedAt = Date.now();
        target.dispatchEvent(
          new MouseEvent('contextmenu', {
            bubbles: true,
            cancelable: true,
            clientX: start.x,
            clientY: start.y,
          }),
        );
        navigator.vibrate?.(10);
        start = null;
      }, HOLD_MS);
    };
    const move = (e: PointerEvent) => {
      if (start && Math.hypot(e.clientX - start.x, e.clientY - start.y) > SLOP_PX) cancel();
    };
    const native = (e: MouseEvent) => {
      if (e.isTrusted && Date.now() - firedAt < 800) {
        e.preventDefault();
        e.stopPropagation();
      }
    };
    // A long press that opened a menu shouldn't also count as a tap when the finger lifts.
    const click = (e: MouseEvent) => {
      if (Date.now() - firedAt < 800) {
        e.preventDefault();
        e.stopPropagation();
      }
    };
    window.addEventListener('pointerdown', down, true);
    window.addEventListener('pointermove', move, true);
    window.addEventListener('pointerup', cancel, true);
    window.addEventListener('pointercancel', cancel, true);
    window.addEventListener('contextmenu', native, true);
    window.addEventListener('click', click, true);
    return () => {
      cancel();
      window.removeEventListener('pointerdown', down, true);
      window.removeEventListener('pointermove', move, true);
      window.removeEventListener('pointerup', cancel, true);
      window.removeEventListener('pointercancel', cancel, true);
      window.removeEventListener('contextmenu', native, true);
      window.removeEventListener('click', click, true);
    };
  }, []);
}
