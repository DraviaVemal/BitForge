export type ConfirmOptions = {
  title?: string;
  confirmText?: string;
  cancelText?: string;
  danger?: boolean;
};

export type ConfirmRequest = ConfirmOptions & {
  message: string;
  resolve: (ok: boolean) => void;
};

let listener: ((request: ConfirmRequest) => void) | null = null;

export function setConfirmListener(fn: ((request: ConfirmRequest) => void) | null): void {
  listener = fn;
}

export function confirmDialog(message: string, options: ConfirmOptions = {}): Promise<boolean> {
  return new Promise((resolve) => {
    if (!listener) {
      resolve(window.confirm(message));
      return;
    }
    listener({ message, ...options, resolve });
  });
}
