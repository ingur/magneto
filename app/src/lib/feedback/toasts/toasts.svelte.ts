// Toast notifications.
//
//   toast.info('Saved')
//   toast.success('Download complete')
//   toast.warn('Disk space low')
//   toast.error('Failed to connect', { duration: 5000 })
//   toast.dismiss()        // clear all
//   toast.dismiss(id)      // clear one
//
// Stack lives bottom-right of the browser+overlay region (never over the
// StatusBar). Auto-dismiss after `duration` ms (default 3000). Set
// duration to 0 to keep the toast until manually dismissed.

export type ToastKind = "info" | "success" | "warn" | "error";

export type Toast = {
  id: number;
  kind: ToastKind;
  message: string;
  duration: number;
  action?: { label: string; onClick: () => void };
};

export type ToastOpts = {
  duration?: number;
  action?: { label: string; onClick: () => void };
};

const DEFAULT_DURATION = 3000;
const MAX_VISIBLE = 5;

let nextId = 0;

class ToastsStore {
  list = $state<Toast[]>([]);

  show(kind: ToastKind, message: string, opts: ToastOpts = {}): number {
    const t: Toast = {
      id: ++nextId,
      kind,
      message,
      duration: opts.duration ?? DEFAULT_DURATION,
      action: opts.action,
    };
    this.list.push(t);
    // Evict oldest when over the visible cap; keeps the stack a fixed
    // size regardless of how many toasts get fired in a burst.
    if (this.list.length > MAX_VISIBLE) this.list.shift();
    if (t.duration > 0) {
      setTimeout(() => this.dismiss(t.id), t.duration);
    }
    return t.id;
  }

  dismiss(id?: number) {
    if (id === undefined) this.list = [];
    else this.list = this.list.filter((t) => t.id !== id);
  }
}

export const toasts = new ToastsStore();

export const toast = {
  info: (msg: string, opts?: ToastOpts) => toasts.show("info", msg, opts),
  success: (msg: string, opts?: ToastOpts) => toasts.show("success", msg, opts),
  warn: (msg: string, opts?: ToastOpts) => toasts.show("warn", msg, opts),
  error: (msg: string, opts?: ToastOpts) => toasts.show("error", msg, opts),
  dismiss: (id?: number) => toasts.dismiss(id),
};
