// Typed wrappers over the Tauri commands exposed by the shell. Every call into the core goes
// through this module so the UI never builds command names by hand.
import { invoke } from '@tauri-apps/api/core';

export interface BuildInfo {
  name: string;
  version: string;
  targetOs: string;
  targetArch: string;
  debug: boolean;
}

export function buildInfo(): Promise<BuildInfo> {
  return invoke<BuildInfo>('build_info');
}
