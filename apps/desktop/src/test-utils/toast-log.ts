import type { ToastManagerEvent } from "@base-ui/react/toast";
import { toast } from "@/components/ui/toast";
import type { ToastKind } from "@/state/toast-store";

export interface LoggedToast {
  id: string;
  kind: ToastKind | undefined;
  title: string | undefined;
  description: string | undefined;
}

const records = new Map<string, LoggedToast>();

// The Base UI manager does not expose its queue, so tests observe its events.
toast[" subscribe"]((event: ToastManagerEvent) => {
  if (event.action === "add") {
    const item = event.options as {
      id: string;
      type?: ToastKind;
      title?: string;
      description?: string;
    };
    records.set(item.id, {
      id: item.id,
      kind: item.type,
      title: item.title,
      description: item.description,
    });
    return;
  }

  if (event.action === "update") {
    const { id, updates } = event.options as {
      id: string;
      updates: Partial<LoggedToast>;
    };
    const current = records.get(id);
    if (current) {
      records.set(id, { ...current, ...updates, id });
    }
    return;
  }

  if (event.action === "close") {
    const { id } = event.options as { id?: string };
    if (id) {
      records.delete(id);
    } else {
      records.clear();
    }
  }
});

export function getToasts(): LoggedToast[] {
  return [...records.values()];
}

export function clearToasts(): void {
  records.clear();
}
