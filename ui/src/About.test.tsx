import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { About } from './About';

describe('About', () => {
  it('shows build information from the core', async () => {
    render(
      <About
        load={() =>
          Promise.resolve({
            name: 'Cadenza',
            version: '0.1.0',
            targetOs: 'linux',
            targetArch: 'x86_64',
            debug: true,
          })
        }
      />,
    );
    expect(await screen.findByText('0.1.0')).toBeTruthy();
    expect(screen.getByText('linux · x86_64')).toBeTruthy();
  });

  it('reports a core failure instead of rendering stale data', async () => {
    render(<About load={() => Promise.reject(new Error('boom'))} />);
    expect((await screen.findByRole('alert')).textContent).toContain('boom');
  });
});
