import { copyTextToClipboard, formatTimestamp as formatTimestampValue } from "../../lib/ui";

export function buildSubscriptionUrl(
  baseUrl: string,
  profileId: string,
  format: string,
  token: string,
): string {
  const normalizedBase = baseUrl.endsWith("/") ? baseUrl.slice(0, -1) : baseUrl;
  return `${normalizedBase}/api/profiles/${encodeURIComponent(profileId)}/${format}?token=${encodeURIComponent(token)}`;
}

export async function copySubscriptionUrl(url: string): Promise<boolean> {
  return copyTextToClipboard(url);
}

export function formatTimestamp(value: string): string {
  return formatTimestampValue(value, value);
}
