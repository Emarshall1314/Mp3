import { useEffect, type RefObject } from 'react';

/// Closes a popover on Escape (focus returns to its button), on a click outside, and when
/// keyboard focus leaves it. `ref` wraps the button and the popover.
export function usePopover(
  open: boolean,
  setOpen: (open: boolean) => void,
  ref: RefObject<HTMLElement | null>,
) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      setOpen(false);
      ref.current?.querySelector('button')?.focus();
    };
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const onFocus = (e: FocusEvent) => {
      if (e.target instanceof Node && !ref.current?.contains(e.target)) setOpen(false);
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('mousedown', onDown);
    window.addEventListener('focusin', onFocus);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('mousedown', onDown);
      window.removeEventListener('focusin', onFocus);
    };
  }, [open, setOpen, ref]);
}
