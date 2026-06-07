// Thin wrapper around Tauri's `invoke`.
//
// In the real desktop app this just forwards to `@tauri-apps/api/core`. When
// the frontend is opened in a plain browser for fast UI iteration (no Tauri
// runtime), it transparently routes calls to the dev mock backend so every
// screen can render. The mock path is dead code in production builds
// (`import.meta.env.DEV` is false) and is never hit inside the Tauri webview.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { isMockEnv, mockInvoke } from "../dev/mockBackend";

export function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isMockEnv()) {
    return mockInvoke<T>(command, args);
  }
  return tauriInvoke<T>(command, args);
}
