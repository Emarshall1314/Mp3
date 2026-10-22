import { useEffect, useRef, useState } from 'react';
import { Check, Speaker } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { Outputs } from '../types';

/// Output device menu in the player bar. The list is fetched each time it opens, so devices
/// plugged in since are there.
export function OutputPicker() {
  const [outputs, setOutputs] = useState<Outputs | null>(null);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    let live = true;
    void api.outputs().then((o) => live && setOutputs(o));
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('mousedown', onDown);
    return () => {
      live = false;
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('mousedown', onDown);
    };
  }, [open]);

  const missing =
    outputs && !outputs.devices.some((d) => d.id === outputs.current) ? outputs.current : null;
  const current = outputs?.devices.find((d) => d.id === outputs.current);
  const dop = current ? (outputs?.dop.includes(current.id) ?? false) : false;

  return (
    <div className="output-picker" ref={ref}>
      <button
        className={`icon-button ${open ? 'active' : ''}`}
        aria-label="Output device"
        aria-haspopup="menu"
        aria-expanded={open}
        title="Output device"
        onClick={() => setOpen(!open)}
      >
        <Speaker size={18} />
      </button>
      {open && (
        <div className="popover output-menu" role="menu" aria-label="Output device">
          <h2 className="popover-title">Output</h2>
          {!outputs && <p className="muted">Looking for devices…</p>}
          {outputs?.devices.map((d) => {
            const current = d.id === outputs.current;
            return (
              <button
                key={d.id}
                role="menuitemradio"
                aria-checked={current}
                className="output-item"
                onClick={() => {
                  setOpen(false);
                  if (!current)
                    void attempt(api.setOutput(d.id)).then(() =>
                      useStore.getState().settingsChanged(),
                    );
                }}
              >
                <span className="check">{current && <Check size={14} />}</span>
                <span className="output-text">
                  <span>{d.name}</span>
                  <span className="muted small">
                    {d.exclusive
                      ? `Exclusive · bit-perfect capable · ${d.id}`
                      : 'Shared · through the system mixer'}
                  </span>
                </span>
              </button>
            );
          })}
          {current?.exclusive && (
            <label className="output-option">
              <input
                type="checkbox"
                checked={dop}
                onChange={(e) => {
                  const on = e.target.checked;
                  void attempt(
                    api.setDop(current.id, on).then(() => api.outputs().then(setOutputs)),
                  );
                }}
              />
              <span className="output-text">
                <span>Send DSD as DoP</span>
                <span className="muted small wrap">
                  Only if this DAC decodes DoP; others play it as loud noise. Native DSD is used
                  automatically when the driver offers it.
                </span>
              </span>
            </label>
          )}
          {missing && (
            <p className="muted small" role="status">
              The selected device ({missing}) is not connected.
            </p>
          )}
        </div>
      )}
    </div>
  );
}
