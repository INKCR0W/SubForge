export const SUBSCRIPTION_FORMATS = [
  { key: "clash", label: "Clash/Mihomo" },
  { key: "sing-box", label: "sing-box" },
  { key: "base64", label: "Base64" },
  { key: "raw", label: "Raw JSON" },
] as const;

export type ProfileFormMode = "create" | "edit";
