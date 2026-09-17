export type ServerMessage = {
  kind: string;
  payload?: unknown;
};

export type ConnectionMode = "sse" | "polling" | "connecting";

export type ConnectionListener = (message: ServerMessage) => void;
export type ModeListener = (mode: ConnectionMode) => void;

export class Connection {
  private eventSource: EventSource | null = null;
  private polling = false;
  private stopped = false;
  private cursor = 0;
  private messageListeners = new Set<ConnectionListener>();
  private modeListeners = new Set<ModeListener>();
  private mode: ConnectionMode = "connecting";

  constructor(private readonly base: string = "") {}

  start(): void {
    this.stopped = false;
    this.connectSse();
  }

  stop(): void {
    this.stopped = true;
    this.eventSource?.close();
    this.eventSource = null;
    this.polling = false;
  }

  onMessage(listener: ConnectionListener): () => void {
    this.messageListeners.add(listener);
    return () => this.messageListeners.delete(listener);
  }

  onModeChange(listener: ModeListener): () => void {
    this.modeListeners.add(listener);
    listener(this.mode);
    return () => this.modeListeners.delete(listener);
  }

  private setMode(mode: ConnectionMode): void {
    if (this.mode === mode) return;
    this.mode = mode;
    for (const listener of this.modeListeners) listener(mode);
  }

  private emit(message: ServerMessage): void {
    for (const listener of this.messageListeners) listener(message);
  }

  private connectSse(): void {
    if (this.stopped) return;
    try {
      const source = new EventSource(`${this.base}/api/events`);
      this.eventSource = source;
      source.onopen = () => this.setMode("sse");
      source.onmessage = (event) => {
        try {
          this.emit(JSON.parse(event.data) as ServerMessage);
        } catch {
          return;
        }
      };
      source.onerror = () => {
        if (this.mode !== "sse") {
          source.close();
          this.eventSource = null;
          this.startPolling();
        }
      };
    } catch {
      this.startPolling();
    }
  }

  private startPolling(): void {
    if (this.polling || this.stopped) return;
    this.polling = true;
    this.setMode("polling");
    void this.poll();
  }

  private async poll(): Promise<void> {
    while (this.polling && !this.stopped) {
      try {
        const response = await fetch(`${this.base}/api/poll?cursor=${this.cursor}`);
        if (!response.ok) throw new Error(`poll ${response.status}`);
        const data = (await response.json()) as { cursor: number; messages: ServerMessage[] };
        this.cursor = data.cursor;
        for (const message of data.messages) this.emit(message);
      } catch {
        await new Promise((resolve) => setTimeout(resolve, 2000));
      }
    }
  }
}
