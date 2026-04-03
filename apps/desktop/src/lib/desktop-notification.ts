import type { CoreEventPayload } from "../types/core";

const WARNING_EVENTS = new Set(["refresh:failed", "refresh:error", "source:degraded"]);
const NOTIFICATION_THROTTLE_MS = 5_000;
const RECENT_NOTICE = new Map<string, number>();

let permissionRequest: Promise<NotificationPermission> | null = null;

export async function notifyDesktopForCoreEvent(event: CoreEventPayload): Promise<void> {
  if (!WARNING_EVENTS.has(event.event)) {
    return;
  }
  if (typeof window === "undefined" || typeof window.Notification === "undefined") {
    return;
  }

  const noticeKey = `${event.event}:${event.sourceId ?? "unknown"}`;
  const now = Date.now();
  const previous = RECENT_NOTICE.get(noticeKey);
  if (previous && now - previous < NOTIFICATION_THROTTLE_MS) {
    return;
  }
  RECENT_NOTICE.set(noticeKey, now);

  const permission = await ensureNotificationPermission();
  if (permission !== "granted") {
    return;
  }

  const title = toNotificationTitle(event.event);
  const body = buildNotificationBody(event);
  new Notification(title, {
    body,
    tag: `subforge-${noticeKey}`,
  });
}

function toNotificationTitle(eventName: string): string {
  switch (eventName) {
    case "source:degraded":
      return "SubForge 来源降级";
    case "refresh:failed":
    case "refresh:error":
      return "SubForge 刷新失败";
    default:
      return "SubForge 运行告警";
  }
}

function buildNotificationBody(event: CoreEventPayload): string {
  const sourcePart = event.sourceId ? `[${event.sourceId}] ` : "";
  const message = event.message?.trim();
  if (!message) {
    return `${sourcePart}${event.event}`;
  }
  return `${sourcePart}${message}`;
}

async function ensureNotificationPermission(): Promise<NotificationPermission> {
  const current = Notification.permission;
  if (current === "granted" || current === "denied") {
    return current;
  }
  if (!permissionRequest) {
    permissionRequest = Notification.requestPermission().finally(() => {
      permissionRequest = null;
    });
  }
  return permissionRequest;
}
