export function formatTimestamp(
  value: string | null | undefined,
  empty = "-",
): string {
  if (!value) {
    return empty;
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return value;
  }
  return parsed.toLocaleString("zh-CN", { hour12: false });
}

export function statusToneClass(status: string): string {
  if (status === "healthy" || status === "enabled" || status === "success") {
    return "ui-badge-success";
  }
  if (status === "degraded" || status === "running") {
    return "ui-badge-warning";
  }
  if (status === "offline" || status === "disabled") {
    return "ui-badge-muted";
  }
  return "ui-badge-danger";
}

export async function copyTextToClipboard(value: string): Promise<boolean> {
  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return true;
    } catch {
      // 忽略并降级到 textarea 方案。
    }
  }

  try {
    const textarea = document.createElement("textarea");
    textarea.value = value;
    textarea.setAttribute("readonly", "true");
    textarea.style.position = "absolute";
    textarea.style.left = "-9999px";
    document.body.appendChild(textarea);
    textarea.select();
    const copied = document.execCommand("copy");
    document.body.removeChild(textarea);
    return copied;
  } catch {
    return false;
  }
}
