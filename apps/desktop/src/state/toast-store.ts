import { toast } from "@/components/ui/toast";

export type ToastKind = "info" | "success" | "warning" | "error";

export interface ToastMessage {
  id?: string;
  kind: ToastKind;
  title: string;
  description?: string;
}

const TIMEOUT_BY_KIND: Record<ToastKind, number> = {
  info: 4000,
  success: 5000,
  warning: 7000,
  error: 8000,
};

/**
 * Pushes a toast into the shadcn (Base UI) toast manager. The manager owns the
 * visible queue, so closing and auto-dismiss stay in one place.
 */
export function pushToast(message: ToastMessage): string {
  return toast.add({
    id: message.id,
    type: message.kind,
    title: message.title,
    description: message.description,
    priority: message.kind === "error" ? "high" : "low",
    timeout: TIMEOUT_BY_KIND[message.kind],
  });
}
