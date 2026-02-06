import { type Extension } from "@codemirror/state";
import { yCollab } from "y-codemirror.next";
import { Awareness } from "y-protocols/awareness";
import { WebsocketProvider } from "y-websocket";
import * as Y from "yjs";

export type ProviderStatus = "connected" | "disconnected";

export interface CollaborationSocketProvider {
  awareness: Awareness;
  connect(): void;
  disconnect(): void;
  destroy(): void;
  on(event: "status", handler: (event: { status: ProviderStatus }) => void): void;
}

interface ProviderFactoryInput {
  readonly doc: Y.Doc;
  readonly url: string;
  readonly room: string;
}

export type ProviderFactory = (
  input: ProviderFactoryInput,
) => CollaborationSocketProvider;

export type WebRtcProviderFactory = (
  input: ProviderFactoryInput,
) => CollaborationSocketProvider;

export interface CollaborationProviderOptions {
  readonly url: string;
  readonly room: string;
  readonly textName?: string;
  readonly doc?: Y.Doc;
  readonly connectOnCreate?: boolean;
  readonly providerFactory?: ProviderFactory;
  readonly webrtcSignalingUrl?: string;
  readonly webrtcProviderFactory?: WebRtcProviderFactory;
}

export class CollaborationProvider {
  readonly doc: Y.Doc;
  readonly provider: CollaborationSocketProvider;
  readonly webrtcProvider: CollaborationSocketProvider | null;
  readonly yText: Y.Text;

  private readonly ownsDoc: boolean;
  private connected = false;

  constructor(options: CollaborationProviderOptions) {
    this.doc = options.doc ?? new Y.Doc();
    this.ownsDoc = options.doc === undefined;
    this.yText = this.doc.getText(options.textName ?? "content");

    const providerFactory = options.providerFactory ?? defaultProviderFactory;
    this.provider = providerFactory({
      doc: this.doc,
      url: options.url,
      room: options.room,
    });
    this.webrtcProvider =
      options.webrtcProviderFactory && options.webrtcSignalingUrl
        ? options.webrtcProviderFactory({
            doc: this.doc,
            room: options.room,
            url: options.webrtcSignalingUrl,
          })
        : null;
    this.provider.on("status", ({ status }) => {
      this.connected = status === "connected";
    });

    if (options.connectOnCreate ?? true) {
      this.connect();
    }
  }

  extension(): Extension {
    return yCollab(this.yText, this.provider.awareness);
  }

  isConnected(): boolean {
    return this.connected;
  }

  connect(): void {
    this.provider.connect();
    this.webrtcProvider?.connect();
    this.connected = true;
  }

  disconnect(): void {
    this.provider.disconnect();
    this.webrtcProvider?.disconnect();
    this.connected = false;
  }

  reconnect(): void {
    this.disconnect();
    this.connect();
  }

  destroy(): void {
    this.provider.destroy();
    this.webrtcProvider?.destroy();
    this.connected = false;
    if (this.ownsDoc) {
      this.doc.destroy();
    }
  }
}

export function createCollaborationProvider(
  options: CollaborationProviderOptions,
): CollaborationProvider {
  return new CollaborationProvider(options);
}

function defaultProviderFactory({
  doc,
  url,
  room,
}: ProviderFactoryInput): CollaborationSocketProvider {
  return new WebsocketProvider(url, room, doc, { connect: false });
}
