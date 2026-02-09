// localStorage keys — derived from contracts/storage-keys.json.

export const STORAGE_KEYS = {
  access_token: "scriptum:access_token",
  access_expires_at: "scriptum:access_expires_at",
  refresh_token: "scriptum:refresh_token",
  refresh_expires_at: "scriptum:refresh_expires_at",
  user: "scriptum:user",
  oauth_state: "scriptum:oauth_state",
  code_verifier: "scriptum:code_verifier",
  flow_id: "scriptum:flow_id",
} as const;

export type StorageKey = (typeof STORAGE_KEYS)[keyof typeof STORAGE_KEYS];
