import { useEffect, useRef, type RefObject } from 'react';
import { useStore } from '../store';

/// Modal dialog behaviour: focus moves into `ref` (its `[data-autofocus]` element, else the first
/// control), Tab stays inside, Esc calls `onEscape`, and focus returns to the opener on close.
export function useModal(open: boolean, ref: RefObject<HTMLElement | null>, onEscape: () => void) {
  // The latest handler, without re-running the effect (which would move focus) when it changes.
  const escape = useRef(onEscape);
  useEffect(() => {
    escape.current = onEscape;
  }, [onEscape]);
  useEffect(() => {
    if (!open) return;
    const opener = document.activeElement;
    const first =
      ref.current?.querySelector<HTMLElement>('[data-autofocus]') ??
      ref.current?.querySelector<HTMLElement>('button, input, textarea, select');
    first?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.defaultPrevented && !useStore.getState().menu) {
        e.preventDefault();
        escape.current();
      }
      if (e.key !== 'Tab' || !ref.current) return;
      const focusable = [
        ...ref.current.querySelectorAll<HTMLElement>(
          'button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex]:not([tabindex="-1"])',
        ),
      ];
      const head = focusable[0];
      const tail = focusable.at(-1);
      if (!head || !tail) return;
      const inside = ref.current.contains(document.activeElement);
      if (e.shiftKey && (document.activeElement === head || !inside)) {
        e.preventDefault();
        tail.focus();
      } else if (!e.shiftKey && (document.activeElement === tail || !inside)) {
        e.preventDefault();
        head.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [open, ref]);
}
