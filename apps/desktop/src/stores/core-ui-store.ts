import { create } from "zustand";
import type {
  CoreConnectionPhase,
  CoreEventPayload,
  CoreStatus,
  WindowCloseBehavior,
} from "../types/core";

type CoreUiState = {
  phase: CoreConnectionPhase;
  status: CoreStatus | null;
  error: string | null;
  heartbeatAt: string | null;
  eventStreamActive: boolean;
  lastEvent: CoreEventPayload | null;
  eventHistory: CoreEventPayload[];
  lastRefreshAt: string | null;
  theme: "dark" | "light";
  autostartEnabled: boolean;
  idleAutoCloseMinutes: number;
  windowCloseBehavior: WindowCloseBehavior;
  toasts: ToastMessage[];
  setPhase: (phase: CoreConnectionPhase) => void;
  setStatus: (status: CoreStatus | null) => void;
  setError: (error: string | null) => void;
  setHeartbeatAt: (heartbeatAt: string | null) => void;
  setEventStreamActive: (active: boolean) => void;
  pushEvent: (event: CoreEventPayload) => void;
  setLastRefreshAt: (timestamp: string | null) => void;
  setTheme: (theme: "dark" | "light") => void;
  setAutostartEnabled: (enabled: boolean) => void;
  setIdleAutoCloseMinutes: (minutes: number) => void;
  setWindowCloseBehavior: (behavior: WindowCloseBehavior) => void;
  addToast: (toast: Omit<ToastMessage, "id">) => string;
  removeToast: (id: string) => void;
};

export type ToastMessage = {
  id: string;
  title: string;
  description: string;
  variant: "default" | "warning" | "error";
};

export const useCoreUiStore = create<CoreUiState>((set) => ({
  phase: "idle",
  status: null,
  error: null,
  heartbeatAt: null,
  eventStreamActive: false,
  lastEvent: null,
  eventHistory: [],
  lastRefreshAt: null,
  theme: "dark",
  autostartEnabled: false,
  idleAutoCloseMinutes: 30,
  windowCloseBehavior: "tray_minimize",
  toasts: [],
  setPhase: (phase) => set({ phase }),
  setStatus: (status) => set({ status }),
  setError: (error) => set({ error }),
  setHeartbeatAt: (heartbeatAt) => set({ heartbeatAt }),
  setEventStreamActive: (eventStreamActive) => set({ eventStreamActive }),
  pushEvent: (event) =>
    set((state) => ({
      lastEvent: event,
      eventHistory: [event, ...state.eventHistory].slice(0, 25),
    })),
  setLastRefreshAt: (lastRefreshAt) => set({ lastRefreshAt }),
  setTheme: (theme) => set({ theme }),
  setAutostartEnabled: (autostartEnabled) => set({ autostartEnabled }),
  setIdleAutoCloseMinutes: (idleAutoCloseMinutes) => set({ idleAutoCloseMinutes }),
  setWindowCloseBehavior: (windowCloseBehavior) => set({ windowCloseBehavior }),
  addToast: (toast) => {
    const id = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    set((state) => ({ toasts: [...state.toasts, { ...toast, id }] }));
    return id;
  },
  removeToast: (id) =>
    set((state) => ({
      toasts: state.toasts.filter((toast) => toast.id !== id),
    })),
}));
