import { useLayoutEffect, useRef, useState } from 'react';

type Spot = { x: number; y: number; w: number; h: number };

/// The selected option's highlight in a segmented control, as one piece that moves: put it first
/// inside the `.segmented` element. Choosing another option, it reaches across to it, then
/// lets go of the old one (as the phone's tab lens does), always inside the control.
export function SegmentLens() {
  const self = useRef<HTMLSpanElement>(null);
  const [spot, setSpot] = useState<Spot | null>(null);
  const [reach, setReach] = useState<Spot | null>(null);
  useLayoutEffect(() => {
    const group = self.current?.parentElement;
    if (!group) return;
    let last: Spot | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const place = () => {
      const on = group.querySelector<HTMLElement>(':scope > button.on');
      if (!on) {
        setSpot(null);
        return;
      }
      const next = { x: on.offsetLeft, y: on.offsetTop, w: on.offsetWidth, h: on.offsetHeight };
      if (last && last.x !== next.x && last.w > 0) {
        const left = Math.min(last.x, next.x);
        setReach({
          x: left,
          y: next.y,
          w: Math.max(last.x + last.w, next.x + next.w) - left,
          h: next.h,
        });
        clearTimeout(timer);
        timer = setTimeout(() => setReach(null), 140);
      }
      last = next;
      setSpot(next);
    };
    place();
    const moved = new MutationObserver(place);
    moved.observe(group, { subtree: true, attributes: true, attributeFilter: ['class'] });
    const resized = new ResizeObserver(place);
    resized.observe(group);
    return () => {
      clearTimeout(timer);
      moved.disconnect();
      resized.disconnect();
    };
  }, []);
  const at = reach ?? spot;
  return (
    <span
      ref={self}
      className="segment-lens"
      aria-hidden
      data-reaching={reach ? '' : undefined}
      style={
        at
          ? { width: at.w, height: at.h, transform: `translate(${at.x}px, ${at.y}px)` }
          : { opacity: 0 }
      }
    />
  );
}
