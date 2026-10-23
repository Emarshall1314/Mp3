import { useEffect, useRef, useState } from 'react';
import { useStore } from '../store';

type Prompt = NonNullable<ReturnType<typeof useStore.getState>['prompt']>;

/// The one small dialog that asks for a line of text (a playlist's name). Enter confirms,
/// Escape or a tap outside cancels.
export function TextPrompt() {
  const prompt = useStore((s) => s.prompt);
  // A fresh form for each question, starting from its value.
  return prompt ? <PromptForm key={prompt.serial} prompt={prompt} /> : null;
}

function PromptForm({ prompt }: { prompt: Prompt }) {
  const close = useStore((s) => s.closePrompt);
  const [value, setValue] = useState(prompt.value);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    requestAnimationFrame(() => input.current?.select());
  }, []);
  const done = () => {
    close();
    prompt.run(value);
  };
  return (
    <div className="prompt-scrim" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <form
        className="prompt"
        role="dialog"
        aria-modal="true"
        aria-label={prompt.title}
        onSubmit={(e) => {
          e.preventDefault();
          done();
        }}
        onKeyDown={(e) => e.key === 'Escape' && close()}
      >
        <h2>{prompt.title}</h2>
        <input
          ref={input}
          type="text"
          value={value}
          aria-label={prompt.title}
          onChange={(e) => setValue(e.target.value)}
        />
        <div className="prompt-buttons">
          <button type="button" className="pill" onClick={close}>
            Cancel
          </button>
          <button type="submit" className="pill primary">
            {prompt.confirm}
          </button>
        </div>
      </form>
    </div>
  );
}
