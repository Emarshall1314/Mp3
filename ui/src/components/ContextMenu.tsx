import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useStore } from '../store';

/// The single context menu (right-click / long-press), positioned to stay on screen.
export function ContextMenu() {
  const menu = useStore((s) => s.menu);
  const close = useStore((s) => s.closeMenu);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  // Where focus was when the menu opened; it goes back there when the menu closes.
  const opener = useRef<Element | null>(null);

  useLayoutEffect(() => {
    if (!menu) {
      if (opener.current instanceof HTMLElement && opener.current.isConnected)
        opener.current.focus();
      opener.current = null;
      return;
    }
    if (!ref.current) return;
    opener.current ??= document.activeElement;
    const r = ref.current.getBoundingClientRect();
    setPos({
      x: Math.min(menu.x, window.innerWidth - r.width - 8),
      y: Math.min(menu.y, window.innerHeight - r.height - 8),
    });
    ref.current.querySelector('button')?.focus();
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' || e.key === 'Tab') {
        e.preventDefault();
        close();
        return;
      }
      const items = [...(ref.current?.querySelectorAll('button') ?? [])];
      const at = items.indexOf(document.activeElement as HTMLButtonElement);
      const to =
        e.key === 'ArrowDown'
          ? (at + 1) % items.length
          : e.key === 'ArrowUp'
            ? (at - 1 + items.length) % items.length
            : e.key === 'Home'
              ? 0
              : e.key === 'End'
                ? items.length - 1
                : null;
      if (to !== null) {
        e.preventDefault();
        items[to]?.focus();
      }
    };
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('mousedown', onDown);
    window.addEventListener('blur', close);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('mousedown', onDown);
      window.removeEventListener('blur', close);
    };
  }, [menu, close]);

  useEffect(() => {
    // Shift+F10 and the Menu key open the focused item's menu, placed by the item.
    const onKey = (e: KeyboardEvent) => {
      if (!(e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10'))) return;
      const target = document.activeElement;
      if (!(target instanceof HTMLElement) || target === document.body) return;
      e.preventDefault();
      const r = target.getBoundingClientRect();
      target.dispatchEvent(
        new MouseEvent('contextmenu', {
          bubbles: true,
          cancelable: true,
          clientX: r.left + Math.min(24, r.width / 2),
          clientY: r.top + r.height / 2,
        }),
      );
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  if (!menu) return null;
  return (
    <div
      ref={ref}
      className="context-menu"
      role="menu"
      style={{
        left: pos?.x ?? menu.x,
        top: pos?.y ?? menu.y,
        visibility: pos ? 'visible' : 'hidden',
      }}
    >
      {menu.items.map((item) => (
        <button
          key={item.label}
          role="menuitem"
          onClick={() => {
            close();
            item.run();
          }}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
