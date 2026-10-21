import { useEffect, useState } from 'react';
import type { BuildInfo } from './core';

interface Props {
  load: () => Promise<BuildInfo>;
}

export function About({ load }: Props) {
  const [info, setInfo] = useState<BuildInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    load().then(setInfo, (e: unknown) => setError(String(e)));
  }, [load]);

  return (
    <main className="about">
      <h1>{info?.name ?? 'Cadenza'}</h1>
      {error && (
        <p className="error" role="alert">
          Could not reach the core: {error}
        </p>
      )}
      {info && (
        <dl aria-label="Build information">
          <dt>Version</dt>
          <dd>{info.version}</dd>
          <dt>Platform</dt>
          <dd>
            {info.targetOs} · {info.targetArch}
          </dd>
          <dt>Build</dt>
          <dd>{info.debug ? 'debug' : 'release'}</dd>
        </dl>
      )}
    </main>
  );
}
