export type WsMessageType =
  | "hello"
  | "hello_ack"
  | "subscribe"
  | "yjs_update"
  | "ack"
  | "snapshot"
  | "awareness_update"
  | "error";

export const CURRENT_WS_PROTOCOL_VERSION = "scriptum-sync.v1";
export const PREVIOUS_WS_PROTOCOL_VERSION = "scriptum-sync.v0";
export const SUPPORTED_WS_PROTOCOL_VERSIONS = [
  CURRENT_WS_PROTOCOL_VERSION,
  PREVIOUS_WS_PROTOCOL_VERSION,
] as const;

export interface HelloMessage {
  type: "hello";
  protocol_version: string;
  session_token: string;
  resume_token?: string;
}

export interface HelloAckMessage {
  type: "hello_ack";
  server_time: string;
  resume_accepted: boolean;
  resume_token?: string;
  resume_expires_at?: string;
}

export interface SubscribeMessage {
  type: "subscribe";
  doc_id: string;
  last_server_seq?: number;
}

export interface YjsUpdateMessage {
  type: "yjs_update";
  doc_id: string;
  client_id: string;
  client_update_id: string;
  base_server_seq: number;
  payload_b64: string;
}

export interface AckMessage {
  type: "ack";
  doc_id: string;
  client_update_id: string;
  server_seq: number;
  applied: boolean;
}

export interface SnapshotMessage {
  type: "snapshot";
  doc_id: string;
  snapshot_seq: number;
  payload_b64: string;
}

export interface AwarenessPeer {
  [key: string]: unknown;
}

export interface AwarenessUpdateMessage {
  type: "awareness_update";
  doc_id: string;
  peers: AwarenessPeer[];
}

export interface ErrorMessage {
  type: "error";
  code: string;
  message: string;
  retryable: boolean;
  doc_id?: string;
}

export type WsMessage =
  | HelloMessage
  | HelloAckMessage
  | SubscribeMessage
  | YjsUpdateMessage
  | AckMessage
  | SnapshotMessage
  | AwarenessUpdateMessage
  | ErrorMessage;
